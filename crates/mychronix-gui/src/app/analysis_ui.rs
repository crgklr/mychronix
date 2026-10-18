//! The app's icon rail, and the Analysis page's frame: a one-line session
//! bar, the view tabs, the lap strip and the channel picker.

use super::*;

/// Width of the icon rail, and of the rail once it slides out on hover.
pub(super) const RAIL_W: f32 = 64.0;
const RAIL_OPEN_W: f32 = 236.0;

const PAGES: [(Page, Icon, &str); 6] = [
    (Page::Garage, Icon::House, "Garage"),
    (Page::Sessions, Icon::List, "Sessions"),
    (Page::Events, Icon::Calendar, "Events"),
    (Page::Analysis, Icon::ChartLine, "Analysis"),
    (Page::Tracks, Icon::MapPin, "Tracks"),
    (Page::Device, Icon::Mychron, "Device"),
];

impl App {
    /// Icons down the left edge; hovering slides the full rail, with labels,
    /// out over the page (the page itself doesn't move).
    pub(super) fn rail(&mut self, ctx: &egui::Context, space: Rect) {
        let id = egui::Id::new("rail_open");
        // Opens on hover with a mouse; with touch it stays narrow, so a tap
        // goes straight to the page instead of sliding the rail under the finger.
        let hovering = !self.touch_mode && ctx.pointer_hover_pos().is_some_and(|p| self.rail_rect.is_some_and(|r| r.expand(2.0).contains(p)));
        let t = ctx.animate_bool_with_time(id, hovering, 0.12);
        let width = RAIL_W + (RAIL_OPEN_W - RAIL_W) * t;
        let rect = Rect::from_min_size(space.min, vec2(width, space.height()));
        self.rail_rect = Some(rect);
        egui::Area::new(egui::Id::new("rail"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let shadow = if t > 0.0 { egui::Shadow { offset: [6, 0], blur: 24, spread: 0, color: Color32::from_black_alpha((110.0 * t) as u8) } } else { egui::Shadow::NONE };
                Frame::new().fill(SIDEBAR).shadow(shadow).show(ui, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());
                    let painter = ui.painter_at(rect);
                    // Logo: the MyChron icon, then the wordmark as the rail opens.
                    let icon = Rect::from_min_size(rect.min + vec2((RAIL_W - 38.0) / 2.0, 18.0), Vec2::splat(38.0));
                    egui::Image::new(egui::include_image!("../../assets/mychronix-icon.svg")).paint_at(ui, icon);
                    // The logo goes home, to the Garage.
                    let logo = ui.interact(icon.expand(4.0), egui::Id::new("rail_logo"), Sense::click()).on_hover_text("Garage");
                    if logo.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if logo.clicked() {
                        self.page = Page::Garage;
                    }
                    if t > 0.05 {
                        let mark = Rect::from_min_size(pos2(icon.right() + 10.0, icon.center().y - 11.0), vec2(22.0 * 578.0 / 82.0, 22.0));
                        egui::Image::new(egui::include_image!("../../assets/mychronix.svg")).tint(Color32::from_white_alpha((255.0 * t) as u8)).paint_at(ui, mark.intersect(rect));
                    }
                    let mut y = icon.bottom() + 24.0;
                    for (page, icon, label) in PAGES {
                        let row = Rect::from_min_size(pos2(rect.left() + 10.0, y), vec2(width - 20.0, 42.0));
                        let resp = ui.interact(row, egui::Id::new(("rail_item", label)), Sense::click());
                        let selected = self.page == page;
                        if selected || resp.hovered() {
                            painter.rect_filled(row, CornerRadius::same(theme::RADIUS_SMALL + 2), if selected { SURFACE_ACTIVE } else { SURFACE_HOVER });
                        }
                        let tint = if selected { TEXT } else { SECONDARY };
                        let icon_rect = Rect::from_center_size(pos2(rect.left() + RAIL_W / 2.0, row.center().y), Vec2::splat(20.0));
                        icon.image(tint, 20.0).paint_at(ui, icon_rect);
                        if t > 0.05 {
                            let color = tint.gamma_multiply(t);
                            painter.text(pos2(icon_rect.right() + 16.0, row.center().y), egui::Align2::LEFT_CENTER, label, medium(14.5), color);
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            self.page = page;
                        }
                        y += 46.0;
                    }
                });
            });
    }

    /// Track, date, driver and kart on one line with the best lap; the setup
    /// on a second. The full tiles and setup open from "Details".
    pub(super) fn session_bar(&mut self, ui: &mut Ui, session: &Session) {
        let i = &session.info;
        // The library's track (a chosen one wins over the logger's name).
        let chosen = self.path.clone().and_then(|p| self.session_track_name(&p));
        let title = chosen.or_else(|| i.venue.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Session".into());
        let when = match (i.date.as_deref(), i.time.as_deref()) {
            (Some(d), Some(t)) => Some(format!("{d} at {}", crate::events::us_time(t))),
            (Some(d), None) => Some(d.to_string()),
            _ => None,
        };
        let sub = [i.session.clone(), when, i.driver.clone(), i.vehicle.clone()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("  ·  ");
        ui.horizontal(|ui| {
            let t = ui.add(egui::Label::new(RichText::new(title).font(semibold(24.0)).color(TEXT)).sense(Sense::click()));
            if t.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if t.on_hover_text("Change the track this session was at").clicked() {
                if let Some(p) = self.path.clone() {
                    self.open_track_pick(p);
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new(sub).font(regular(14.5)).color(SECONDARY));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if button(ui, Some(Icon::FolderOpen), "Open…", false).on_hover_text("Open a session file").clicked() {
                    self.open_dialog();
                }
                let label = if self.session_details { "Hide details" } else { "Details" };
                if chip(ui, label, self.session_details).clicked() {
                    self.session_details = !self.session_details;
                }
                if let Some(best) = session.best_lap(MIN_LAP_MS) {
                    ui.label(RichText::new(format_lap_time(best.duration_ms())).font(semibold(17.0)).color(theme::GOOD));
                    ui.label(RichText::new("Best").size(12.5).color(DIM));
                }
                // Sensors that stopped recording in this session.
                if let Some(checked) = self.open_session_check(session) {
                    let names: Vec<&str> = checked.issues.iter().map(|i| i.channel.as_str()).collect();
                    let text = format!("{}: no data", names.join(", "));
                    let r = ui.add(egui::Button::image_and_text(Icon::TriangleAlert.image(theme::FAVORITE, 14.0), RichText::new(text).size(13.0).color(theme::FAVORITE)).frame(false));
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if r.on_hover_text("What happened, and what to check").clicked() {
                        self.sensors.report = Some(vec![checked]);
                    }
                }
            });
        });
        if self.session_details {
            ui.add_space(12.0);
            summary_tiles(ui, session);
            ui.add_space(10.0);
            self.setup_strip(ui);
        } else {
            ui.add_space(2.0);
            self.setup_line(ui);
        }
        // Session notes: what stands out across the racing laps.
        let notes = self.open_session_notes(session);
        if !notes.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add(Icon::Activity.image(DIM, 14.0));
                for (k, (text, why, attention)) in notes.into_iter().enumerate() {
                    if k > 0 {
                        ui.label(RichText::new("·").size(13.0).color(DIM));
                    }
                    ui.label(RichText::new(text).size(13.0).color(if attention { theme::FAVORITE } else { SECONDARY })).on_hover_text(why);
                }
            });
        }
    }

    /// The analysis views as tabs.
    pub(super) fn view_tabs(&mut self, ui: &mut Ui, session: &Session) {
        // Views that can't work for this session are grayed out, with why.
        let gps = session.gps.as_ref().is_some_and(|g| g.len() > 10);
        let distance = gps || session.channel("GPS Speed").is_some() || session.channel(crate::setup::SPEED_FROM_RPM).is_some();
        let unavailable = |view: AnalysisView| -> Option<&'static str> {
            match view {
                AnalysisView::Splits if !gps => Some("Needs GPS: splits are measured along the track"),
                AnalysisView::Corners if !gps => Some("Needs GPS: corners are found from the kart's position"),
                AnalysisView::Compare if !distance => {
                    Some("Needs GPS, or RPM with the kart's gearing and tire size (set them in the setup), to line laps up by distance")
                }
                _ => None,
            }
        };
        if unavailable(self.analysis_view).is_some() {
            self.analysis_view = AnalysisView::Session;
        }
        let compare = if self.compare.is_empty() { "Compare".to_string() } else { format!("Compare ({})", self.compare.len()) };
        let tabs = [
            (AnalysisView::Session, Icon::ChartLine, "Graphs".to_string(), "Channels through a lap, with the track map"),
            (AnalysisView::Report, Icon::List, "Lap by lap".to_string(), "Each lap's numbers side by side"),
            (AnalysisView::Splits, Icon::Flag, "Splits".to_string(), "Each lap's time through each part of the track"),
            (AnalysisView::Corners, Icon::Timer, "Corners".to_string(), "Braking in each corner, against the best lap"),
            (AnalysisView::Histogram, Icon::Gauge, "Histogram".to_string(), "How much time a channel spends in each band, e.g. RPM"),
            (AnalysisView::Scatter, Icon::Activity, "Scatter".to_string(), "One channel against another: RPM vs speed, the G-G diagram"),
            (AnalysisView::Compare, Icon::Activity, compare, "Laps from any sessions overlaid by distance"),
        ];
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 0.0), Sense::hover());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (view, icon, label, hint) in tabs {
                let selected = self.analysis_view == view;
                let off = unavailable(view);
                let galley = ui.painter().layout_no_wrap(label.clone(), medium(14.5), TEXT);
                let size = vec2(galley.size().x + 50.0, 40.0);
                let (rect, resp) = ui.allocate_exact_size(size, if off.is_some() { Sense::hover() } else { Sense::click() });
                let tint = if off.is_some() { DIM.gamma_multiply(0.6) } else if selected { TEXT } else if resp.hovered() { TEXT.gamma_multiply(0.85) } else { SECONDARY };
                if resp.hovered() && !selected && off.is_none() {
                    ui.painter().rect_filled(rect.shrink2(vec2(0.0, 4.0)), CornerRadius::same(theme::RADIUS_SMALL), SURFACE_HOVER);
                }
                icon.image(tint, 16.0).paint_at(ui, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(16.0)));
                ui.painter().text(pos2(rect.left() + 36.0, rect.center().y), egui::Align2::LEFT_CENTER, label, medium(14.5), tint);
                if selected {
                    let bar = Rect::from_min_max(pos2(rect.left() + 8.0, rect.bottom() - 2.5), pos2(rect.right() - 8.0, rect.bottom()));
                    ui.painter().rect_filled(bar, CornerRadius::same(2), TEXT);
                }
                if let Some(why) = off {
                    resp.on_hover_text(why);
                    continue;
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.on_hover_text(hint).clicked() {
                    self.analysis_view = view;
                }
            }
        });
        // A hairline under the tabs.
        let y = ui.min_rect().bottom();
        ui.painter().hline(line.left()..=line.right(), y, Stroke::new(1.0, theme::OUTLINE));
    }

    /// Every lap as a block, sized by its time. On Graphs a click views the
    /// lap and the small circle adds it to Compare; on Compare a click adds or
    /// removes it. Out, in and slow laps are dimmed.
    pub(super) fn lap_strip(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>, compare_mode: bool) {
        let laps = &session.laps;
        if laps.is_empty() {
            return;
        }
        let slow = slow_laps(session);
        let best = session.best_lap(MIN_LAP_MS).copied();
        let mut sorted: Vec<i64> = laps.iter().map(|l| l.duration_ms()).collect();
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2].max(1) as f32;
        // Racing laps share one width; long laps (stopped under a flag) are wider, capped so they don't swamp the strip.
        let weight = |l: &aim_xrk::Lap| (l.duration_ms() as f32 / median).clamp(1.0, 1.8);
        let all_w = if compare_mode { 86.0 } else { 64.0 };
        let gap = 4.0;
        let avail = ui.available_width() - all_w - gap * laps.len() as f32;
        let total: f32 = laps.iter().map(weight).sum();
        let h = 46.0;
        let mut toggle = None;
        let mut pick: Option<Option<usize>> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            if compare_mode {
                // Which session these laps come from, and what a click does.
                let (rect, _) = ui.allocate_exact_size(vec2(all_w, h), Sense::hover());
                let number = self.path.as_ref().and_then(|p| p.file_stem()).and_then(|n| aim_xrk::summary::session_number(&n.to_string_lossy()));
                let p = ui.painter();
                p.text(pos2(rect.left() + 4.0, rect.top() + 13.0), egui::Align2::LEFT_CENTER, "Add laps", regular(11.5), DIM);
                p.text(pos2(rect.left() + 4.0, rect.top() + 31.0), egui::Align2::LEFT_CENTER, number.map(|n| format!("Session {n}")).unwrap_or_default(), medium(12.5), SECONDARY);
            } else {
                // The whole session.
                let (rect, resp) = ui.allocate_exact_size(vec2(all_w, h), Sense::click());
                let on = self.lap.is_none();
                let p = ui.painter();
                p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL), if on { SURFACE_ACTIVE } else if resp.hovered() { SURFACE_HOVER } else { SURFACE });
                if on {
                    p.rect_stroke(rect, CornerRadius::same(theme::RADIUS_SMALL), Stroke::new(1.5, TEXT), egui::StrokeKind::Inside);
                }
                p.text(rect.center(), egui::Align2::CENTER_CENTER, "All", medium(13.5), if on { TEXT } else { SECONDARY });
                if resp.on_hover_text("The whole session").clicked() {
                    pick = Some(None);
                }
            }
            for (i, lap) in laps.iter().enumerate() {
                let w = (avail * weight(lap) / total).max(28.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
                // On Compare, "selected" means in the comparison.
                let selected = if compare_mode { false } else { self.lap == Some(i) };
                let is_best = Some(*lap) == best;
                let compare_slot = self.compare.iter().find(|c| c.lap == *lap && Some(&c.path) == self.path.as_ref()).map(|c| c.slot);
                let p = ui.painter();
                let fill = if selected {
                    SURFACE_ACTIVE
                } else if resp.hovered() {
                    SURFACE_HOVER
                } else if slow[i] {
                    WINDOW
                } else {
                    SURFACE
                };
                p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL), fill);
                if slow[i] && !selected {
                    p.rect_stroke(rect, CornerRadius::same(theme::RADIUS_SMALL), Stroke::new(1.0, theme::OUTLINE), egui::StrokeKind::Inside);
                }
                if selected {
                    p.rect_stroke(rect, CornerRadius::same(theme::RADIUS_SMALL), Stroke::new(1.5, TEXT), egui::StrokeKind::Inside);
                }
                if let (true, Some(slot)) = (compare_mode, compare_slot) {
                    p.rect_stroke(rect, CornerRadius::same(theme::RADIUS_SMALL), Stroke::new(2.0, SERIES[slot]), egui::StrokeKind::Inside);
                }
                let time_color = if is_best { theme::GOOD } else if slow[i] { DIM } else { TEXT };
                if w >= 62.0 {
                    p.text(pos2(rect.left() + 8.0, rect.top() + 13.0), egui::Align2::LEFT_CENTER, lap.number.to_string(), regular(11.5), DIM);
                    p.text(pos2(rect.left() + 8.0, rect.top() + 31.0), egui::Align2::LEFT_CENTER, format_lap_time(lap.duration_ms()), medium(13.0), time_color);
                } else {
                    p.text(rect.center(), egui::Align2::CENTER_CENTER, lap.number.to_string(), medium(12.5), time_color);
                }
                // Compare: a circle in the corner, filled with its color once added.
                let dot = Rect::from_center_size(pos2(rect.right() - 10.0, rect.top() + 10.0), Vec2::splat(14.0));
                let over_dot = resp.hover_pos().is_some_and(|q| dot.expand(3.0).contains(q));
                match compare_slot {
                    Some(slot) => {
                        p.circle_filled(dot.center(), 5.0, SERIES[slot]);
                    }
                    None if resp.hovered() && w >= 40.0 && !compare_mode => {
                        p.circle_stroke(dot.center(), 6.0, Stroke::new(1.2, if over_dot { TEXT } else { DIM }));
                        p.text(dot.center() - vec2(0.0, 0.5), egui::Align2::CENTER_CENTER, "+", medium(11.0), if over_dot { TEXT } else { DIM });
                    }
                    None => {}
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let mut tip = format!("Lap {}  ·  {}", lap.number, format_lap_time(lap.duration_ms()));
                if is_best {
                    tip.push_str("  ·  best");
                } else if let (Some(b), false) = (best, slow[i]) {
                    tip.push_str(&format!("  ·  {} on the best", format_delta(lap.duration_ms() - b.duration_ms())));
                } else if slow[i] {
                    tip.push_str("  ·  out, in or slow lap");
                }
                tip.push_str(match (compare_mode, compare_slot.is_some()) {
                    (true, true) => "\nClick to remove from the comparison",
                    (true, false) => "\nClick to add to the comparison",
                    (false, true) => "\nClick to view  ·  the dot: remove from Compare",
                    (false, false) => "\nClick to view  ·  + to add to Compare",
                });
                let clicked = resp.on_hover_text(tip).clicked();
                if clicked && (compare_mode || over_dot) {
                    toggle = Some(*lap);
                } else if clicked {
                    pick = Some(Some(i));
                }
            }
        });
        if let Some(choice) = pick {
            self.lap = choice;
            self.cursor_s = None;
        }
        if let Some(lap) = toggle {
            self.toggle_compare(session, lap);
        }
    }

    /// "+ Add channel": the grouped channel list in a panel over the page.
    pub(super) fn channel_picker(&mut self, ui: &mut Ui, session: &Session, start_ms: i64) {
        let resp = ui.horizontal(|ui| {
            let r = button(ui, None, "+ Add channel", false);
            let inputs = driver_inputs(session);
            if inputs.len() > 1
                && button(ui, Some(Icon::Activity), "Driver inputs", false)
                    .on_hover_text("Graph steering, speed and braking/acceleration (and throttle and brake pressure when recorded)")
                    .clicked()
            {
                self.selected.clear();
                self.graph_order.clear();
                for i in inputs {
                    self.select(i);
                }
            }
            let n = self.selected.len();
            ui.label(RichText::new(format!("{n} graphed  ·  ✕ on a graph removes it")).size(12.5).color(DIM));
            // Time or distance along the bottom axis (distance needs GPS).
            if session.gps.is_some() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if chip(ui, "Distance", self.graph_distance).on_hover_text("Meters around the lap, so corners line up").clicked() {
                        self.graph_distance = true;
                    }
                    if chip(ui, "Time", !self.graph_distance).clicked() {
                        self.graph_distance = false;
                    }
                    ui.label(RichText::new("Axis").size(12.5).color(DIM));
                });
            }
            r
        });
        let button_rect = resp.inner.rect;
        if resp.inner.clicked() {
            self.channel_picker_open = !self.channel_picker_open;
        }
        if !self.channel_picker_open {
            return;
        }
        let size = vec2(340.0, 460.0_f32.min(button_rect.top() - 40.0).max(200.0));
        let area = egui::Area::new(egui::Id::new("channel_picker"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(button_rect.left(), button_rect.top() - size.y - 8.0))
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, theme::OUTLINE))
                    .corner_radius(CornerRadius::same(theme::RADIUS))
                    .shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(140) })
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(size.x - 28.0);
                        ui.set_height(size.y - 28.0);
                        self.channel_list(ui, session, start_ms);
                    });
            });
        // Click anywhere else to close it.
        let clicked_outside = ui.input(|i| i.pointer.any_click())
            && ui.ctx().pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !button_rect.contains(p));
        if clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.channel_picker_open = false;
        }
    }
}

/// The channels that show what the driver is doing, in graph order: steering,
/// throttle, brake, then speed and longitudinal g. Only those recorded with data.
pub(super) fn driver_inputs(session: &Session) -> Vec<usize> {
    let find = |pred: &dyn Fn(&str) -> bool| session.channels.iter().position(|c| !is_dead(c) && pred(&c.name.to_lowercase()));
    [
        find(&|n| n.contains("steer")),
        find(&|n| n.contains("throttle") || n == "tps" || n.ends_with(" tps") || n.contains("pedal")),
        find(&|n| n.contains("brake") || n.contains("brk")),
        find(&|n| n == "gps speed"),
        find(&|n| n == "gps lonacc"),
    ]
    .into_iter()
    .flatten()
    .collect()
}
