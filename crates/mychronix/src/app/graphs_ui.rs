//! The Analysis page: the channel list beside the graphs, which view shows,
//! and the Graphs view itself (charts and the track map beside them).

use super::*;

impl App {
    pub(super) fn channel_list(&mut self, ui: &mut Ui, session: &Session, start_ms: i64) {
        if !self.analysis.collapsed_init {
            self.analysis.collapsed_init = true;
            self.analysis.collapsed_groups.insert("Lap timing");
            self.analysis.collapsed_groups.insert("More channels");
            // MYCHRONIX_CHANNEL_SEARCH pre-fills the search (for screenshots).
            if let Some(q) = &crate::hooks::get().channel_search {
                self.analysis.filter = q.clone();
            }
        }
        let live = session.channels.iter().filter(|c| !is_dead(c)).count();
        section_header(
            ui,
            "Channels",
            &format!("{} graphed · {live}", self.analysis.selected.len()),
        );
        search_field(ui, &mut self.analysis.filter, "Search channels");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (view, label) in [(ChannelView::All, "All"), (ChannelView::Graphed, "Graphed")] {
                if chip(ui, label, self.analysis.channel_view == view).clicked() {
                    self.analysis.channel_view = view;
                }
            }
        });
        ui.add_space(6.0);

        let needle = self.analysis.filter.to_lowercase();
        let cursor_ms = self
            .analysis
            .cursor_s
            .map(|s| start_ms + (s * 1000.0) as i64);
        let full = self.analysis.selected.len() >= SERIES.len();

        egui::ScrollArea::vertical()
            .id_salt("channels")
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut toggle_group = None;
                for group in CHANNEL_GROUPS {
                    let members: Vec<usize> = session
                        .channels
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| {
                            channel_group(c, session.info.imu_mount.is_some()) == group
                                && !is_dead(c)
                        })
                        .filter(|(_, c)| {
                            needle.is_empty() || c.name.to_lowercase().contains(&needle)
                        })
                        .filter(|(i, _)| {
                            self.analysis.channel_view != ChannelView::Graphed
                                || self.analysis.selected.contains_key(i)
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if members.is_empty() {
                        continue;
                    }
                    // Searching or showing graphed channels opens every group.
                    let open = !needle.is_empty()
                        || self.analysis.channel_view == ChannelView::Graphed
                        || !self.analysis.collapsed_groups.contains(group);
                    let graphed = members
                        .iter()
                        .filter(|i| self.analysis.selected.contains_key(i))
                        .count();
                    let header = ui.horizontal(|ui| {
                        ui.add(
                            (if open {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            })
                            .image(DIM, 13.0),
                        );
                        ui.label(RichText::new(group).font(medium(13.0)).color(SECONDARY));
                        let detail = if graphed > 0 {
                            format!("{graphed} of {} graphed", members.len())
                        } else {
                            members.len().to_string()
                        };
                        ui.label(RichText::new(detail).size(12.0).color(DIM));
                    });
                    let header = ui.interact(
                        header.response.rect,
                        ui.id().with(("group", group)),
                        Sense::click(),
                    );
                    if header.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if header.clicked() {
                        toggle_group = Some(group);
                    }
                    if !open {
                        ui.add_space(4.0);
                        continue;
                    }
                    for i in members {
                        let c = &session.channels[i];
                        let slot = self.analysis.selected.get(&i).copied();
                        let response = row(ui, false, |ui| {
                            swatch(ui, slot.map(|s| SERIES[s]));
                            ui.add_space(2.0);
                            let name_color = if slot.is_some() { TEXT } else { SECONDARY };
                            ui.label(RichText::new(&c.name).color(name_color));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let units = Shown::of(c).unit();
                                if !units.is_empty() {
                                    ui.label(RichText::new(units).size(12.0).color(DIM));
                                }
                                if let Some(v) = cursor_ms.and_then(|t| sample_at(c, t)) {
                                    ui.label(
                                        RichText::new(format_value(v, Shown::of(c).decimals(c)))
                                            .font(medium(13.5))
                                            .color(TEXT),
                                    );
                                }
                            });
                        });
                        let response = response.on_hover_text(if slot.is_some() {
                            "Click to remove its graph"
                        } else {
                            "Click to graph"
                        });
                        if response.clicked() {
                            if slot.is_some() {
                                self.analysis.selected.remove(&i);
                                self.analysis.graph_order.retain(|&c| c != i);
                            } else if !full {
                                self.select(i);
                                // Show the new graph, which goes at the bottom of the stack.
                                self.analysis.scroll_to_graph = Some(i);
                            }
                        }
                    }
                    ui.add_space(6.0);
                }
                if let Some(g) = toggle_group
                    && !self.analysis.collapsed_groups.remove(g)
                {
                    self.analysis.collapsed_groups.insert(g);
                }
            });
    }
}

// ---------------------------------------------------------------------------
// Analysis page

impl App {
    pub(super) fn analysis(
        &mut self,
        ui: &mut Ui,
        session: &std::sync::Arc<Session>,
        start_ms: i64,
        end_ms: i64,
    ) {
        // A view that can't work for this session (no GPS) falls back to Graphs.
        if analysis_ui::view_unavailable(session, self.analysis.analysis_view.rail_item()).is_some()
        {
            self.analysis.analysis_view = AnalysisView::Session;
        }
        let pill = self.session_pill(ui, session);
        self.analysis.pill_rect = Some(pill);
        if let Some(i) = self.path.as_ref().and_then(|p| {
            self.library
                .as_ref()?
                .sessions
                .iter()
                .position(|s| &s.path == p)
        }) {
            self.want_weather(&[i], &ui.ctx().clone());
        }
        ui.add_space(14.0);
        let title = match self.analysis.analysis_view {
            AnalysisView::Report => "Laps",
            AnalysisView::Splits => "Splits",
            AnalysisView::Corners => "Corners",
            AnalysisView::Map => "Map",
            AnalysisView::Compare => "Compare",
            AnalysisView::Session | AnalysisView::Histogram | AnalysisView::Scatter => "Graphs",
        };
        // The map takes the whole view; everything else gets a title.
        if self.analysis.analysis_view != AnalysisView::Map {
            self.view_title(ui, title);
            ui.add_space(12.0);
        }
        self.analysis_view_body(ui, session, start_ms, end_ms);
        self.session_panel(&ui.ctx().clone(), session);
        self.setup_dialog(ui.ctx());
    }

    pub(super) fn analysis_view_body(
        &mut self,
        ui: &mut Ui,
        session: &std::sync::Arc<Session>,
        start_ms: i64,
        end_ms: i64,
    ) {
        match self.analysis.analysis_view {
            AnalysisView::Map => {
                self.map_view(ui, session);
                return;
            }
            AnalysisView::Compare => {
                self.compare_view(ui, session);
                return;
            }
            AnalysisView::Report => {
                self.report_view(ui, session);
                return;
            }
            AnalysisView::Corners => {
                self.corners_view(ui, session);
                return;
            }
            AnalysisView::Splits => {
                self.splits_view(ui, session);
                return;
            }
            AnalysisView::Histogram => {
                self.histogram_view(ui, session);
                return;
            }
            AnalysisView::Scatter => {
                self.scatter_view(ui, session);
                return;
            }
            AnalysisView::Session => {}
        }
        // Up and down open the lap before or after ("All" comes first).
        let typing = ui.ctx().egui_wants_keyboard_input();
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp) && !typing,
                i.key_pressed(egui::Key::ArrowDown) && !typing,
            )
        });
        if up || down {
            let n = session.laps.len();
            let lap = match (self.analysis.lap, up) {
                (Some(0), true) | (None, true) => None,
                (Some(k), true) => Some(k - 1),
                (None, false) if n > 0 => Some(0),
                (Some(k), false) => Some((k + 1).min(n.saturating_sub(1))),
                (None, false) => None,
            };
            if lap != self.analysis.lap {
                // The same moment into the next lap; the whole session starts over.
                if lap.is_none() || self.analysis.lap.is_none() {
                    self.analysis.cursor_s = None;
                }
                self.analysis.lap = lap;
            }
        }
        self.lap_strip(ui, session, false);
        ui.add_space(12.0);

        let gap = 12.0;
        let height = ui.available_height();
        let map_w = if session.gps.is_some() {
            (ui.available_width() * 0.4).clamp(280.0, 560.0)
        } else {
            0.0
        };
        let graphs_w = ui.available_width() - if map_w > 0.0 { map_w + gap } else { 0.0 };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(graphs_w, height), column, |ui| {
                ui.set_width(graphs_w);
                // Leave room for "+ Add channel" under the graphs.
                let graphs_h = ui.available_height() - 44.0;
                ui.allocate_ui_with_layout(vec2(graphs_w, graphs_h), column, |ui| {
                    self.graphs(ui, session, start_ms, end_ms)
                });
                ui.add_space(8.0);
                self.channel_picker(ui, session, start_ms);
            });
            if map_w > 0.0 {
                ui.allocate_ui_with_layout(vec2(map_w, height), column, |ui| {
                    ui.set_width(map_w);
                    self.track_map(ui, session, start_ms, end_ms)
                });
            }
        });
    }

    pub(super) fn graphs(&mut self, ui: &mut Ui, session: &Session, start_ms: i64, end_ms: i64) {
        if self.analysis.selected.is_empty() {
            card(ui, |ui| {
                ui.set_min_height(180.0);
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Add a channel below to graph it.").color(SECONDARY));
                });
            });
            return;
        }

        // In the order they were added; newest at the bottom.
        let selected: Vec<(usize, usize)> = self
            .analysis
            .graph_order
            .iter()
            .filter_map(|c| self.analysis.selected.get(c).map(|&s| (*c, s)))
            .collect();
        let gap = 10.0;
        // RaceStudio's Time Compare: time gained or lost against the best lap,
        // along this lap (shown when a lap other than the best is open).
        let reference = self.time_compare_reference(session, start_ms);
        let compare_h = 120.0;
        let n = selected.len() as f32;
        let reserved = if reference.is_some() {
            compare_h + gap
        } else {
            0.0
        };
        let card_h = ((ui.available_height() - reserved - gap * (n - 1.0)) / n).max(110.0);
        let scroll_to = self.analysis.scroll_to_graph.take();
        let mut remove = None;
        let mut axis = self.graph_axis(ui, session, start_ms, end_ms);
        // Seconds behind the best lap at each point of this one, on this graph's x axis.
        let time_compare: Option<(aim_xrk::Lap, Vec<[f64; 2]>)> = reference.and_then(|best| {
            let points = time_compare_curve(session, best, start_ms, end_ms)?
                .into_iter()
                .map(|(t_ms, d, gap_s)| {
                    [
                        if axis.profile.is_some() {
                            d
                        } else {
                            (t_ms - start_ms as f64) / 1000.0
                        },
                        gap_s,
                    ]
                })
                .collect();
            Some((best, points))
        });
        let cursor_ms = self
            .analysis
            .cursor_s
            .map(|s| start_ms + (s * 1000.0) as i64);
        let mut hovered = None;
        let view_id = ui.id().with("graph_view");
        axis.pan = self.graph_arrow_keys(ui, &axis, view_id);
        let mut seen_view = None;

        egui::ScrollArea::vertical()
            .id_salt("graphs")
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = gap;
                for (k, &(i, slot)) in selected.iter().enumerate() {
                    let c = &session.channels[i];
                    let last = k + 1 == selected.len() && time_compare.is_none();
                    let top = ui.cursor().top();
                    graph_card(ui, card_h, |ui| {
                        if channel_graph_header(ui, c, SERIES[slot], cursor_ms) {
                            remove = Some(i);
                        }
                        let (seen, over) = self.channel_graph(ui, c, i, SERIES[slot], last, &axis);
                        seen_view.get_or_insert(seen);
                        if over.is_some() {
                            hovered = over;
                        }
                    });
                    if scroll_to == Some(i) {
                        let rect = Rect::from_min_max(
                            pos2(ui.min_rect().left(), top),
                            pos2(ui.min_rect().right(), ui.cursor().top()),
                        );
                        ui.scroll_to_rect(rect, Some(Align::Max));
                    }
                }
                if let Some((best, points)) = &time_compare {
                    graph_card(ui, compare_h, |ui| {
                        let over = self.time_compare_graph(ui, session, *best, points, &axis);
                        if over.is_some() {
                            hovered = over;
                        }
                    });
                }
            });
        if let Some(r) = seen_view {
            ui.data_mut(|d| d.insert_temp(view_id, (*r.start(), *r.end())));
        }
        if hovered.is_some() && !self.analysis.cursor_by_keys {
            self.analysis.cursor_s = hovered;
        }
        if let Some(i) = remove {
            self.analysis.selected.remove(&i);
            self.analysis.graph_order.retain(|&c| c != i);
        }
    }

    /// The graphs' x axis for the window from `start_ms` to `end_ms`. A new
    /// window (another lap, the whole session, or the other axis) starts the
    /// graphs at its full length.
    fn graph_axis(&mut self, ui: &Ui, session: &Session, start_ms: i64, end_ms: i64) -> GraphAxis {
        // Distance mode: meters along the lap from GPS speed (time mode without GPS).
        let profile = if self.analysis.graph_distance {
            crate::compare::DistanceProfile::from_gps(
                session,
                &aim_xrk::Lap {
                    number: 0,
                    start_ms,
                    end_ms,
                },
            )
        } else {
            None
        };
        let x_max = profile
            .as_ref()
            .map(|p| p.length())
            .unwrap_or((end_ms - start_ms) as f64 / 1000.0)
            .max(0.001);
        let key = (start_ms, end_ms, profile.is_some());
        let window_changed = self.analysis.graph_window != Some(key);
        self.analysis.graph_window = Some(key);
        GraphAxis {
            unit: if profile.is_some() { "m" } else { "s" },
            profile,
            start_ms,
            end_ms,
            x_max,
            window_changed,
            link: ui.id().with("time"),
            pan: None,
        }
    }

    /// Left and right move the cursor along the graphs, a small step of what's
    /// in view (Shift: ten times that); the view follows it when it leaves.
    /// Returns where the view moves to, if it does.
    fn graph_arrow_keys(
        &mut self,
        ui: &Ui,
        axis: &GraphAxis,
        view_id: egui::Id,
    ) -> Option<(f64, f64)> {
        let typing = ui.ctx().egui_wants_keyboard_input();
        let step = ui.input(|i| {
            let dir = if typing {
                0.0
            } else if i.key_pressed(egui::Key::ArrowRight) {
                1.0
            } else if i.key_pressed(egui::Key::ArrowLeft) {
                -1.0
            } else {
                0.0
            };
            dir * if i.modifiers.shift { 0.05 } else { 0.005 }
        });
        let x_max = axis.x_max;
        let (view_lo, view_hi) = ui
            .data(|d| d.get_temp::<(f64, f64)>(view_id))
            .filter(|_| !axis.window_changed)
            .unwrap_or((0.0, x_max));
        let mut pan = None;
        if step != 0.0 {
            let x = self
                .analysis
                .cursor_s
                .map(|s| axis.cursor_x(s))
                .unwrap_or(view_lo);
            let x = (x + step * (view_hi - view_lo)).clamp(0.0, x_max);
            self.analysis.cursor_s = Some(match &axis.profile {
                Some(p) => ((p.time_at(x) - axis.start_ms as f64) / 1000.0).max(0.0),
                None => x,
            });
            self.analysis.cursor_by_keys = true;
            let width = view_hi - view_lo;
            if x < view_lo || x > view_hi {
                let lo = (x - width / 2.0).clamp(0.0, (x_max - width).max(0.0));
                pan = Some((lo, lo + width));
            }
        }
        if ui.input(|i| i.pointer.is_moving()) {
            self.analysis.cursor_by_keys = false;
        }
        pan
    }

    /// A channel's graph under its header, linked to the others. Returns the
    /// x range in view, and the cursor (seconds into the window) where the
    /// pointer is over it.
    fn channel_graph(
        &self,
        ui: &mut Ui,
        c: &Channel,
        i: usize,
        color: Color32,
        last: bool,
        axis: &GraphAxis,
    ) -> (std::ops::RangeInclusive<f64>, Option<f64>) {
        let start_ms = axis.start_ms;
        let mut points = decimate(c, start_ms, axis.end_ms, MAX_POINTS);
        if axis.profile.is_some() {
            for p in &mut points {
                p[0] = axis.to_x(start_ms as f64 + p[0] * 1000.0);
            }
        }
        let calculated = is_calculated(&c.name);
        let height = ui.available_height() - if calculated { 20.0 } else { 0.0 };
        let response = axis.plot(("channel", i), height, last).show(ui, |plot| {
            if let Some((lo, hi)) = axis.pan {
                plot.set_plot_bounds_x(lo..=hi);
            }
            plot.line(
                Line::new(c.name.clone(), PlotPoints::from(points))
                    .color(color)
                    .width(2.0),
            );
            if let Some(s) = self.analysis.cursor_s {
                plot.vline(
                    VLine::new("cursor", axis.cursor_x(s))
                        .color(SECONDARY)
                        .width(1.0),
                );
            }
            // Only over this graph: the pointer elsewhere (on the track map,
            // say) has a coordinate too, off the graph's ends.
            plot.response()
                .hovered()
                .then(|| plot.pointer_coordinate())
                .flatten()
        });
        let seen = response.transform.bounds().range_x();
        if calculated {
            calculated_note(ui);
        }
        (seen, response.inner.map(|p| axis.cursor_at(p.x)))
    }

    /// Time compare under its header: seconds behind the `best` lap along
    /// this one (`points`, on the graphs' x axis). Returns the cursor where
    /// the pointer is over it.
    fn time_compare_graph(
        &self,
        ui: &mut Ui,
        session: &Session,
        best: aim_xrk::Lap,
        points: &[[f64; 2]],
        axis: &GraphAxis,
    ) -> Option<f64> {
        // The gap at the cursor: above zero, behind the best lap there.
        let at_cursor = self.analysis.cursor_s.and_then(|s| {
            let x = axis.cursor_x(s);
            let i = points.partition_point(|p| p[0] < x);
            points
                .get(i.min(points.len().saturating_sub(1)))
                .map(|p| p[1])
        });
        ui.horizontal(|ui| {
            ui.add(Icon::Timer.image(DIM, 14.0));
            ui.label(RichText::new("Time compare").font(medium(14.5)).color(TEXT));
            let n = session
                .laps
                .iter()
                .position(|l| *l == best)
                .map(|i| i + 1)
                .unwrap_or(0);
            ui.label(
                RichText::new(format!(
                    "vs best lap {n}  ·  {}",
                    format_lap_time(best.end_ms - best.start_ms)
                ))
                .size(13.0)
                .color(DIM),
            )
            .on_hover_text(
                "Above the line: behind the best lap at that point. Below: ahead of it.",
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(g) = at_cursor {
                    let (text, color) = if g > 0.0005 {
                        (format!("+{g:.3} s"), theme::FAVORITE)
                    } else if g < -0.0005 {
                        (format!("−{:.3} s", -g), theme::GOOD)
                    } else {
                        ("0.000 s".to_string(), TEXT)
                    };
                    ui.label(RichText::new(text).font(semibold(16.0)).color(color));
                }
            });
        });
        let response = axis
            .plot("time_compare", ui.available_height(), true)
            .include_y(0.0)
            .y_axis_formatter(|mark, _| format!("{:+.1} s", mark.value))
            .show(ui, |plot| {
                if let Some((lo, hi)) = axis.pan {
                    plot.set_plot_bounds_x(lo..=hi);
                }
                plot.hline(egui_plot::HLine::new("even", 0.0).color(DIM).width(1.0));
                plot.line(
                    Line::new("Time compare", PlotPoints::from(points.to_vec()))
                        .color(SECONDARY)
                        .width(2.0),
                );
                if let Some(s) = self.analysis.cursor_s {
                    plot.vline(
                        VLine::new("cursor", axis.cursor_x(s))
                            .color(SECONDARY)
                            .width(1.0),
                    );
                }
                plot.response()
                    .hovered()
                    .then(|| plot.pointer_coordinate())
                    .flatten()
            });
        response.inner.map(|p| axis.cursor_at(p.x))
    }
}

/// A channel graph's header: its color, name and units, the value at the
/// cursor, and × to remove the graph. True when × was clicked.
fn channel_graph_header(ui: &mut Ui, c: &Channel, color: Color32, cursor_ms: Option<i64>) -> bool {
    let mut remove = false;
    ui.horizontal(|ui| {
        let (key, _) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
        ui.painter().line_segment(
            [
                pos2(key.left(), key.center().y),
                pos2(key.right(), key.center().y),
            ],
            Stroke::new(2.5, color),
        );
        ui.label(RichText::new(&c.name).font(medium(14.5)).color(TEXT));
        let units = Shown::of(c).unit();
        if !units.is_empty() {
            ui.label(RichText::new(units).size(13.0).color(DIM));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let x = ui
                .add(Icon::X.image(DIM, 13.0).sense(Sense::click()))
                .on_hover_text("Remove this graph");
            if x.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if x.clicked() {
                remove = true;
            }
            ui.add_space(8.0);
            // The value at the cursor, big, with its units ("6889 rpm").
            if let Some(v) = cursor_ms.and_then(|t| sample_at(c, t)) {
                ui.spacing_mut().item_spacing.x = 5.0;
                if !units.is_empty() {
                    ui.label(RichText::new(units).font(medium(16.0)).color(SECONDARY));
                }
                ui.label(
                    RichText::new(format_value(v, Shown::of(c).decimals(c)))
                        .font(semibold(26.0))
                        .color(TEXT),
                );
            }
        });
    });
    remove
}

/// The graphs' shared x axis: seconds into the window, or meters along it
/// on the distance axis.
struct GraphAxis {
    /// Distance along the window, on the distance axis.
    profile: Option<crate::compare::DistanceProfile>,
    start_ms: i64,
    end_ms: i64,
    /// The axis's length.
    x_max: f64,
    unit: &'static str,
    /// A new window this frame: the graphs start over at its full length.
    window_changed: bool,
    /// The group the graphs' axes and cursors are linked in.
    link: egui::Id,
    /// Where the arrow keys moved the view to, this frame.
    pan: Option<(f64, f64)>,
}

impl GraphAxis {
    /// Where logger time `t_ms` falls on the axis.
    fn to_x(&self, t_ms: f64) -> f64 {
        match &self.profile {
            Some(p) => p.dist_at(t_ms),
            None => (t_ms - self.start_ms as f64) / 1000.0,
        }
    }

    /// Where the cursor, `s` seconds into the window, falls on the axis.
    fn cursor_x(&self, s: f64) -> f64 {
        self.to_x(self.start_ms as f64 + s * 1000.0)
    }

    /// The cursor (seconds into the window) at `x` on the axis. The cursor
    /// is kept as time, so the map and readouts follow it either way.
    fn cursor_at(&self, x: f64) -> f64 {
        match &self.profile {
            Some(p) => {
                ((p.time_at(x.clamp(0.0, self.x_max)) - self.start_ms as f64) / 1000.0).max(0.0)
            }
            None => x.max(0.0),
        }
    }

    /// A graph on this axis, `height` tall and linked to the others, with
    /// the axis's labels under it when `x_axis`.
    fn plot<'a>(&self, id: impl egui::AsId, height: f32, x_axis: bool) -> Plot<'a> {
        let unit = self.unit;
        let mut plot = Plot::new(id);
        if self.window_changed {
            plot = plot.reset();
        }
        plot.height(height)
            .show_background(false)
            .grid_color(GRID)
            .show_axes([x_axis, true])
            // Wide enough for six characters ("-12000"), so stacked graphs line up.
            .y_axis_min_width(58.0)
            .show_x(false)
            .show_y(false)
            .cursor_color(DIM)
            .link_axis(self.link, [true, false])
            .link_cursor(self.link, [true, false])
            .allow_zoom([true, false])
            .allow_drag([true, false])
            .allow_scroll(false)
            // Every graph spans the whole lap, even a channel with few samples. A fixed
            // range (not auto bounds): linked auto bounds feed each other's margins and grow.
            .auto_bounds([false, true])
            .default_x_bounds(0.0, self.x_max)
            .x_axis_formatter(move |mark, _| {
                if mark.step_size < 1.0 {
                    format!("{:.1} {unit}", mark.value)
                } else {
                    format!("{:.0} {unit}", mark.value)
                }
            })
    }
}

/// Along the lap from `start_ms` to `end_ms`: (logger time ms, meters into
/// the lap, seconds behind `best` there).
pub(super) fn time_compare_curve(
    session: &Session,
    best: aim_xrk::Lap,
    start_ms: i64,
    end_ms: i64,
) -> Option<Vec<(f64, f64, f64)>> {
    let lap = crate::compare::DistanceProfile::from_gps(
        session,
        &aim_xrk::Lap {
            number: 0,
            start_ms,
            end_ms,
        },
    )?;
    let reference = crate::compare::DistanceProfile::from_gps(session, &best)?;
    Some(crate::compare::gap_along(&lap, &reference, 600))
}

impl App {
    /// The best lap, when the open lap is another one (what Time Compare measures against).
    pub(super) fn time_compare_reference(
        &self,
        session: &Session,
        start_ms: i64,
    ) -> Option<aim_xrk::Lap> {
        let best = session.best_lap(MIN_LAP_MS).copied();
        self.analysis
            .lap
            .and_then(|i| session.laps.get(i))
            .filter(|l| l.start_ms == start_ms && Some(**l) != best)
            .and(best)
            .filter(|b| b.end_ms - b.start_ms >= MIN_LAP_MS)
    }

    /// The GPS path for the current window, colored by a channel (or by where
    /// time is gained and lost against the best lap), with a dot at the shared
    /// cursor. Hovering the map moves the cursor.
    pub(super) fn track_map(&mut self, ui: &mut Ui, session: &Session, start_ms: i64, end_ms: i64) {
        let Some(gps) = &session.gps else { return };
        let lo = gps.times_ms.partition_point(|&t| t < start_ms);
        let hi = gps.times_ms.partition_point(|&t| t <= end_ms);
        let coloring = self.map_coloring(session, start_ms, end_ms);
        let cursor_ms = self
            .analysis
            .cursor_s
            .map(|s| start_ms + (s * 1000.0) as i64);
        let mut pick = None;
        let mut hovered = None;

        Frame::new()
            .fill(SURFACE)
            .corner_radius(CornerRadius::same(theme::RADIUS))
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 12,
                bottom: 12,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height() - 24.0);
                pick = map_header(ui, session, &coloring);

                let legend_h = 34.0;
                let (rect, response) = ui.allocate_exact_size(
                    vec2(
                        ui.available_width(),
                        (ui.available_height() - legend_h).max(80.0),
                    ),
                    Sense::hover(),
                );
                if hi <= lo + 1 {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "No GPS fix in this part of the session",
                        regular(13.0),
                        DIM,
                    );
                    return;
                }
                let pts = map_points(gps, lo, hi, rect);
                let channel = coloring.channel.map(|i| &session.channels[i]);
                let painter = ui.painter_at(rect);
                let ramp = paint_map_line(&painter, gps, lo, hi, &pts, channel, &coloring);
                // Where the window starts.
                painter.circle_filled(pts[0], 4.0, DIM);

                // Hover: snap the shared cursor to the nearest point.
                if let Some(pointer) = response.hover_pos() {
                    let (k, d) = pts
                        .iter()
                        .enumerate()
                        .map(|(k, p)| (k, p.distance_sq(pointer)))
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .unwrap();
                    if d < 28.0 * 28.0 {
                        hovered = Some((gps.times_ms[lo + k] - start_ms) as f64 / 1000.0);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                }
                let units = channel.map(|c| Shown::of(c).unit()).unwrap_or("");
                let dec = channel.map(|c| Shown::of(c).decimals(c)).unwrap_or(0);
                if let Some(t) = cursor_ms {
                    if let Some(k) = gps
                        .index_at(t)
                        .filter(|&i| i >= lo && i < hi)
                        .map(|i| i - lo)
                    {
                        painter.circle_filled(pts[k], 7.0, SURFACE);
                        painter.circle_filled(pts[k], 5.0, TEXT);
                    }
                    let line = map_readout(gps, t, &coloring, channel, units, dec);
                    painter.text(
                        rect.left_top() + vec2(2.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        line,
                        medium(14.0),
                        TEXT,
                    );
                }

                // Legend: the color ramp and the range it covers.
                ui.add_space(6.0);
                if coloring.gain.is_some() {
                    gain_legend(ui);
                } else {
                    ramp_legend(ui, &ramp, dec, units);
                }
            });
        match pick {
            Some(MapColorPick::TimeCompare) => self.analysis.map_time_compare = true,
            Some(MapColorPick::Channel(i)) => {
                self.analysis.map_channel = Some(i);
                self.analysis.map_time_compare = false;
            }
            None => {}
        }
        if hovered.is_some() {
            self.analysis.cursor_s = hovered;
        }
    }

    /// What the track map colors the line by this frame.
    fn map_coloring(&self, session: &Session, start_ms: i64, end_ms: i64) -> MapColoring {
        // RPM by default, or speed when the session has no RPM recorded.
        let default = ["RPM", "GPS Speed"].iter().find_map(|n| {
            session
                .channels
                .iter()
                .position(|c| c.name == *n && !is_dead(c))
        });
        let channel = self
            .analysis
            .map_channel
            .filter(|&i| session.channels.get(i).is_some_and(|c| !is_dead(c)))
            .or(default)
            .or_else(|| self.analysis.selected.keys().next().copied());
        // Time compare coloring: how fast the gap to the best lap grows (losing)
        // or shrinks (gaining), per meter, at each point.
        let reference = self.time_compare_reference(session, start_ms);
        let gap_curve = reference
            .filter(|_| self.analysis.map_time_compare)
            .and_then(|best| time_compare_curve(session, best, start_ms, end_ms));
        let gain = gap_curve.as_ref().map(|c| {
            let rate = |k: usize| {
                let (a, b) = (c[k.saturating_sub(1)], c[(k + 1).min(c.len() - 1)]);
                (b.2 - a.2) / (b.1 - a.1).max(1e-6)
            };
            let rates: Vec<(f64, f64)> = (0..c.len()).map(|k| (c[k].0, rate(k))).collect();
            let scale = rates
                .iter()
                .map(|r| r.1.abs())
                .fold(0.0_f64, f64::max)
                .max(1e-9)
                * 0.6;
            (rates, scale)
        });
        MapColoring {
            channel,
            reference,
            gap_curve,
            gain,
        }
    }
}

/// What the track map colors its line by: a channel, or where this lap
/// gains and loses time against the best lap.
struct MapColoring {
    /// The channel, when coloring by one (or for the readout).
    channel: Option<usize>,
    /// The best lap, when the open lap is another one.
    reference: Option<aim_xrk::Lap>,
    /// Along the lap: (logger time ms, meters, seconds behind the best lap).
    gap_curve: Option<Vec<(f64, f64, f64)>>,
    /// Time compare coloring: (logger time ms, seconds lost per meter) along
    /// the lap, and the rate that's the full color.
    gain: Option<(Vec<(f64, f64)>, f64)>,
}

/// A pick from the track map's color-by menu.
enum MapColorPick {
    TimeCompare,
    Channel(usize),
}

/// A channel's colors over a range of its values.
struct MapRamp {
    colors: fn(f32) -> Color32,
    vmin: f64,
    vmax: f64,
}

impl MapRamp {
    fn at(&self, v: f64) -> Color32 {
        (self.colors)(((v - self.vmin) / (self.vmax - self.vmin).max(f64::EPSILON)) as f32)
    }
}

/// The track map's header: title, and the color-by picker. Returns a pick.
fn map_header(ui: &mut Ui, session: &Session, coloring: &MapColoring) -> Option<MapColorPick> {
    let mut pick = None;
    let (gain, color_ch) = (&coloring.gain, coloring.channel);
    ui.horizontal(|ui| {
        ui.add(Icon::MapPin.image(DIM, 16.0));
        ui.label(RichText::new("Track").font(medium(14.5)).color(TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let name = if gain.is_some() {
                "Time compare"
            } else {
                color_ch
                    .map(|i| session.channels[i].name.as_str())
                    .unwrap_or("—")
            };
            egui::ComboBox::from_id_salt("map_channel")
                .selected_text(RichText::new(name).size(13.0))
                .width(118.0)
                .show_ui(ui, |ui| {
                    if coloring.reference.is_some()
                        && ui
                            .selectable_label(gain.is_some(), "Time compare")
                            .on_hover_text(
                                "Where this lap gains and loses time against the best lap",
                            )
                            .clicked()
                    {
                        pick = Some(MapColorPick::TimeCompare);
                    }
                    // Only what's worth seeing on the track: engine and movement.
                    let imu = session.info.imu_mount.is_some();
                    for (i, c) in session.channels.iter().enumerate() {
                        if !is_dead(c)
                            && matches!(channel_group(c, imu), "Engine" | "Movement")
                            && ui
                                .selectable_label(gain.is_none() && color_ch == Some(i), &c.name)
                                .clicked()
                        {
                            pick = Some(MapColorPick::Channel(i));
                        }
                    }
                })
                .response
                .on_hover_text("Channel that colors the track");
        });
    });
    pick
}

/// GPS points `lo..hi` on screen, fitted into `rect` below a strip at the
/// top for the cursor readout.
fn map_points(gps: &aim_xrk::GpsTrack, lo: usize, hi: usize, rect: Rect) -> Vec<egui::Pos2> {
    // Local flat projection: meters east/north of the window's first point.
    let (lat0, lon0) = (gps.latitude[lo], gps.longitude[lo]);
    let xy: Vec<(f64, f64)> = (lo..hi)
        .map(|i| aim_xrk::geo::to_local(lat0, lon0, gps.latitude[i], gps.longitude[i]))
        .collect();
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(x, y) in &xy {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    // Leave a strip at the top for the cursor readout.
    let area = Rect::from_min_max(rect.min + vec2(0.0, 26.0), rect.max).shrink(12.0);
    let scale =
        (area.width() as f64 / (x1 - x0).max(1.0)).min(area.height() as f64 / (y1 - y0).max(1.0));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let to_screen = |(x, y): (f64, f64)| {
        pos2(
            area.center().x + ((x - cx) * scale) as f32,
            area.center().y - ((y - cy) * scale) as f32,
        )
    };
    xy.iter().map(|&p| to_screen(p)).collect()
}

/// Draws the track's line through `pts`, colored by the channel or by time
/// compare. Returns the channel's color ramp, for the legend.
fn paint_map_line(
    painter: &egui::Painter,
    gps: &aim_xrk::GpsTrack,
    lo: usize,
    hi: usize,
    pts: &[egui::Pos2],
    channel: Option<&Channel>,
    coloring: &MapColoring,
) -> MapRamp {
    // Color scale from the window's values, ignoring the extreme 2% each side
    // (the logger records occasional sensor spikes).
    let values: Vec<Option<f64>> = (lo..hi)
        .map(|i| channel.and_then(|c| sample_at(c, gps.times_ms[i])))
        .collect();
    let (vmin, vmax) = robust_range(values.iter().flatten().copied());
    let ramp = MapRamp {
        colors: channel.map_or(heat_color as fn(f32) -> Color32, channel_ramp),
        vmin,
        vmax,
    };
    for k in 0..pts.len() - 1 {
        let (i, j) = (lo + k, lo + k + 1);
        // Don't bridge GPS dropouts.
        if gps.times_ms[j] - gps.times_ms[i] > 1000 {
            continue;
        }
        let color = match &coloring.gain {
            Some((rates, scale)) => {
                let t = gps.times_ms[i] as f64;
                let r = rates[rates.partition_point(|p| p.0 < t).min(rates.len() - 1)].1;
                gain_color((r / scale) as f32)
            }
            None => values[k].map(|v| ramp.at(v)).unwrap_or(DIM),
        };
        painter.line_segment([pts[k], pts[k + 1]], Stroke::new(6.0, color));
    }
    ramp
}

/// The readout at the cursor, logger time `t`: the gap to the best lap or
/// the channel's value, then the speed.
fn map_readout(
    gps: &aim_xrk::GpsTrack,
    t: i64,
    coloring: &MapColoring,
    channel: Option<&Channel>,
    units: &str,
    dec: u8,
) -> String {
    let mut line = String::new();
    if let Some(c) = &coloring.gap_curve {
        let g = c[c.partition_point(|p| p.0 < t as f64).min(c.len() - 1)].2;
        line = if g >= 0.0 {
            format!("+{g:.3} s")
        } else {
            format!("−{:.3} s", -g)
        };
    } else if let Some(v) = channel.and_then(|c| sample_at(c, t)) {
        line = format!("{} {units}", format_value(v, dec));
    }
    if let Some(i) = gps.index_at(t) {
        if !line.is_empty() {
            line.push_str("   ");
        }
        line.push_str(&format!(
            "{:.1} {}",
            crate::units::speed(gps.speed_kmh[i]),
            crate::units::speed_unit()
        ));
    }
    line
}

/// The time compare legend: gaining to losing time against the best lap.
fn gain_legend(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Gaining").size(12.0).color(DIM));
        let (bar, _) = ui.allocate_exact_size(vec2(140.0, 6.0), Sense::hover());
        for j in 0..40 {
            let r = Rect::from_min_size(
                pos2(bar.left() + bar.width() * j as f32 / 40.0, bar.top()),
                vec2(bar.width() / 40.0 + 0.5, bar.height()),
            );
            ui.painter()
                .rect_filled(r, 0.0, gain_color(-1.0 + 2.0 * j as f32 / 39.0));
        }
        ui.label(
            RichText::new("Losing time  ·  vs best lap")
                .size(12.0)
                .color(DIM),
        );
    });
}

/// A channel's legend: its color ramp, between the values it covers.
fn ramp_legend(ui: &mut Ui, ramp: &MapRamp, dec: u8, units: &str) {
    let (vmin, vmax) = (ramp.vmin, ramp.vmax);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format_value(vmin, dec)).size(12.0).color(DIM));
        let bar_w = (ui.available_width() - 110.0).clamp(60.0, 200.0);
        let (bar, _) = ui.allocate_exact_size(vec2(bar_w, 6.0), Sense::hover());
        let steps = 32;
        for s in 0..steps {
            let r = Rect::from_min_size(
                pos2(
                    bar.left() + bar.width() * s as f32 / steps as f32,
                    bar.top(),
                ),
                vec2(bar.width() / steps as f32 + 0.5, bar.height()),
            );
            ui.painter().rect_filled(
                r,
                0.0,
                ramp.at(vmin + (vmax - vmin) * s as f64 / (steps - 1) as f64),
            );
        }
        ui.label(
            RichText::new(format!("{} {units}", format_value(vmax, dec)))
                .size(12.0)
                .color(DIM),
        );
    });
}

/// Range of `values` without the extreme 2% at each end.
pub(super) fn robust_range(values: impl Iterator<Item = f64>) -> (f64, f64) {
    let mut v: Vec<f64> = values.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return (0.0, 1.0);
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    let (lo, hi) = (at(0.02), at(0.98));
    if hi > lo {
        (lo, hi)
    } else {
        (v[0], v[v.len() - 1].max(v[0] + 1.0))
    }
}

/// Linear blend from `a` (t = 0) to `b` (t = 1).
pub(super) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}
