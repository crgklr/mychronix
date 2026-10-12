//! The Garage, Mychronix's home: the MyChrons, the drivers with their seat
//! time, the karts with the setup they're running, the last race day and
//! what's coming up. And the session picker Analysis shows before a session
//! is open.

use super::*;
use crate::events;
use crate::setup::{self, Baselines};

/// One MyChron's driver and numbers, from on-track sessions (warm-ups left out).
struct DriverRow {
    logger: String,
    name: String,
    kart: String,
    engine: String,
    seat_ms: u64,
    laps: u32,
    days: Vec<aim_xrk::summary::Date>,
    /// Seat time per race day, oldest first: (date, ms).
    per_day: Vec<(aim_xrk::summary::Date, u64)>,
    /// Best lap at the track raced most: (track, lap ms, session index).
    best: Option<(String, u64, usize)>,
    /// Newest session, for editing the setup from here.
    newest: Option<usize>,
}

fn driver_rows(lib: &Library, profiles: &crate::profiles::Profiles) -> Vec<DriverRow> {
    let mut loggers: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).chain(profiles.loggers.keys().cloned()).collect();
    loggers.sort();
    loggers.dedup();
    loggers
        .into_iter()
        .map(|logger| {
            let p = profiles.get(&logger);
            let name = p.map(|p| p.driver.clone()).filter(|d| !d.trim().is_empty()).unwrap_or_else(|| format!("Logger {logger}"));
            let kart = p.map(|p| p.kart.clone()).unwrap_or_default();
            let engine = p.map(|p| p.engine.clone()).unwrap_or_default();
            let mine: Vec<usize> = (0..lib.sessions.len()).filter(|&i| lib.sessions[i].logger == logger).collect();
            let on: Vec<usize> = mine.iter().copied().filter(|&i| !events::is_warm_up(&lib.sessions[i].summary)).collect();
            let seat_ms = on.iter().map(|&i| lib.sessions[i].summary.duration_ms.unwrap_or(0)).sum();
            let laps = on.iter().map(|&i| lib.sessions[i].summary.laps.unwrap_or(0)).sum();
            let mut per_day: Vec<(aim_xrk::summary::Date, u64)> = Vec::new();
            for &i in &on {
                if let Some(d) = lib.sessions[i].summary.date {
                    let ms = lib.sessions[i].summary.duration_ms.unwrap_or(0);
                    match per_day.iter_mut().find(|x| x.0 == d) {
                        Some(x) => x.1 += ms,
                        None => per_day.push((d, ms)),
                    }
                }
            }
            per_day.sort_by_key(|x| x.0);
            let days = per_day.iter().map(|x| x.0).collect();
            // The track with the most on-track sessions, and the best lap there.
            let mut counts: Vec<(String, usize)> = Vec::new();
            for &i in &on {
                let t = lib.sessions[i].summary.track.clone().unwrap_or_default();
                match counts.iter_mut().find(|c| c.0 == t) {
                    Some(c) => c.1 += 1,
                    None => counts.push((t, 1)),
                }
            }
            let best = counts
                .iter()
                .filter(|(t, _)| on.iter().any(|&i| lib.sessions[i].summary.track.as_deref() == Some(t) && lib.sessions[i].summary.best_lap_ms.is_some()))
                .max_by_key(|c| c.1)
                .and_then(|(t, _)| {
                    on.iter()
                        .filter(|&&i| lib.sessions[i].summary.track.as_deref() == Some(t))
                        .filter_map(|&i| lib.sessions[i].summary.best_lap_ms.map(|b| (t.clone(), b, i)))
                        .min_by_key(|x| x.1)
                });
            let newest = mine.iter().copied().max_by_key(|&i| {
                let s = &lib.sessions[i].summary;
                (s.date, s.start_time.clone(), s.number)
            });
            DriverRow { logger, name, kart, engine, seat_ms, laps, days, per_day, best, newest }
        })
        .collect()
}

fn short_date(d: aim_xrk::summary::Date) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}", d.day, M[(d.month.clamp(1, 12) - 1) as usize])
}

fn hours(ms: u64) -> String {
    let m = ms / 60_000;
    if m >= 60 { format!("{}h {:02}m", m / 60, m % 60) } else { format!("{m}m") }
}

/// A section of a fixed size on the page background (no box), with a small
/// uppercase heading and an optional pill on the right. Fixed sizes keep rows even.
fn panel(ui: &mut Ui, size: Vec2, title: &str, pill: Option<(&str, Color32)>, add: impl FnOnce(&mut Ui)) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let inner = rect.shrink2(vec2(0.0, 4.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rect);
    // The page's wide gaps are between sections, not inside them.
    child.spacing_mut().item_spacing = vec2(8.0, 6.0);
    child.horizontal(|ui| {
        ui.label(RichText::new(title.to_uppercase()).font(semibold(12.0)).color(DIM).extra_letter_spacing(0.8));
        if let Some((text, color)) = pill {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| pill_label(ui, text, color));
        }
    });
    child.add_space(10.0);
    add(&mut child);
    resp
}

/// A small rounded status label.
fn pill_label(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.into(), medium(12.0), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(18.0, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(255), color.gamma_multiply(0.16));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// A round avatar with the driver's initial.
fn avatar(ui: &mut Ui, name: &str, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, SURFACE_ACTIVE);
    ui.painter().circle_stroke(rect.center(), size / 2.0 - 1.0, Stroke::new(2.0, color));
    let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, semibold(size * 0.42), TEXT);
}

impl App {
    pub(super) fn garage_page(&mut self, ui: &mut Ui) {
        self.device_poll(&ui.ctx().clone());
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.take().unwrap_or_else(crate::profiles::Profiles::load);
        let baselines = self.setup_baselines.take().unwrap_or_else(Baselines::load);
        let defs = self.events.take().unwrap_or_else(crate::events::Events::load);
        let drivers = driver_rows(&lib, &profiles);
        let resolved = setup::resolve(&lib, &baselines);
        let all_events = events::group(&lib, &profiles, &defs);
        let today = events::today();
        let connected_logger = self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string());
        let model = self.dev_current_wifi.as_deref().map(model_from_ssid).unwrap_or("MyChron");
        let new_count = self.new_on_logger().len();
        let mut go: Option<Page> = None;
        let mut open_event: Option<String> = None;
        let mut plan = false;
        let mut edit_setup: Option<usize> = None;

        // Header.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Garage").font(semibold(28.0)).color(TEXT));
                let n = drivers.len();
                let days: usize = {
                    let mut d: Vec<_> = drivers.iter().flat_map(|d| d.days.clone()).collect();
                    d.sort();
                    d.dedup();
                    d.len()
                };
                ui.label(RichText::new(format!("{n} driver{}  ·  {n} kart{}  ·  {days} race day{} this season", if n == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" }, if days == 1 { "" } else { "s" })).font(regular(14.5)).color(SECONDARY));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if connected_logger.is_some() && new_count > 0 && button(ui, Some(Icon::FolderOpen), &format!("Download {new_count} new session{}", if new_count == 1 { "" } else { "s" }), true).clicked() {
                    go = Some(Page::Device);
                }
            });
        });
        ui.add_space(18.0);

        // Three rows sized to the window: status, drivers, karts and race days.
        let gap = 28.0;
        let w = ui.available_width();
        let avail = ui.available_height();
        let row1 = (avail * 0.27).clamp(176.0, 236.0);
        let row2 = 136.0;
        let row3 = (avail - row1 - row2 - gap * 2.0 - 2.0).max(260.0);
        egui::ScrollArea::vertical().id_salt("garage").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            let half = (w - gap) / 2.0;

            // Row 1: MyChrons and seat time.
            ui.horizontal_top(|ui| {
                let any = drivers.iter().any(|d| Some(&d.logger) == connected_logger.as_ref());
                let pill = if any { ("Connected", theme::GOOD) } else { ("Not connected", DIM) };
                panel(ui, vec2(half, row1), "MyChrons", Some(pill), |ui| {
                    ui.spacing_mut().item_spacing.y = 14.0;
                    for d in &drivers {
                        let on = Some(&d.logger) == connected_logger.as_ref();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            ui.add(Icon::Mychron.image(if on { theme::GOOD } else { SECONDARY }, 40.0));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(format!("{}'s {}", d.name, if on { model } else { "MyChron" })).font(semibold(17.0)).color(TEXT));
                                let line = match (on, self.dev_files.is_empty(), new_count) {
                                    (true, true, _) => "On its WiFi · reading its sessions…".to_string(),
                                    (true, false, 0) => "On its WiFi · everything downloaded".to_string(),
                                    (true, false, n) => format!("On its WiFi · {n} new session{} to download", if n == 1 { "" } else { "s" }),
                                    (false, ..) => "Off or out of range. Turn on its WiFi to connect.".to_string(),
                                };
                                ui.label(RichText::new(line).size(13.0).color(if on { theme::GOOD } else { SECONDARY }));
                                // What's already here from it.
                                let mine: Vec<&tracks::LocalSession> = lib.sessions.iter().filter(|s| s.logger == d.logger).collect();
                                let newest = mine.iter().filter_map(|s| s.summary.date).max();
                                ui.label(RichText::new(format!("{} sessions downloaded{}", mine.len(), newest.map(|n| format!("  ·  newest {}", short_date(n))).unwrap_or_default())).size(12.5).color(DIM));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(ui, Some(Icon::Wifi), if on { "Open" } else { "Connect" }, false).clicked() {
                                    go = Some(Page::Device);
                                }
                            });
                        });
                    }
                    if drivers.is_empty() {
                        ui.label(RichText::new("No MyChron yet. Turn on its WiFi and open Device to download your first sessions.").color(SECONDARY));
                    }
                });
                panel(ui, vec2(half, row1), "Seat time this season", None, |ui| {
                    // One row per driver: the total, then a bar per race day at full height.
                    let max = drivers.iter().flat_map(|d| d.per_day.iter().map(|x| x.1)).max().unwrap_or(1).max(1) as f32;
                    let row_h = ((ui.available_height() - 8.0 * (drivers.len().max(1) - 1) as f32) / drivers.len().max(1) as f32).max(60.0);
                    for (k, d) in drivers.iter().enumerate() {
                        ui.allocate_ui_with_layout(vec2(ui.available_width(), row_h), Layout::left_to_right(Align::Max), |ui| {
                            ui.spacing_mut().item_spacing.x = 24.0;
                            ui.vertical(|ui| {
                                ui.set_width(170.0);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.label(RichText::new(&d.name).size(13.0).color(SECONDARY));
                                ui.label(RichText::new(hours(d.seat_ms)).font(semibold(34.0)).color(TEXT));
                                ui.label(RichText::new(format!("on track  ·  {} laps  ·  {} day{}", d.laps, d.days.len(), if d.days.len() == 1 { "" } else { "s" })).size(12.5).color(DIM));
                            });
                            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                            let n = d.per_day.len().max(1) as f32;
                            let slot = (rect.width() / n).min(84.0);
                            let label_h = 18.0;
                            for (j, (date, ms)) in d.per_day.iter().enumerate() {
                                let h = ((*ms as f32 / max) * (rect.height() - label_h - 18.0)).max(2.0);
                                let x = rect.left() + j as f32 * slot;
                                let bar = Rect::from_min_max(pos2(x + 6.0, rect.bottom() - label_h - h), pos2(x + slot - 6.0, rect.bottom() - label_h));
                                ui.painter().rect_filled(bar, CornerRadius::same(4), SERIES[k % SERIES.len()].gamma_multiply(0.9));
                                ui.painter().text(pos2(bar.center().x, bar.top() - 8.0), egui::Align2::CENTER_CENTER, format!("{}m", ms / 60_000), regular(11.0), SECONDARY);
                                ui.painter().text(pos2(bar.center().x, rect.bottom() - 6.0), egui::Align2::CENTER_CENTER, short_date(*date), regular(11.0), DIM);
                            }
                        });
                    }
                });
            });

            // Row 2: drivers as tiles, plus a tile for the next one.
            panel(ui, vec2(w, row2), "Drivers", None, |ui| {
                let cols = 2.0;
                let tile_w = (ui.available_width() - gap) / cols;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (k, d) in drivers.iter().enumerate() {
                        let tile_h = 84.0;
                        let (rect, resp) = ui.allocate_exact_size(vec2(tile_w, tile_h), Sense::click());
                        let corner = CornerRadius::same(theme::RADIUS);
                        // An outline, not a fill; brighter on hover.
                        ui.painter().rect_stroke(rect, corner, Stroke::new(1.0, if resp.hovered() { SECONDARY } else { theme::OUTLINE }), egui::StrokeKind::Inside);
                        // The engine, about a third bigger than the tile and nudged left,
                        // so the tile's edges crop it.
                        let picture = crate::profiles::engine_picture(&d.engine);
                        let text_left = match picture {
                            Some((uri, bytes)) => {
                                let size = tile_h * 1.3;
                                let img = Rect::from_min_size(pos2(rect.left() - 30.0, rect.top() - 2.0), Vec2::splat(size));
                                let clip = rect.shrink(1.0);
                                let painter = ui.painter_at(clip);
                                let mut pic = ui.new_child(egui::UiBuilder::new().max_rect(img));
                                pic.set_clip_rect(clip);
                                egui::Image::from_bytes(uri, bytes).paint_at(&pic, img);
                                let _ = painter;
                                img.right() + 10.0
                            }
                            None => {
                                let mut a = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(rect.left_top() + vec2(16.0, (tile_h - 48.0) / 2.0), Vec2::splat(48.0))));
                                avatar(&mut a, &d.name, 48.0, SERIES[k % SERIES.len()]);
                                rect.left() + 78.0
                            }
                        };
                        // Name and kart on the left, best lap on the right, both centred on the tile.
                        let p = ui.painter_at(rect);
                        let mid = rect.center().y;
                        let kart = [d.kart.clone(), d.engine.clone()].into_iter().filter(|x| !x.trim().is_empty()).collect::<Vec<_>>().join("  ·  ");
                        p.text(pos2(text_left, mid - 11.0), egui::Align2::LEFT_CENTER, &d.name, semibold(18.0), TEXT);
                        p.text(pos2(text_left, mid + 12.0), egui::Align2::LEFT_CENTER, if kart.is_empty() { "No kart named".to_string() } else { kart }, regular(13.0), SECONDARY);
                        if let Some((track, ms, _)) = &d.best {
                            let right = rect.right() - 18.0;
                            p.text(pos2(right, mid - 12.0), egui::Align2::RIGHT_CENTER, format!("Best at {track}"), regular(12.5), DIM);
                            p.text(pos2(right, mid + 10.0), egui::Align2::RIGHT_CENTER, format_lap_time(*ms as i64), semibold(22.0), theme::GOOD);
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.on_hover_text("Open the best lap").clicked() {
                            if let Some((_, _, i)) = d.best {
                                self.pending_open = Some(lib.sessions[i].path.clone());
                            }
                        }
                    }
                });
            });

            // Row 3: karts with their setup, the last race day, what's coming up.
            let third = (w - gap * 2.0) / 3.0;
            ui.horizontal_top(|ui| {
                panel(ui, vec2(third, row3), "Karts", None, |ui| {
                    for d in &drivers {
                        let Some(newest) = d.newest else { continue };
                        let r = &resolved[newest];
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(RichText::new(if d.kart.is_empty() { "Kart" } else { &d.kart }).font(semibold(16.0)).color(TEXT));
                                ui.label(RichText::new(&d.name).size(12.5).color(DIM));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(ui, Some(Icon::Settings), "Change", false).on_hover_text("Change the setup from the newest session on").clicked() {
                                    edit_setup = Some(newest);
                                }
                            });
                        });
                        ui.add_space(6.0);
                        let s = &r.setup;
                        let rows: Vec<(&str, String)> = [
                            ("Gearing", if s.drive_teeth.trim().is_empty() && s.driven_teeth.trim().is_empty() { String::new() } else { format!("{} / {}", s.drive_teeth.trim(), s.driven_teeth.trim()) }),
                            ("Tyres", s.tyres.trim().to_string()),
                            ("Pressure", match (s.front_psi.trim(), s.rear_psi.trim()) { ("", "") => String::new(), (f, r) if f == r => format!("{f} psi"), (f, r) => format!("{f} / {r} psi") }),
                            ("Weight", if s.weight_lb.trim().is_empty() { String::new() } else { format!("{} lb with driver", s.weight_lb.trim()) }),
                            ("Ballast", if s.ballast_lb.trim().is_empty() { String::new() } else { format!("{} lb", s.ballast_lb.trim()) }),
                            ("Jetting", s.jetting.trim().to_string()),
                        ]
                        .into_iter()
                        .filter(|(_, v)| !v.is_empty())
                        .collect();
                        egui::Grid::new(("kart_setup", d.logger.as_str())).num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                            for (k, v) in &rows {
                                ui.label(RichText::new(*k).size(13.0).color(DIM));
                                ui.label(RichText::new(v).size(13.5).color(TEXT));
                                ui.end_row();
                            }
                        });
                        if rows.is_empty() {
                            ui.label(RichText::new("No setup recorded yet.").color(SECONDARY));
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new(setup_last_changed(&lib, &resolved, &d.logger)).size(12.5).color(DIM));
                        ui.add_space(10.0);
                    }
                });

                let last = all_events.iter().find(|e| !e.sessions.is_empty());
                let date_pill = last.map(|e| short_date(e.start));
                let resp = panel(ui, vec2(third, row3), "Last race day", date_pill.as_deref().map(|d| (d, SECONDARY)), |ui| {
                    let Some(e) = last else {
                        ui.label(RichText::new("No race days yet.").color(SECONDARY));
                        return;
                    };
                    let on: Vec<usize> = e.sessions.iter().copied().filter(|&i| !events::is_warm_up(&lib.sessions[i].summary)).collect();
                    let best = on.iter().filter_map(|&i| lib.sessions[i].summary.best_lap_ms).min();
                    // The track's outline, when the library has it.
                    let outline = lib.tracks.iter().find(|t| t.name.eq_ignore_ascii_case(&e.track)).map(|t| t.outline.clone()).unwrap_or_default();
                    let map_h = (ui.available_height() - 64.0).max(80.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), map_h), Sense::hover());
                    outline_thumbnail(ui.painter(), rect, &outline, TEXT.gamma_multiply(0.85));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(e.title()).font(semibold(16.0)).color(TEXT));
                            ui.label(RichText::new(format!("{} sessions on track", on.len())).size(12.5).color(DIM));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(b) = best {
                                ui.label(RichText::new(format_lap_time(b as i64)).font(semibold(20.0)).color(theme::GOOD));
                            }
                        });
                    });
                });
                if resp.hovered() && last.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.on_hover_text("Open this race day").clicked() {
                    open_event = last.map(|e| e.key());
                }

                panel(ui, vec2(third, row3), "Coming up", None, |ui| {
                    let upcoming: Vec<&events::Event> = all_events.iter().filter(|e| e.start > today || (e.sessions.is_empty() && e.end >= today)).collect();
                    for e in upcoming.iter().rev().take(4) {
                        let r = row(ui, false, |ui| {
                            ui.label(RichText::new(e.title()).font(medium(14.5)).color(TEXT));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let n = events::days(e.start) - events::days(today);
                                ui.label(RichText::new(if n > 1 { format!("in {n} days") } else if n == 1 { "tomorrow".into() } else { "today".into() }).size(12.5).color(DIM));
                            });
                        });
                        if r.clicked() {
                            open_event = Some(e.key());
                        }
                    }
                    if upcoming.is_empty() {
                        ui.label(RichText::new("Nothing planned").font(medium(15.0)).color(TEXT));
                        ui.label(RichText::new("Plan your next race, and the sessions you download there file themselves under it.").size(13.0).color(SECONDARY));
                    }
                    ui.add_space(8.0);
                    if button(ui, Some(Icon::Calendar), "Plan an event", false).clicked() {
                        plan = true;
                    }
                });
            });
        });

        if let Some(i) = edit_setup {
            let path = lib.sessions[i].path.clone();
            self.setup_edit = Some(setup_ui::SetupEdit {
                path,
                setup: resolved[i].setup.clone(),
                before: setup::before(&lib, &baselines, i),
                has_own: resolved[i].source == setup::Source::Own,
            });
        }
        self.library = Some(lib);
        self.profiles = Some(profiles);
        self.setup_baselines = Some(baselines);
        self.events = Some(defs);
        self.setup_dialog(ui.ctx());
        if let Some(key) = open_event {
            self.event_sel = Some(key);
            self.page = Page::Events;
        }
        if plan {
            self.page = Page::Events;
            self.plan_on_open = true;
        }
        if let Some(p) = go {
            self.page = p;
        }
        if let Some(path) = self.pending_open.take() {
            self.load(path);
        }
    }

    /// Analysis with nothing open: pick a session, race days newest first.
    pub(super) fn session_picker(&mut self, ui: &mut Ui) {
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.take().unwrap_or_else(crate::profiles::Profiles::load);
        let mut open = None;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Pick a session to analyse").font(semibold(26.0)).color(TEXT));
                ui.label(RichText::new("Your downloaded sessions, newest race day first.").font(regular(14.5)).color(SECONDARY));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, Some(Icon::FolderOpen), "Open a file…", false).on_hover_text("A .xrk or .xrz from somewhere else").clicked() {
                    self.open_dialog();
                }
            });
        });
        ui.add_space(12.0);
        let mut loggers: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).collect();
        loggers.sort();
        loggers.dedup();
        let name = |l: &str| profiles.get(l).map(|p| p.driver.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| format!("Logger {l}"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if loggers.len() > 1 {
                if chip(ui, "All drivers", self.picker_logger.is_none()).clicked() {
                    self.picker_logger = None;
                }
                for l in &loggers {
                    if chip(ui, &name(l), self.picker_logger.as_deref() == Some(l)).clicked() {
                        self.picker_logger = Some(l.clone());
                    }
                }
                ui.add_space(10.0);
            }
            if chip(ui, "Show warm-ups", self.picker_warmups).on_hover_text("Sessions where the kart never left the stand").clicked() {
                self.picker_warmups = !self.picker_warmups;
            }
        });
        ui.add_space(10.0);
        let mut shown: Vec<usize> = (0..lib.sessions.len())
            .filter(|&i| self.picker_logger.as_ref().is_none_or(|l| *l == lib.sessions[i].logger))
            .filter(|&i| self.picker_warmups || !events::is_warm_up(&lib.sessions[i].summary))
            .collect();
        shown.sort_by_key(|&i| {
            let s = &lib.sessions[i].summary;
            std::cmp::Reverse((s.date, s.start_time.clone(), s.number))
        });
        if lib.sessions.is_empty() {
            card(ui, |ui| {
                ui.label(RichText::new("No sessions yet").font(semibold(17.0)).color(TEXT));
                ui.label(RichText::new("Connect your MyChron on the Device page to download them, or open a file.").color(SECONDARY));
            });
        }
        egui::ScrollArea::vertical().id_salt("picker").auto_shrink([false, false]).show(ui, |ui| {
            let mut k = 0;
            while k < shown.len() {
                let date = lib.sessions[shown[k]].summary.date;
                let mut end = k;
                while end < shown.len() && lib.sessions[shown[end]].summary.date == date {
                    end += 1;
                }
                let day = &shown[k..end];
                let best = day.iter().filter(|&&i| !events::is_warm_up(&lib.sessions[i].summary)).filter_map(|&i| lib.sessions[i].summary.best_lap_ms).min();
                ui.add_space(if k == 0 { 0.0 } else { 14.0 });
                ui.horizontal(|ui| {
                    ui.add(Icon::Calendar.image(SECONDARY, 16.0));
                    ui.label(RichText::new(date.map(long_date).unwrap_or_else(|| "Unknown date".into())).font(semibold(16.0)).color(TEXT));
                    let mut tracks: Vec<String> = day.iter().filter_map(|&i| lib.sessions[i].summary.track.clone()).collect();
                    tracks.dedup();
                    ui.label(RichText::new(tracks.join(", ")).color(SECONDARY));
                });
                ui.add_space(6.0);
                for &i in day {
                    let s = &lib.sessions[i];
                    let mut who = s.meta.as_ref().map(|m| m.driver.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| name(&s.logger));
                    if events::is_warm_up(&s.summary) {
                        who = format!("Warm-up  ·  {who}");
                    }
                    if session_row(ui, &lib, i, &who, best, false).clicked() {
                        open = Some(s.path.clone());
                    }
                }
                k = end;
            }
        });
        self.library = Some(lib);
        self.profiles = Some(profiles);
        if let Some(p) = open {
            self.load(p);
        }
    }
}

/// "Changed in session 31 (10 Oct)", or where the setup comes from.
fn setup_last_changed(lib: &Library, resolved: &[setup::Resolved], logger: &str) -> String {
    let changed = (0..lib.sessions.len())
        .filter(|&i| lib.sessions[i].logger == logger && !resolved[i].changes.is_empty())
        .max_by_key(|&i| {
            let s = &lib.sessions[i].summary;
            (s.date, s.start_time.clone(), s.number)
        });
    match changed {
        Some(i) => {
            let s = &lib.sessions[i].summary;
            format!("Last changed in session {}{}", s.number.unwrap_or(0), s.date.map(|d| format!(" ({})", short_date(d))).unwrap_or_default())
        }
        None => "Unchanged since the baseline".to_string(),
    }
}
