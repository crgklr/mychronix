//! The app's icon rail, and the analysis views' frame: the rail's pages and
//! analysis views with the session indicator, the "what you're looking at"
//! pill and the session panel it opens, each view's title, the lap strip and
//! the channel picker.

use super::*;

/// Width of the icon rail, and of the rail once it slides out on hover.
pub(super) const RAIL_W: f32 = 64.0;
const RAIL_OPEN_W: f32 = 236.0;
/// From one rail row to the next.
const ROW_STEP: f32 = 44.0;

const TOP_PAGES: [(Page, Icon, &str); 3] = [(Page::Garage, Icon::House, "Garage"), (Page::Sessions, Icon::List, "Sessions"), (Page::Events, Icon::Calendar, "Events")];

/// The analysis views, each its own place in the rail.
const VIEWS: [(AnalysisView, Icon, &str); 6] = [
    (AnalysisView::Report, Icon::Clock, "Laps"),
    (AnalysisView::Splits, Icon::Flag, "Splits"),
    (AnalysisView::Corners, Icon::Timer, "Corners"),
    (AnalysisView::Map, Icon::Track, "Map"),
    (AnalysisView::Session, Icon::ChartLine, "Graphs"),
    (AnalysisView::Compare, Icon::Activity, "Compare"),
];

const BOTTOM_PAGES: [(Page, Icon, &str); 2] = [(Page::Tracks, Icon::MapPin, "Tracks"), (Page::Device, Icon::Mychron, "Device")];

impl AnalysisView {
    /// The rail item it's under: Histogram and Scatter are tabs of Graphs.
    pub(super) fn rail_item(self) -> AnalysisView {
        match self {
            AnalysisView::Histogram | AnalysisView::Scatter => AnalysisView::Session,
            v => v,
        }
    }
}

/// Why a view can't work for this session, if it can't.
pub(super) fn view_unavailable(session: &Session, view: AnalysisView) -> Option<&'static str> {
    let gps = session.gps.as_ref().is_some_and(|g| g.len() > 10);
    let distance = gps || session.channel("GPS Speed").is_some() || session.channel(crate::setup::SPEED_FROM_RPM).is_some();
    match view {
        AnalysisView::Splits if !gps => Some("Needs GPS: splits are measured along the track"),
        AnalysisView::Corners if !gps => Some("Needs GPS: corners are found from the kart's position"),
        AnalysisView::Map if !gps => Some("Needs GPS: the map is the kart's line around the track"),
        AnalysisView::Compare if !distance => Some("Needs GPS, or RPM with the kart's gearing and tire size (set them in the setup), to line laps up by distance"),
        _ => None,
    }
}

/// "Sat 10/10".
fn short_day(d: aim_xrk::summary::Date) -> String {
    long_date(d).get(..3).map(|day| format!("{day} {}/{}", d.month, d.day)).unwrap_or_default()
}

/// One row of the rail: icon, and the label as it slides out. Returns its response.
#[allow(clippy::too_many_arguments)]
fn rail_row(ui: &Ui, rail: Rect, width: f32, y: f32, icon: Icon, label: &str, selected: bool, off: Option<&str>, t: f32) -> egui::Response {
    let row = Rect::from_min_size(pos2(rail.left() + 10.0, y), vec2(width - 20.0, 40.0));
    let resp = ui.interact(row, egui::Id::new(("rail_item", label)), if off.is_some() { Sense::hover() } else { Sense::click() });
    let painter = ui.painter_at(rail);
    if selected || (resp.hovered() && off.is_none()) {
        painter.rect_filled(row, CornerRadius::same(theme::RADIUS_SMALL + 2), if selected { SURFACE_ACTIVE } else { SURFACE_HOVER });
    }
    let tint = if off.is_some() { DIM.gamma_multiply(0.6) } else if selected { TEXT } else { SECONDARY };
    let icon_rect = Rect::from_center_size(pos2(rail.left() + RAIL_W / 2.0, row.center().y), Vec2::splat(20.0));
    icon.image(tint, 20.0).paint_at(ui, icon_rect);
    if t > 0.05 {
        painter.text(pos2(icon_rect.right() + 16.0, row.center().y), egui::Align2::LEFT_CENTER, label, medium(14.5), tint.gamma_multiply(t));
    }
    if resp.hovered() && off.is_none() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    match off {
        Some(why) => resp.on_hover_text(why),
        None => resp,
    }
}

impl App {
    /// Icons down the left edge; hovering slides the full rail, with labels,
    /// out over the page (the page itself doesn't move). The analysis views
    /// sit under the session indicator, which opens the session panel.
    pub(super) fn rail(&mut self, ctx: &egui::Context, space: Rect, session: Option<&Session>) {
        let id = egui::Id::new("rail_open");
        // Opens on hover with a mouse; with touch it stays narrow, so a tap
        // goes straight to the page instead of sliding the rail under the finger.
        let hovering = !self.touch_mode && ctx.pointer_hover_pos().is_some_and(|p| self.rail_rect.is_some_and(|r| r.expand(2.0).contains(p)));
        let t = ctx.animate_bool_with_time(id, hovering, 0.12);
        let width = RAIL_W + (RAIL_OPEN_W - RAIL_W) * t;
        let rect = Rect::from_min_size(space.min, vec2(width, space.height()));
        self.rail_rect = Some(rect);
        let indicator = session.map(|s| self.session_label(s));
        let compare_n = self.compare.len();
        egui::Area::new(egui::Id::new("rail"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let shadow = if t > 0.0 { egui::Shadow { offset: [6, 0], blur: 24, spread: 0, color: Color32::from_black_alpha((110.0 * t) as u8) } } else { egui::Shadow::NONE };
                Frame::new().fill(SIDEBAR).shadow(shadow).show(ui, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());
                    // Its empty space moves the window (no title bar on Linux); the items below take their own clicks.
                    let empty = ui.interact(rect, egui::Id::new("rail_move"), Sense::click_and_drag());
                    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                    super::window_ui::move_or_maximize(ui.ctx(), &empty, maximized);
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
                    let separator = |y: f32| painter.hline(rect.left() + 18.0..=rect.right() - 18.0, y, Stroke::new(1.0, theme::OUTLINE));
                    let mut y = icon.bottom() + 20.0;
                    for (page, icon, label) in TOP_PAGES {
                        if rail_row(ui, rect, width, y, icon, label, self.page == page, None, t).clicked() {
                            self.page = page;
                        }
                        y += ROW_STEP;
                    }
                    separator(y + 4.0);
                    y += 14.0;

                    // What you're looking at: a dot, and the session as the rail opens.
                    if let Some((title, detail)) = &indicator {
                        let card = Rect::from_min_size(pos2(rect.left() + 10.0, y), vec2(width - 20.0, 44.0));
                        let resp = ui.interact(card, egui::Id::new("rail_session"), Sense::click());
                        let lit = resp.hovered() || self.session_panel;
                        painter.rect(card, CornerRadius::same(theme::RADIUS_SMALL + 2), SURFACE, Stroke::new(1.0, if lit { SURFACE_ACTIVE } else { theme::OUTLINE }), egui::StrokeKind::Inside);
                        painter.circle_filled(pos2(rect.left() + RAIL_W / 2.0, card.center().y), 4.0, theme::ACCENT);
                        if t > 0.05 {
                            let text = Rect::from_min_max(pos2(rect.left() + RAIL_W / 2.0 + 14.0, card.top()), card.right_bottom() - vec2(8.0, 0.0));
                            let p = ui.painter_at(text);
                            p.text(pos2(text.left(), card.center().y - 8.0), egui::Align2::LEFT_CENTER, title, medium(13.0), TEXT.gamma_multiply(t));
                            p.text(pos2(text.left(), card.center().y + 9.0), egui::Align2::LEFT_CENTER, detail, regular(12.0), SECONDARY.gamma_multiply(t));
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.on_hover_text("What you're looking at: click to change").clicked() {
                            self.session_panel = !self.session_panel;
                            self.page = Page::Analysis;
                        }
                        y += 52.0;
                    }
                    for (view, icon, label) in VIEWS {
                        let selected = self.page == Page::Analysis && self.analysis_view.rail_item() == view;
                        let off = session.and_then(|s| view_unavailable(s, view));
                        let label = if view == AnalysisView::Compare && compare_n > 0 { format!("{label} ({compare_n})") } else { label.to_string() };
                        if rail_row(ui, rect, width, y, icon, &label, selected, off, t).clicked() {
                            self.page = Page::Analysis;
                            if self.analysis_view.rail_item() != view {
                                self.analysis_view = view;
                            }
                        }
                        y += ROW_STEP;
                    }
                    separator(y + 4.0);
                    y += 14.0;
                    for (page, icon, label) in BOTTOM_PAGES {
                        if rail_row(ui, rect, width, y, icon, label, self.page == page, None, t).clicked() {
                            self.page = page;
                        }
                        y += ROW_STEP;
                    }
                    // Settings at the bottom of the rail.
                    if rail_row(ui, rect, width, rect.bottom() - 56.0, Icon::Settings, "Settings", self.page == Page::Settings, None, t).clicked() {
                        self.page = Page::Settings;
                    }
                });
            });
    }

    /// The open session in two short lines: "mka · Session 35" and
    /// "Sat 10/10 · Timmy · Lap 10".
    fn session_label(&mut self, session: &Session) -> (String, String) {
        let i = &session.info;
        let chosen = self.path.clone().and_then(|p| self.session_track_name(&p));
        let track = chosen.or_else(|| i.venue.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Session".into());
        let number = self.path.as_ref().and_then(|p| p.file_stem()).and_then(|n| aim_xrk::summary::session_number(&n.to_string_lossy()));
        let date = self
            .path
            .as_ref()
            .and_then(|p| self.library.get_or_insert_with(Library::load).sessions.iter().find(|s| &s.path == p).and_then(|s| s.summary.date))
            .map(short_day)
            .or_else(|| i.date.clone());
        let lap = match self.lap.and_then(|k| session.laps.get(k)) {
            Some(l) => format!("Lap {}", l.number),
            None => "All laps".to_string(),
        };
        let title = [Some(track), number.map(|n| format!("Session {n}"))].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let detail = [date, i.driver.clone().filter(|d| !d.is_empty()), Some(lap)].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        (title, detail)
    }

    /// The pill at the top of each analysis view: what you're looking at, which
    /// opens the session panel; warnings beside it. Returns the pill's rect.
    pub(super) fn session_pill(&mut self, ui: &mut Ui, session: &Session) -> Rect {
        let (title, detail) = self.session_label(session);
        let time = self.lap.and_then(|k| session.laps.get(k)).or_else(|| session.best_lap(MIN_LAP_MS)).map(|l| format_lap_time(l.duration_ms()));
        let gearing = self.gearing_warning();
        let sensors = self.open_session_check(session);
        let mut pill = Rect::NOTHING;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let mut job = egui::text::LayoutJob::default();
            let mut add = |text: &str, font: egui::FontId, color: Color32| job.append(text, 0.0, egui::TextFormat { font_id: font, color, ..Default::default() });
            add(&title, semibold(13.5), TEXT);
            add(&format!("  ·  {detail}"), regular(13.5), SECONDARY);
            if let Some(t) = &time {
                add(&format!("   {t}"), semibold(13.5), TEXT);
            }
            let galley = ui.painter().layout_job(job);
            let size = galley.size() + vec2(28.0 + 22.0, 14.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let lit = resp.hovered() || self.session_panel;
            ui.painter().rect(rect, CornerRadius::same(255), SURFACE, Stroke::new(1.0, if lit { SURFACE_ACTIVE } else { theme::OUTLINE }), egui::StrokeKind::Inside);
            ui.painter().galley(rect.left_center() + vec2(14.0, -galley.size().y / 2.0), galley, TEXT);
            Icon::ChevronDown.image(if lit { TEXT } else { SECONDARY }, 14.0).paint_at(ui, Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), Vec2::splat(14.0)));
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text("The session, its setup and laps: click to change what you're looking at").clicked() {
                self.session_panel = !self.session_panel;
            }
            pill = rect;
            // Sensors that stopped recording in this session.
            if let Some(checked) = sensors {
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
            if let Some(why) = gearing {
                ui.label(RichText::new("Gearing doesn't match the data").size(13.0).color(theme::FAVORITE)).on_hover_text(why);
            }
        });
        pill
    }

    /// Each view's title, with what belongs beside it (Graphs' tabs).
    pub(super) fn view_title(&mut self, ui: &mut Ui, title: &str) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(title).font(semibold(26.0)).color(TEXT));
            if self.analysis_view.rail_item() == AnalysisView::Session {
                ui.add_space(14.0);
                for (view, label, hint) in [
                    (AnalysisView::Session, "Channels", "Channels through a lap, with the track map"),
                    (AnalysisView::Histogram, "Histogram", "How much time a channel spends in each band, e.g. RPM"),
                    (AnalysisView::Scatter, "Scatter", "One channel against another: RPM vs speed, the grip circle"),
                ] {
                    if chip(ui, label, self.analysis_view == view).on_hover_text(hint).clicked() {
                        self.analysis_view = view;
                    }
                }
            }
        });
    }

    /// The panel the pill and the rail's session indicator open: the session
    /// (today's header and details), its laps, and other sessions to open.
    pub(super) fn session_panel(&mut self, ctx: &egui::Context, session: &std::sync::Arc<Session>) {
        let Some(pill) = self.pill_rect.filter(|_| self.session_panel) else { return };
        let screen = ctx.content_rect();
        let width = (screen.right() - 28.0 - pill.left()).min(1280.0);
        let mut open_path = None;
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("session_panel")).order(egui::Order::Foreground).fixed_pos(pill.left_bottom() + vec2(0.0, 8.0)).show(ctx, |ui| {
            Frame::new()
                .fill(SURFACE)
                .stroke(Stroke::new(1.0, SURFACE_ACTIVE))
                .corner_radius(CornerRadius::same(14))
                .shadow(egui::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(165) })
                .inner_margin(Margin::same(20))
                .show(ui, |ui| {
                    ui.set_width(width - 40.0);
                    let gap = 22.0;
                    let inner = width - 40.0 - 2.0 * gap;
                    let widths = [inner * 0.5, inner * 0.28, inner * 0.22];
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.vertical(|ui| {
                            ui.set_width(widths[0]);
                            self.panel_session(ui, session);
                        });
                        ui.vertical(|ui| {
                            ui.set_width(widths[1]);
                            if self.panel_laps(ui, session, widths[1]) {
                                close = true;
                            }
                        });
                        ui.vertical(|ui| {
                            ui.set_width(widths[2]);
                            open_path = self.panel_sessions(ui);
                        });
                    });
                });
        });
        // Click anywhere else (but the pill and the rail's indicator, which toggle it) or Escape to close.
        let outside = ui_clicked_outside(ctx, area.response.rect, pill, self.rail_rect);
        if close || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.session_panel = false;
        }
        if let Some(p) = open_path {
            self.session_panel = false;
            self.load(p);
        }
        if self.page != Page::Analysis {
            self.session_panel = false;
        }
    }

    /// The panel's first column: today's session header and details.
    fn panel_session(&mut self, ui: &mut Ui, session: &Session) {
        let i = &session.info;
        let chosen = self.path.clone().and_then(|p| self.session_track_name(&p));
        let title = chosen.or_else(|| i.venue.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Session".into());
        let when = match (i.date.as_deref(), i.time.as_deref()) {
            (Some(d), Some(t)) => Some(format!("{d} at {}", crate::events::clock_time(t))),
            (Some(d), None) => Some(d.to_string()),
            _ => None,
        };
        let sub = [i.session.clone(), when, i.driver.clone(), i.vehicle.clone()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  ");
        ui.horizontal(|ui| {
            let t = ui.add(egui::Label::new(RichText::new(title).font(semibold(22.0)).color(TEXT)).sense(Sense::click()));
            if t.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if t.on_hover_text("Change the track this session was at").clicked() {
                if let Some(p) = self.path.clone() {
                    self.open_track_pick(p);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, Some(Icon::FolderOpen), "Open…", false).on_hover_text("Open a session file").clicked() {
                    self.open_dialog();
                }
            });
        });
        ui.add(egui::Label::new(RichText::new(sub).font(regular(14.0)).color(SECONDARY)).wrap());
        ui.add_space(12.0);
        summary_tiles(ui, session);
        ui.add_space(10.0);
        self.setup_strip(ui);
        ui.add_space(6.0);
        self.weather_line(ui);
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
        // The IMU's mounting, worked out from the session: reassurance, not a setting.
        if let Some(m) = session.info.imu_mount {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(Icon::Mychron.image(DIM, 14.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("The MyChron leans {:.0}° on its mount. Forward g and Vertical g are corrected for it automatically.", m.tilt_deg)).size(12.5).color(DIM),
                    )
                    .wrap(),
                );
            });
        }
    }

    /// The panel's lap picker: the whole session, then every lap. True when one was picked.
    fn panel_laps(&mut self, ui: &mut Ui, session: &Session, width: f32) -> bool {
        ui.label(RichText::new("LAP").font(semibold(11.5)).color(DIM).extra_letter_spacing(0.8));
        ui.add_space(6.0);
        let slow = slow_laps(session);
        let best = session.best_lap(MIN_LAP_MS).copied();
        let gap = 6.0;
        let w = (width - gap * 2.0) / 3.0;
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            let cell = |ui: &mut Ui, on: bool, dim: bool, number: &str, time: &str, color: Color32| {
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
                let p = ui.painter();
                p.rect(rect, CornerRadius::same(theme::RADIUS_SMALL), if on { SURFACE_ACTIVE } else if resp.hovered() { SURFACE_HOVER } else { SURFACE }, Stroke::new(1.0, if on { SECONDARY } else { theme::OUTLINE }), egui::StrokeKind::Inside);
                p.text(rect.left_center() + vec2(9.0, 0.0), egui::Align2::LEFT_CENTER, number, regular(12.5), DIM);
                p.text(rect.right_center() - vec2(9.0, 0.0), egui::Align2::RIGHT_CENTER, time, medium(13.0), if dim { DIM } else { color });
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                resp
            };
            if cell(ui, self.lap.is_none(), false, "All", "", TEXT).on_hover_text("The whole session").clicked() {
                picked = Some(None);
            }
            for (k, lap) in session.laps.iter().enumerate() {
                let color = if Some(*lap) == best { theme::GOOD } else { TEXT };
                let r = cell(ui, self.lap == Some(k), slow[k], &lap.number.to_string(), &format_lap_time(lap.duration_ms()), color);
                if r.on_hover_text(if slow[k] { "Out, in or slow lap" } else { "" }).clicked() {
                    picked = Some(Some(k));
                }
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Dimmed: out, in and slow laps.").size(12.0).color(DIM));
        if let Some(choice) = picked {
            self.lap = choice;
            self.cursor_s = None;
            return true;
        }
        false
    }

    /// The panel's other sessions at this track, newest race day first. Returns one picked.
    fn panel_sessions(&mut self, ui: &mut Ui) -> Option<PathBuf> {
        ui.label(RichText::new("ANOTHER SESSION").font(semibold(11.5)).color(DIM).extra_letter_spacing(0.8));
        ui.add_space(6.0);
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.take().unwrap_or_else(crate::profiles::Profiles::load);
        let here = self.path.as_ref().and_then(|p| lib.sessions.iter().find(|s| &s.path == p));
        let (track, logger) = (here.and_then(|s| s.track), here.map(|s| s.logger.clone()));
        let mut rows: Vec<usize> = (0..lib.sessions.len())
            .filter(|&i| {
                let s = &lib.sessions[i];
                (track.is_some() && s.track == track) || (track.is_none() && Some(&s.logger) == logger.as_ref())
            })
            .filter(|&i| !crate::events::is_warm_up(&lib.sessions[i].summary))
            .collect();
        rows.sort_by_key(|&i| {
            let s = &lib.sessions[i].summary;
            std::cmp::Reverse((s.date, s.start_time.clone(), s.number))
        });
        rows.truncate(10);
        let mut open = None;
        let mut last_date = None;
        for &i in &rows {
            let s = &lib.sessions[i];
            if last_date != Some(s.summary.date) {
                last_date = Some(s.summary.date);
                ui.add_space(6.0);
                let day = s.summary.date.map(long_date).unwrap_or_else(|| "Unknown date".into());
                ui.label(RichText::new(day.to_uppercase()).font(medium(11.0)).color(DIM).extra_letter_spacing(0.5));
                ui.add_space(2.0);
            }
            let on = self.path.as_ref() == Some(&s.path);
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
            if on || resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL), if on { SURFACE_ACTIVE } else { SURFACE_HOVER });
            }
            let who = session_driver(s, &profiles);
            let name = format!("Session {}{}", s.summary.number.unwrap_or(0), if who.is_empty() { String::new() } else { format!("  ·  {who}") });
            ui.painter().text(rect.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, name, if on { semibold(13.0) } else { regular(13.0) }, TEXT);
            if let Some(b) = s.summary.best_lap_ms {
                ui.painter().text(rect.right_center() - vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, format_lap_time(b as i64), regular(12.5), DIM);
            }
            if resp.hovered() && !on {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() && !on {
                open = Some(s.path.clone());
            }
        }
        ui.add_space(8.0);
        if ui.add(egui::Label::new(RichText::new("All sessions…").size(13.0).color(SECONDARY)).sense(Sense::click())).clicked() {
            self.page = Page::Sessions;
        }
        self.library = Some(lib);
        self.profiles = Some(profiles);
        open
    }
}

/// A click this frame outside `panel`, `pill` and the rail (whose session
/// indicator toggles the panel itself).
fn ui_clicked_outside(ctx: &egui::Context, panel: Rect, pill: Rect, rail: Option<Rect>) -> bool {
    ctx.input(|i| i.pointer.any_click()) && ctx.pointer_interact_pos().is_some_and(|p| !panel.contains(p) && !pill.contains(p) && !rail.is_some_and(|r| r.contains(p)))
}

impl App {
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
        // Every lap the same width, the row filling the page.
        let all_w = if compare_mode { 86.0 } else { 64.0 };
        let gap = 4.0;
        let avail = ui.available_width() - all_w - gap * laps.len() as f32;
        let w = (avail / laps.len() as f32).max(28.0);
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
                } else if let (true, Some(b)) = (w >= 44.0, best) {
                    // Too narrow for a full time: the best keeps its time to the hundredth, the rest show their gap to it.
                    let text = if is_best {
                        let t = format_lap_time(lap.duration_ms());
                        t[..t.len() - 1].to_string()
                    } else {
                        format!("+{:.2}", (lap.duration_ms() - b.duration_ms()) as f64 / 1000.0)
                    };
                    p.text(pos2(rect.left() + 5.0, rect.top() + 13.0), egui::Align2::LEFT_CENTER, lap.number.to_string(), regular(11.0), DIM);
                    p.text(pos2(rect.left() + 5.0, rect.top() + 31.0), egui::Align2::LEFT_CENTER, text, medium(12.0), time_color);
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
