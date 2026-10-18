//! Lap comparison: laps lined up by distance, the time gap between them,
//! and where on the track each one gains or loses.

use super::*;

impl App {
    /// Adds a lap of `session` to the comparison, or removes it if it's there.
    pub(super) fn toggle_compare(&mut self, session: &std::sync::Arc<Session>, lap: aim_xrk::Lap) {
        let Some(path) = self.path.clone() else { return };
        if let Some(i) = self.compare.iter().position(|c| c.lap == lap && c.path == path) {
            self.compare.remove(i);
            return;
        }
        self.add_compare_lap(path, session.clone(), lap);
    }

    pub(super) fn add_compare_lap(&mut self, path: PathBuf, session: std::sync::Arc<Session>, lap: aim_xrk::Lap) {
        if self.compare.len() >= SERIES.len() || self.compare.iter().any(|c| c.lap == lap && c.path == path) {
            return;
        }
        let used: Vec<usize> = self.compare.iter().map(|c| c.slot).collect();
        let slot = (0..SERIES.len()).find(|s| !used.contains(s)).unwrap_or(0);
        let number = path.file_stem().and_then(|n| aim_xrk::summary::session_number(&n.to_string_lossy()));
        let driver = session.info.driver.clone().filter(|d| !d.is_empty());
        let label = [
            Some(format!("S{} L{}", number.map(|n| n.to_string()).unwrap_or_else(|| "?".into()), lap.number)),
            Some(format_lap_time(lap.duration_ms())),
            driver,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ·  ");
        self.compare.push(crate::compare::CompareLap::new(path, session, lap, label, slot));
    }

    /// The best lap of each downloaded session at the current session's track, fastest first.
    pub(super) fn add_best_laps_here(&mut self, current: Option<std::sync::Arc<Session>>) {
        let lib = self.library.take().unwrap_or_else(Library::load);
        let here = self.path.as_ref().and_then(|p| lib.sessions.iter().find(|s| &s.path == p)).and_then(|s| s.track);
        if let Some(track) = here {
            let mut sessions: Vec<&tracks::LocalSession> =
                lib.sessions.iter().filter(|s| s.track == Some(track) && s.summary.best_lap_ms.is_some()).collect();
            sessions.sort_by_key(|s| s.summary.best_lap_ms);
            for s in sessions {
                if self.compare.len() >= SERIES.len() {
                    break;
                }
                // The open session is set aside while the page draws, so it's passed in.
                let session = if self.path.as_ref() == Some(&s.path) {
                    current.clone().or_else(|| self.session.clone())
                } else {
                    let path = s.path.clone();
                    crate::jobs::open_session(&path).ok().map(|mut x| {
                        self.fill_names(&mut x, &path);
                        std::sync::Arc::new(x)
                    })
                };
                let Some(session) = session else { continue };
                if let Some(best) = session.best_lap(MIN_LAP_MS).copied() {
                    self.add_compare_lap(s.path.clone(), session, best);
                }
            }
        }
        self.library = Some(lib);
    }

    pub(super) fn compare_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        self.compare_mode_toggle(ui);
        if self.compare_mode == compare_drivers_ui::CompareMode::Drivers {
            self.compare_drivers_bar(ui, session);
        } else {
            // This session's laps, to add or remove with a click.
            self.lap_strip(ui, session, true);
            ui.add_space(12.0);
        }
        // Lap chips: color, label, reference marker; click to make reference, × to remove.
        let mut make_ref = None;
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (k, c) in self.compare.iter().enumerate() {
                Frame::new()
                    .fill(if k == 0 { SURFACE_ACTIVE } else { SURFACE })
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(Margin { left: 12, right: 8, top: 6, bottom: 6 })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (sw, _) = ui.allocate_exact_size(vec2(16.0, 12.0), Sense::hover());
                            ui.painter().line_segment([sw.left_center(), sw.right_center()], Stroke::new(3.0, SERIES[c.slot]));
                            let r = ui.add(egui::Label::new(RichText::new(&c.label).font(medium(13.5)).color(TEXT)).sense(Sense::click()));
                            if k == 0 {
                                ui.label(RichText::new("Reference").size(12.0).color(DIM));
                            } else if r.on_hover_text("Make this the reference").clicked() {
                                make_ref = Some(k);
                            }
                            if ui.add(Icon::X.image(DIM, 12.0).sense(Sense::click())).on_hover_text("Remove").clicked() {
                                remove = Some(k);
                            }
                        });
                    });
            }
            if self.compare_mode == compare_drivers_ui::CompareMode::Laps {
                if button(ui, Some(Icon::Flag), "Add best laps at this track", false).clicked() {
                    self.add_best_laps_here(Some(session.clone()));
                }
                if !self.compare.is_empty() && button(ui, None, "Clear", false).clicked() {
                    self.compare.clear();
                }
            }
        });
        if let Some(k) = make_ref {
            let lap = self.compare.remove(k);
            self.compare.insert(0, lap);
        }
        if let Some(k) = remove {
            self.compare.remove(k);
        }
        ui.add_space(12.0);
        self.compare_calculated_notes(ui);

        if self.compare.len() < 2 {
            card(ui, |ui| {
                ui.set_min_height(160.0);
                ui.label(RichText::new("Pick laps to compare").font(semibold(17.0)).color(TEXT));
                ui.label(
                    RichText::new("Click laps in the strip above to add them; open another session (Sessions page) to add its laps too. Or add your best laps at this track. The first lap is the reference the others are measured against.")
                        .color(SECONDARY),
                );
            });
            return;
        }
        if self.compare.iter().any(|c| c.profile.is_none()) {
            card(ui, |ui| {
                ui.label(RichText::new("Some of these laps have no GPS, so they can't be lined up by distance yet.").color(SECONDARY));
            });
            return;
        }

        let reference = self.compare[0].profile.clone().unwrap();
        let ref_len = reference.length();
        // A new reference lap starts the graphs at its full length.
        let reset = self.compare_window != Some(ref_len.to_bits());
        self.compare_window = Some(ref_len.to_bits());
        let channels: Vec<String> = self.selected.keys().filter_map(|&i| session.channels.get(i)).map(|c| c.name.clone()).collect();
        let gap = 12.0;
        let height = ui.available_height();
        let map_w = (ui.available_width() * 0.36).clamp(280.0, 520.0);
        let graphs_w = ui.available_width() - map_w - gap;
        let mut hovered = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(graphs_w, height), column, |ui| {
                ui.set_width(graphs_w);
                let n = (channels.len() + 1) as f32;
                let card_h = ((ui.available_height() - 10.0 * (n - 1.0)) / n).max(150.0);
                let link = ui.id().with("compare_x");
                egui::ScrollArea::vertical().id_salt("compare_graphs").show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    // Time gap to the reference.
                    let gaps: Vec<(usize, Vec<[f64; 2]>)> = self.compare[1..]
                        .iter()
                        .map(|c| (c.slot, crate::compare::time_gap(c.profile.as_ref().unwrap(), &reference, 600)))
                        .collect();
                    if let Some(h) = self.compare_graph(ui, "Time gap to reference", "s", card_h, link, false, &gaps, ref_len, true, reset) {
                        hovered = Some(h);
                    }
                    for (k, name) in channels.iter().enumerate() {
                        let lines: Vec<(usize, Vec<[f64; 2]>)> = self
                            .compare
                            .iter()
                            .filter_map(|c| {
                                same_channel(&c.session, name).map(|ch| (c.slot, crate::compare::channel_on_distance(c, ch, ref_len)))
                            })
                            .collect();
                        let units = session.channels.iter().find(|c| &c.name == name).map(|c| pretty_units(&c.units).to_string()).unwrap_or_default();
                        if let Some(h) = self.compare_graph(ui, name, &units, card_h, link, k + 1 == channels.len(), &lines, ref_len, false, reset) {
                            hovered = Some(h);
                        }
                    }
                });
            });
            ui.allocate_ui_with_layout(vec2(map_w, height), column, |ui| {
                ui.set_width(map_w);
                // The map needs the reference lap's GPS; without it, the lap breakdown.
                let lap = &self.compare[0];
                let has_map = lap.session.gps.as_ref().is_some_and(|g| g.times_ms.iter().filter(|&&t| t >= lap.lap.start_ms && t <= lap.lap.end_ms).nth(2).is_some());
                if has_map {
                    self.compare_map(ui, &reference);
                } else {
                    egui::ScrollArea::vertical().id_salt("compare_breakdown").show(ui, |ui| self.compare_breakdown(ui, &reference));
                }
            });
        });
        if hovered.is_some() {
            self.compare_cursor_m = hovered;
        }
    }

    /// One graph card over distance: a line per lap, readouts at the cursor in the header.
    #[allow(clippy::too_many_arguments)]
    fn compare_graph(
        &self,
        ui: &mut Ui,
        title: &str,
        units: &str,
        height: f32,
        link: egui::Id,
        x_axis: bool,
        lines: &[(usize, Vec<[f64; 2]>)],
        ref_len: f64,
        zero_line: bool,
        reset: bool,
    ) -> Option<f64> {
        let mut hovered = None;
        let cursor = self.compare_cursor_m;
        graph_card(ui, height, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).font(medium(14.5)).color(TEXT));
                if !units.is_empty() {
                    ui.label(RichText::new(units).size(13.0).color(DIM));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(x) = cursor {
                        for (slot, pts) in lines.iter().rev() {
                            let i = pts.partition_point(|p| p[0] < x).min(pts.len().saturating_sub(1));
                            if let Some(p) = pts.get(i) {
                                let text = if zero_line { format!("{:+.2}", p[1]) } else { format!("{:.0}", p[1]) };
                                ui.label(RichText::new(text).font(semibold(14.0)).color(TEXT));
                                let (sw, _) = ui.allocate_exact_size(vec2(12.0, 10.0), Sense::hover());
                                ui.painter().line_segment([sw.left_center(), sw.right_center()], Stroke::new(3.0, SERIES[*slot]));
                            }
                        }
                    }
                });
            });
            let mut plot = Plot::new(("compare", title));
            if reset {
                plot = plot.reset();
            }
            let response = plot
                .height(ui.available_height())
                .show_background(false)
                .grid_color(GRID)
                .show_axes([x_axis, true])
                .y_axis_min_width(46.0)
                .show_x(false)
                .show_y(false)
                .cursor_color(DIM)
                .link_axis(link, [true, false])
                .link_cursor(link, [true, false])
                .allow_zoom([true, false])
                .allow_drag([true, false])
                .allow_scroll(false)
                .auto_bounds([false, true])
                .default_x_bounds(0.0, ref_len.max(0.001))
                .x_axis_formatter(|mark, _| format!("{:.0} m", mark.value))
                .show(ui, |plot| {
                    if zero_line {
                        plot.hline(egui_plot::HLine::new("level", 0.0).color(DIM).width(1.0));
                    }
                    for (slot, pts) in lines {
                        plot.line(Line::new(format!("lap{slot}"), PlotPoints::from(pts.clone())).color(SERIES[*slot]).width(2.0));
                    }
                    if let Some(x) = cursor {
                        plot.vline(VLine::new("cursor", x).color(SECONDARY).width(1.0));
                    }
                    plot.pointer_coordinate()
                });
            if let Some(p) = response.inner {
                hovered = Some(p.x.clamp(0.0, ref_len));
            }
        });
        hovered
    }

    /// The reference lap's line, colored by where the second lap gains (blue)
    /// or loses (red) time against it, with the cursor.
    fn compare_map(&mut self, ui: &mut Ui, reference: &crate::compare::DistanceProfile) {
        let ref_lap = self.compare[0].clone();
        let other = self.compare[1].clone();
        let Some(gps) = ref_lap.session.gps.as_ref() else { return };
        let lo = gps.times_ms.partition_point(|&t| t < ref_lap.lap.start_ms);
        let hi = gps.times_ms.partition_point(|&t| t <= ref_lap.lap.end_ms);
        if hi < lo + 2 {
            return;
        }
        let gaps = crate::compare::time_gap(other.profile.as_ref().unwrap(), reference, 600);
        // Rate the gap changes per meter: positive means the other lap is losing time there.
        let rate_at = |d: f64| {
            let i = gaps.partition_point(|p| p[0] < d).clamp(1, gaps.len() - 1);
            let (a, b) = (gaps[i - 1], gaps[i]);
            (b[1] - a[1]) / (b[0] - a[0]).max(1e-6)
        };
        let rates: Vec<f64> = (lo..hi).map(|i| rate_at(reference.dist_at(gps.times_ms[i] as f64))).collect();
        let scale = rates.iter().map(|r| r.abs()).fold(0.0_f64, f64::max).max(1e-6) * 0.6;

        Frame::new()
            .fill(SURFACE)
            .corner_radius(CornerRadius::same(theme::RADIUS))
            .inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 12 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height() - 24.0);
                ui.horizontal(|ui| {
                    ui.add(Icon::MapPin.image(DIM, 16.0));
                    ui.label(RichText::new("Where time changes").font(medium(14.5)).color(TEXT));
                });
                ui.label(RichText::new(format!("{} against the reference", other.label)).size(12.5).color(SECONDARY));
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), (ui.available_height() - 40.0).max(120.0)), Sense::hover());
                let (lat0, lon0) = (gps.latitude[lo], gps.longitude[lo]);
                let kx = 111_320.0 * lat0.to_radians().cos();
                let xy: Vec<(f64, f64)> = (lo..hi).map(|i| ((gps.longitude[i] - lon0) * kx, (gps.latitude[i] - lat0) * 110_540.0)).collect();
                let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                for &(x, y) in &xy {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
                let area = rect.shrink(12.0);
                let s = (area.width() as f64 / (x1 - x0).max(1.0)).min(area.height() as f64 / (y1 - y0).max(1.0));
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let pts: Vec<egui::Pos2> =
                    xy.iter().map(|&(x, y)| pos2(area.center().x + ((x - cx) * s) as f32, area.center().y - ((y - cy) * s) as f32)).collect();
                let painter = ui.painter_at(rect);
                for k in 0..pts.len() - 1 {
                    painter.line_segment([pts[k], pts[k + 1]], Stroke::new(4.0, gain_color((rates[k] / scale) as f32)));
                }
                // Hover the map to move the cursor.
                if let Some(pointer) = response.hover_pos() {
                    let (k, d) = pts.iter().enumerate().map(|(k, p)| (k, p.distance_sq(pointer))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                    if d < 28.0 * 28.0 {
                        self.compare_cursor_m = Some(reference.dist_at(gps.times_ms[lo + k] as f64));
                    }
                }
                if let Some(m) = self.compare_cursor_m {
                    let t = reference.time_at(m);
                    let k = gps.times_ms[lo..hi].partition_point(|&g| (g as f64) < t).min(pts.len() - 1);
                    painter.circle_filled(pts[k], 7.0, SURFACE);
                    painter.circle_filled(pts[k], 5.0, TEXT);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Gaining").size(12.0).color(DIM));
                    let (bar, _) = ui.allocate_exact_size(vec2(140.0, 8.0), Sense::hover());
                    for j in 0..40 {
                        let r = Rect::from_min_size(pos2(bar.left() + bar.width() * j as f32 / 40.0, bar.top()), vec2(bar.width() / 40.0 + 0.5, bar.height()));
                        ui.painter().rect_filled(r, 0.0, gain_color(-1.0 + 2.0 * j as f32 / 39.0));
                    }
                    ui.label(RichText::new("Losing time").size(12.0).color(DIM));
                });
            });
    }
}

/// Diverging color for time gained (−1, blue) through even (0, gray) to lost (+1, red).
pub(super) fn gain_color(t: f32) -> Color32 {
    const GAIN: Color32 = Color32::from_rgb(0x39, 0x87, 0xe5);
    const EVEN: Color32 = Color32::from_rgb(0x58, 0x5e, 0x66);
    const LOSE: Color32 = Color32::from_rgb(0xe6, 0x67, 0x67);
    let t = t.clamp(-1.0, 1.0);
    if t < 0.0 { mix(EVEN, GAIN, -t) } else { mix(EVEN, LOSE, t) }
}
