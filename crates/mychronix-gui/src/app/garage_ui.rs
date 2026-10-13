//! The Garage, Mychronix's home: the MyChrons, the drivers with their seat
//! time, the karts with the setup they're running, the last race day and
//! what's coming up; adding and editing drivers and karts. And the session
//! picker Analysis shows before a session is open.

use super::*;
use crate::events;
use crate::profiles::{Driver, Kart, Profiles};
use crate::setup::{self, Baselines, Setup};

/// One driver and their numbers, from on-track sessions (warm-ups left out)
/// on the MyChrons of their karts.
struct DriverRow {
    /// None for a MyChron that isn't on a driver's kart.
    id: Option<u32>,
    name: String,
    kart: String,
    engine: String,
    loggers: Vec<String>,
    seat_ms: u64,
    laps: u32,
    days: Vec<aim_xrk::summary::Date>,
    /// Seat time per race day, oldest first: (date, ms).
    per_day: Vec<(aim_xrk::summary::Date, u64)>,
    /// Best lap at the track raced most: (track, lap ms, session index).
    best: Option<(String, u64, usize)>,
}

fn driver_rows(lib: &Library, profiles: &Profiles) -> Vec<DriverRow> {
    let mut groups: Vec<(Option<u32>, String, Vec<&Kart>, Vec<String>)> = profiles
        .drivers
        .iter()
        .map(|d| {
            let karts: Vec<&Kart> = profiles.karts.iter().filter(|k| k.driver == Some(d.id)).collect();
            let loggers = karts.iter().filter(|k| !k.logger.is_empty()).map(|k| k.logger.clone()).collect();
            (Some(d.id), d.name.clone(), karts, loggers)
        })
        .collect();
    // MyChrons on no driver's kart, by logger.
    let mut others: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).chain(profiles.karts.iter().map(|k| k.logger.clone())).filter(|l| !l.is_empty()).collect();
    others.sort();
    others.dedup();
    for l in others {
        if !groups.iter().any(|g| g.3.contains(&l)) {
            groups.push((None, format!("Logger {l}"), profiles.kart_on(&l).into_iter().collect(), vec![l]));
        }
    }
    groups
        .into_iter()
        .map(|(id, name, karts, loggers)| {
            let kart = karts.iter().map(|k| k.name.clone()).collect::<Vec<_>>().join(", ");
            let engine = karts.iter().map(|k| k.engine.clone()).find(|e| !e.trim().is_empty()).unwrap_or_default();
            let on: Vec<usize> = (0..lib.sessions.len()).filter(|&i| loggers.contains(&lib.sessions[i].logger) && !events::is_warm_up(&lib.sessions[i].summary)).collect();
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
            DriverRow { id, name, kart, engine, loggers, seat_ms, laps, days, per_day, best }
        })
        .collect()
}

/// A kart in the Karts panel: its profile (None for a MyChron not on a kart
/// yet), and its newest session.
struct KartRow {
    kart: Option<Kart>,
    logger: String,
    newest: Option<usize>,
}

fn kart_rows(lib: &Library, profiles: &Profiles) -> Vec<KartRow> {
    let newest = |logger: &str| {
        (0..lib.sessions.len()).filter(|&i| !logger.is_empty() && lib.sessions[i].logger == logger).max_by_key(|&i| {
            let s = &lib.sessions[i].summary;
            (s.date, s.start_time.clone(), s.number)
        })
    };
    let mut rows: Vec<KartRow> = profiles.karts.iter().map(|k| KartRow { kart: Some(k.clone()), logger: k.logger.clone(), newest: newest(&k.logger) }).collect();
    let mut loose: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).filter(|l| profiles.kart_on(l).is_none()).collect();
    loose.sort();
    loose.dedup();
    rows.extend(loose.into_iter().map(|l| KartRow { kart: None, newest: newest(&l), logger: l }));
    rows
}

/// A driver being added or edited (`driver.id` is 0 for a new one).
pub(super) struct DriverEdit {
    driver: Driver,
    confirm_delete: bool,
}

/// Classes offered while typing, besides those already entered.
const CLASSES: [&str; 22] = [
    "Kid Kart", "Micro Swift", "Mini Swift", "Cadet", "Junior 1", "Junior 2", "Junior 3", "KA100 Junior", "KA100 Senior", "X30 Junior", "X30 Senior",
    "Junior ROK", "Senior ROK", "Masters ROK", "Junior VLR", "Senior VLR", "Masters VLR", "LO206", "Clones", "Animal", "Open Class", "Masters",
];

/// A kart being added or edited.
pub(super) struct KartEdit {
    id: Option<u32>,
    name: String,
    engine: String,
    driver: Option<u32>,
    logger: String,
    baseline: Setup,
    /// Typing an engine that isn't in the list.
    other_engine: bool,
    confirm_delete: bool,
}

impl KartEdit {
    fn new(profiles: &Profiles, kart: Option<&Kart>, logger: &str) -> Self {
        Self::for_driver(profiles, kart, logger, None)
    }

    fn for_driver(profiles: &Profiles, kart: Option<&Kart>, logger: &str, driver: Option<u32>) -> Self {
        match kart {
            Some(k) => KartEdit {
                id: Some(k.id),
                name: k.name.clone(),
                engine: k.engine.clone(),
                driver: k.driver,
                logger: k.logger.clone(),
                baseline: k.baseline.clone(),
                other_engine: !k.engine.trim().is_empty() && !crate::profiles::is_known_engine(&k.engine),
                confirm_delete: false,
            },
            None => KartEdit {
                id: None,
                name: String::new(),
                engine: String::new(),
                // A new kart for the only driver is theirs.
                driver: driver.or((profiles.drivers.len() == 1).then(|| profiles.drivers[0].id)),
                logger: logger.to_string(),
                baseline: Setup::default(),
                other_engine: false,
                confirm_delete: false,
            },
        }
    }
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
/// uppercase heading and, on the right, an optional pill and an optional
/// button (true when clicked). Fixed sizes keep rows even.
fn panel(ui: &mut Ui, size: Vec2, title: &str, pill: Option<(&str, Color32)>, action: Option<(Icon, &str)>, add: impl FnOnce(&mut Ui)) -> (egui::Response, bool) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let inner = rect.shrink2(vec2(0.0, 4.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rect);
    // The page's wide gaps are between sections, not inside them.
    child.spacing_mut().item_spacing = vec2(8.0, 6.0);
    let mut clicked = false;
    child.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.label(RichText::new(title.to_uppercase()).font(semibold(12.0)).color(DIM).extra_letter_spacing(0.8));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((icon, label)) = action {
                clicked = small_button(ui, icon, label).clicked();
            }
            if let Some((text, color)) = pill {
                pill_label(ui, text, color);
            }
        });
    });
    child.add_space(6.0);
    add(&mut child);
    (resp, clicked)
}

/// A quiet text button with an icon, for panel headings.
fn small_button(ui: &mut Ui, icon: Icon, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.into(), medium(13.0), SECONDARY);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 40.0, 28.0), Sense::click());
    let color = if resp.hovered() { TEXT } else { SECONDARY };
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(14), SURFACE_HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    icon.image(color, 14.0).paint_at(ui, Rect::from_min_size(pos2(rect.left() + 10.0, rect.center().y - 7.0), Vec2::splat(14.0)));
    ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), egui::Align2::LEFT_CENTER, label, medium(13.0), color);
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

/// "Logger 12345678", and the kart it's on if that isn't `except`.
fn logger_label(profiles: &Profiles, logger: &str, except: Option<u32>) -> String {
    match profiles.kart_on(logger) {
        Some(k) if Some(k.id) != except => format!("Logger {logger} (on {})", k.name),
        _ => format!("Logger {logger}"),
    }
}

impl App {
    pub(super) fn garage_page(&mut self, ui: &mut Ui) {
        self.device_poll(&ui.ctx().clone());
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.take().unwrap_or_else(Profiles::load);
        let baselines = self.setup_baselines.take().unwrap_or_else(Baselines::load);
        let defs = self.events.take().unwrap_or_else(crate::events::Events::load);
        let drivers = driver_rows(&lib, &profiles);
        let karts = kart_rows(&lib, &profiles);
        let resolved = setup::resolve(&lib, &baselines);
        let all_events = events::group(&lib, &profiles, &defs);
        let today = events::today();
        let connected_logger = self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string());
        let model = self.dev_current_wifi.as_deref().map(model_from_ssid).unwrap_or("MyChron");
        let new_count = self.new_on_logger().len();
        // Each MyChron and whose it is.
        let mychrons: Vec<(String, String)> = drivers.iter().flat_map(|d| d.loggers.iter().map(|l| (l.clone(), d.name.clone()))).collect();
        let mut go: Option<Page> = None;
        let mut open_event: Option<String> = None;
        let mut plan = false;
        let mut edit_setup: Option<usize> = None;
        let mut edit_driver: Option<DriverEdit> = None;
        let mut edit_kart: Option<KartEdit> = None;

        // Header.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Garage").font(semibold(28.0)).color(TEXT));
                let (n, k) = (profiles.drivers.len(), profiles.karts.len());
                let days: usize = {
                    let mut d: Vec<_> = drivers.iter().flat_map(|d| d.days.clone()).collect();
                    d.sort();
                    d.dedup();
                    d.len()
                };
                ui.label(RichText::new(format!("{n} driver{}  ·  {k} kart{}  ·  {days} race day{} this season", if n == 1 { "" } else { "s" }, if k == 1 { "" } else { "s" }, if days == 1 { "" } else { "s" })).font(regular(14.5)).color(SECONDARY));
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
                let any = mychrons.iter().any(|(l, _)| Some(l) == connected_logger.as_ref());
                let pill = if any { ("Connected", theme::GOOD) } else { ("Not connected", DIM) };
                panel(ui, vec2(half, row1), "MyChrons", Some(pill), None, |ui| {
                    ui.spacing_mut().item_spacing.y = 14.0;
                    for (logger, owner) in &mychrons {
                        let on = Some(logger) == connected_logger.as_ref();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            ui.add(Icon::Mychron.image(if on { theme::GOOD } else { SECONDARY }, 40.0));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(format!("{owner}'s {}", if on { model } else { "MyChron" })).font(semibold(17.0)).color(TEXT));
                                let line = match (on, self.dev_files.is_empty(), new_count) {
                                    (true, true, _) => "On its WiFi · reading its sessions…".to_string(),
                                    (true, false, 0) => "On its WiFi · everything downloaded".to_string(),
                                    (true, false, n) => format!("On its WiFi · {n} new session{} to download", if n == 1 { "" } else { "s" }),
                                    (false, ..) => "Off or out of range. Turn on its WiFi to connect.".to_string(),
                                };
                                ui.label(RichText::new(line).size(13.0).color(if on { theme::GOOD } else { SECONDARY }));
                                // What's already here from it.
                                let mine: Vec<&tracks::LocalSession> = lib.sessions.iter().filter(|s| &s.logger == logger).collect();
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
                    if mychrons.is_empty() {
                        ui.label(RichText::new("No MyChron yet. Turn on its WiFi and open Device to download your first sessions.").color(SECONDARY));
                    }
                });
                panel(ui, vec2(half, row1), "Seat time this season", None, None, |ui| {
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
                            if d.per_day.is_empty() {
                                ui.painter().text(pos2(rect.left(), rect.bottom() - 14.0), egui::Align2::LEFT_CENTER, "No sessions yet", regular(12.5), DIM);
                            }
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

            // Row 2: drivers as tiles.
            let (_, add_driver) = panel(ui, vec2(w, row2), "Drivers", None, Some((Icon::Plus, "Add driver")), |ui| {
                let cols = drivers.len().clamp(2, 4) as f32;
                let tile_w = (ui.available_width() - gap * (cols - 1.0)) / cols;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (k, d) in drivers.iter().enumerate() {
                        let tile_h = 84.0;
                        let (rect, resp) = ui.allocate_exact_size(vec2(tile_w, tile_h), Sense::click());
                        // The best lap opens on its own; the rest of the tile edits the driver.
                        let lap_rect = Rect::from_min_max(pos2(rect.right() - 150.0, rect.top()), rect.max);
                        let lap = d.best.as_ref().map(|_| ui.interact(lap_rect, ui.id().with(("best_lap", k)), Sense::click()));
                        let lap_hovered = lap.as_ref().is_some_and(|l| l.hovered());
                        let corner = CornerRadius::same(theme::RADIUS);
                        // An outline, not a fill; brighter on hover.
                        ui.painter().rect_stroke(rect, corner, Stroke::new(1.0, if resp.hovered() || lap_hovered { SECONDARY } else { theme::OUTLINE }), egui::StrokeKind::Inside);
                        // The engine, about a third bigger than the tile and nudged left,
                        // so the tile's edges crop it.
                        let text_left = match crate::profiles::engine_picture(&d.engine) {
                            Some((uri, bytes)) => {
                                let size = tile_h * 1.3;
                                let img = Rect::from_min_size(pos2(rect.left() - 30.0, rect.top() - 2.0), Vec2::splat(size));
                                let mut pic = ui.new_child(egui::UiBuilder::new().max_rect(img));
                                pic.set_clip_rect(rect.shrink(1.0));
                                egui::Image::from_bytes(uri, bytes).paint_at(&pic, img);
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
                        let person = d.id.and_then(|id| profiles.drivers.iter().find(|p| p.id == id));
                        let kart = [
                            person.map(|p| p.class.trim().to_string()).unwrap_or_default(),
                            person.and_then(|p| p.age(today)).map(|a| format!("age {a}")).unwrap_or_default(),
                            d.kart.clone(),
                        ]
                        .into_iter()
                        .filter(|x| !x.trim().is_empty())
                        .collect::<Vec<_>>()
                        .join("  ·  ");
                        p.text(pos2(text_left, mid - 11.0), egui::Align2::LEFT_CENTER, &d.name, semibold(18.0), TEXT);
                        p.text(pos2(text_left, mid + 12.0), egui::Align2::LEFT_CENTER, if kart.is_empty() { "No kart yet".to_string() } else { kart }, regular(13.0), SECONDARY);
                        if let Some((track, ms, i)) = &d.best {
                            let right = rect.right() - 18.0;
                            p.text(pos2(right, mid - 12.0), egui::Align2::RIGHT_CENTER, format!("Best at {track}"), regular(12.5), DIM);
                            p.text(pos2(right, mid + 10.0), egui::Align2::RIGHT_CENTER, format_lap_time(*ms as i64), semibold(22.0), if lap_hovered { TEXT } else { theme::GOOD });
                            if let Some(lap) = lap {
                                if lap.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if lap.on_hover_text("Open the best lap").clicked() {
                                    self.pending_open = Some(lib.sessions[*i].path.clone());
                                }
                            }
                        }
                        if resp.hovered() && !lap_hovered {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let hint = if d.id.is_some() { "Edit this driver" } else { "Put this MyChron on a kart" };
                        if resp.on_hover_text(hint).clicked() && !lap_hovered {
                            match d.id {
                                Some(id) => edit_driver = profiles.drivers.iter().find(|p| p.id == id).map(|p| DriverEdit { driver: p.clone(), confirm_delete: false }),
                                None => edit_kart = Some(KartEdit::new(&profiles, None, &d.loggers[0])),
                            }
                        }
                    }
                    if drivers.is_empty() {
                        ui.label(RichText::new("No drivers yet. Add one, then add their kart.").color(SECONDARY));
                    }
                });
            });
            if add_driver {
                edit_driver = Some(DriverEdit { driver: Driver::default(), confirm_delete: false });
            }

            // Row 3: karts with their setup, the last race day, what's coming up.
            let third = (w - gap * 2.0) / 3.0;
            ui.horizontal_top(|ui| {
                let (_, add_kart) = panel(ui, vec2(third, row3), "Karts", None, Some((Icon::Plus, "Add kart")), |ui| {
                    egui::ScrollArea::vertical().id_salt("garage_karts").auto_shrink([false, false]).show(ui, |ui| {
                        for (n, row) in karts.iter().enumerate() {
                            let name = row.kart.as_ref().map(|k| k.name.clone()).unwrap_or_else(|| "Not on a kart yet".into());
                            let sub = match &row.kart {
                                Some(k) => [
                                    profiles.driver_name(k.driver).map(str::to_string),
                                    (!k.engine.trim().is_empty()).then(|| k.engine.clone()),
                                    Some(if k.logger.is_empty() { "no MyChron".to_string() } else { "MyChron on it".to_string() }),
                                ]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join("  ·  "),
                                None => format!("Logger {}", row.logger),
                            };
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    ui.label(RichText::new(&name).font(semibold(16.0)).color(TEXT));
                                    ui.label(RichText::new(sub).size(12.5).color(DIM));
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if small_button(ui, Icon::Pencil, "Edit").on_hover_text("Name, engine, driver, MyChron and baseline setup").clicked() {
                                        edit_kart = Some(KartEdit::new(&profiles, row.kart.as_ref(), &row.logger));
                                    }
                                    if let Some(newest) = row.newest {
                                        if small_button(ui, Icon::Settings, "Setup").on_hover_text("Change the setup from the newest session on").clicked() {
                                            edit_setup = Some(newest);
                                        }
                                    }
                                });
                            });
                            ui.add_space(6.0);
                            let s = match (row.newest, &row.kart) {
                                (Some(i), _) => resolved[i].setup.clone(),
                                (None, Some(k)) => k.baseline.clone(),
                                (None, None) => Setup::default(),
                            };
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
                            egui::Grid::new(("kart_setup", n)).num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
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
                            let when = if row.newest.is_some() { setup_last_changed(&lib, &resolved, &row.logger) } else { "Baseline, before its first session".to_string() };
                            ui.label(RichText::new(when).size(12.5).color(DIM));
                            ui.add_space(14.0);
                        }
                        if karts.is_empty() {
                            ui.label(RichText::new("No karts yet. Add one with its engine and the setup it runs.").color(SECONDARY));
                        }
                    });
                });
                if add_kart {
                    if profiles.drivers.is_empty() {
                        // A kart needs a driver: add one first, then their kart from there.
                        edit_driver = Some(DriverEdit { driver: Driver::default(), confirm_delete: false });
                    } else {
                        edit_kart = Some(KartEdit::new(&profiles, None, ""));
                    }
                }

                let last = all_events.iter().find(|e| !e.sessions.is_empty());
                let date_pill = last.map(|e| short_date(e.start));
                let (resp, _) = panel(ui, vec2(third, row3), "Last race day", date_pill.as_deref().map(|d| (d, SECONDARY)), None, |ui| {
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

                panel(ui, vec2(third, row3), "Coming up", None, None, |ui| {
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
        if edit_driver.is_some() {
            self.driver_edit = edit_driver;
        }
        if edit_kart.is_some() {
            self.kart_edit = edit_kart;
        }
        self.library = Some(lib);
        self.profiles = Some(profiles);
        self.setup_baselines = Some(baselines);
        self.events = Some(defs);
        self.setup_dialog(ui.ctx());
        self.driver_dialog(ui.ctx());
        self.kart_dialog(ui.ctx());
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

    /// Adding or editing a driver: name, class, birthday, weight, and their karts.
    fn driver_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.driver_edit.take() else { return };
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
        let existing = edit.driver.id != 0;
        let name = edit.driver.name.trim().to_string();
        let taken = profiles.drivers.iter().any(|d| d.id != edit.driver.id && d.name.eq_ignore_ascii_case(&name));
        let karts: Vec<Kart> = profiles.karts.iter().filter(|k| existing && k.driver == Some(edit.driver.id)).cloned().collect();
        let mut classes: Vec<String> = profiles.drivers.iter().map(|d| d.class.trim().to_string()).chain(CLASSES.iter().map(|c| c.to_string())).filter(|c| !c.is_empty()).collect();
        let mut seen = std::collections::HashSet::new();
        classes.retain(|c| seen.insert(c.to_lowercase()));
        let today = events::today();
        let (mut open, mut save, mut cancel, mut delete) = (true, false, false, false);
        let mut open_kart: Option<KartEdit> = None;
        let label = |ui: &mut Ui, t: &str| ui.label(RichText::new(t).font(medium(15.0)).color(SECONDARY));
        let field_w = 360.0;
        egui::Window::new(RichText::new(if existing { "Driver" } else { "Add driver" }).font(semibold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(500.0);
                egui::Grid::new("driver_edit").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                    label(ui, "Name");
                    let field = ui.add(egui::TextEdit::singleline(&mut edit.driver.name).font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_width(field_w).hint_text("e.g. Alex"));
                    if !existing && edit.driver.name.is_empty() && !field.has_focus() && ui.memory(|m| m.focused().is_none()) {
                        field.request_focus();
                    }
                    ui.end_row();
                    label(ui, "Class");
                    suggest_field(ui, "driver_class", &mut edit.driver.class, "e.g. Mini Swift", &classes, field_w);
                    ui.end_row();
                    label(ui, "Birthday");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut edit.driver.born).font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_width(170.0).hint_text("3/14/2017"));
                        let note = match (edit.driver.born.trim().is_empty(), edit.driver.age(today)) {
                            (true, _) => String::new(),
                            (false, Some(a)) => format!("age {a}"),
                            (false, None) => "month/day/year".to_string(),
                        };
                        ui.label(RichText::new(note).color(DIM));
                    });
                    ui.end_row();
                    label(ui, "Weight");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut edit.driver.weight_lb).font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_width(90.0).hint_text("75"));
                        ui.label(RichText::new("lb in race gear").color(DIM));
                    });
                    ui.end_row();
                });
                if taken {
                    ui.label(RichText::new("There's already a driver with that name.").size(12.5).color(theme::FAVORITE));
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Karts").font(semibold(15.0)).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Adds the driver first if they're new.
                        if small_button(ui, Icon::Plus, "Add kart").clicked() {
                            open_kart = Some(KartEdit::for_driver(profiles, None, "", Some(edit.driver.id)));
                        }
                    });
                });
                if karts.is_empty() {
                    let text = if name.is_empty() { "Name them, then add their kart." } else { "None yet." };
                    ui.label(RichText::new(text).size(12.5).color(DIM));
                }
                for k in &karts {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            // Leave room for Edit.
                            ui.set_max_width(ui.available_width() - 90.0);
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(&k.name).font(medium(15.0)).color(TEXT));
                            let sub = [k.engine.trim().to_string(), if k.logger.is_empty() { "no MyChron".into() } else { "MyChron on it".into() }]
                                .into_iter()
                                .filter(|x| !x.is_empty())
                                .collect::<Vec<_>>()
                                .join("  ·  ");
                            ui.label(RichText::new(sub).size(12.5).color(DIM));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if small_button(ui, Icon::Pencil, "Edit").clicked() {
                                open_kart = Some(KartEdit::new(profiles, Some(k), &k.logger));
                            }
                        });
                    });
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if existing {
                        let label = if edit.confirm_delete { "Yes, delete" } else { "Delete" };
                        if button(ui, None, label, false).clicked() {
                            if edit.confirm_delete { delete = true } else { edit.confirm_delete = true }
                        }
                        if edit.confirm_delete {
                            ui.label(RichText::new("Their karts and sessions stay.").size(12.5).color(DIM));
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, Some(Icon::Check), if existing { "Save" } else { "Add" }, true).clicked() {
                            save = true;
                        }
                        if button(ui, None, "Cancel", false).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        let ok = !name.is_empty() && !taken;
        let mut d = edit.driver.clone();
        for f in [&mut d.name, &mut d.class, &mut d.born, &mut d.weight_lb] {
            *f = f.trim().to_string();
        }
        let changed = if delete {
            profiles.drivers.retain(|x| x.id != d.id);
            for k in profiles.karts.iter_mut().filter(|k| k.driver == Some(d.id)) {
                k.driver = None;
            }
            true
        } else if (save || open_kart.is_some()) && ok {
            // Going to a kart saves the driver first.
            if !existing {
                d.id = profiles.drivers.iter().map(|x| x.id).max().unwrap_or(0) + 1;
                if let Some(k) = open_kart.as_mut() {
                    k.driver = Some(d.id);
                }
            }
            match profiles.drivers.iter_mut().find(|x| x.id == d.id) {
                Some(x) => *x = d,
                None => profiles.drivers.push(d),
            }
            true
        } else {
            false
        };
        if changed {
            log::info!("driver {}: {name}", if delete { "deleted" } else if existing { "saved" } else { "added" });
            if let Err(e) = profiles.save() {
                self.error = Some(format!("Couldn't save the driver: {e}"));
            }
            if !delete {
                self.kart_edit = open_kart;
            }
        } else if open && !cancel {
            self.driver_edit = Some(edit);
        }
    }

    /// Adding or editing a kart: name, engine, usual driver, the MyChron on it
    /// and its baseline setup.
    fn kart_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.kart_edit.take() else { return };
        let tyres = self.tyre_suggestions();
        // MyChrons known here: from sessions, karts and the one connected.
        let mut loggers: Vec<String> = self.library.as_ref().map(|l| l.sessions.iter().map(|s| s.logger.clone()).collect()).unwrap_or_default();
        loggers.extend(self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()));
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
        loggers.extend(profiles.karts.iter().map(|k| k.logger.clone()));
        loggers.retain(|l| !l.is_empty());
        loggers.sort();
        loggers.dedup();
        let name = edit.name.trim().to_string();
        let taken = profiles.karts.iter().any(|k| Some(k.id) != edit.id && k.name.eq_ignore_ascii_case(&name));
        let (mut open, mut save, mut cancel, mut delete) = (true, false, false, false);
        let label = |ui: &mut Ui, t: &str| ui.label(RichText::new(t).font(medium(15.0)).color(SECONDARY));
        let field_w = 400.0;
        egui::Window::new(RichText::new(if edit.id.is_some() { "Kart" } else { "Add kart" }).font(semibold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(560.0);
                egui::Grid::new("kart_edit").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                    label(ui, "Name");
                    let field = ui.add(egui::TextEdit::singleline(&mut edit.name).font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_width(field_w).hint_text("e.g. Mini Swift Cadet"));
                    if edit.id.is_none() && edit.name.is_empty() && !field.has_focus() && ui.memory(|m| m.focused().is_none()) {
                        field.request_focus();
                    }
                    ui.end_row();
                    label(ui, "Engine");
                    ui.vertical(|ui| {
                        let shown = if edit.other_engine { "Other engine".to_string() } else if edit.engine.is_empty() { "Choose an engine".to_string() } else { edit.engine.clone() };
                        egui::ComboBox::from_id_salt("kart_engine")
                            .selected_text(RichText::new(shown).font(regular(15.0)).color(if edit.engine.is_empty() && !edit.other_engine { DIM } else { TEXT }))
                            .width(field_w)
                            .height(420.0)
                            .show_ui(ui, |ui| {
                                for (maker, engines) in crate::profiles::ENGINES {
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(maker.to_uppercase()).font(semibold(11.0)).color(DIM).extra_letter_spacing(0.6));
                                    for e in engines {
                                        if ui.selectable_label(!edit.other_engine && edit.engine.eq_ignore_ascii_case(e), RichText::new(*e).font(regular(14.5))).clicked() {
                                            edit.engine = e.to_string();
                                            edit.other_engine = false;
                                        }
                                    }
                                }
                                ui.separator();
                                if ui.selectable_label(edit.other_engine, RichText::new("Other engine").font(regular(14.5))).clicked() {
                                    edit.other_engine = true;
                                    edit.engine.clear();
                                }
                            });
                        if edit.other_engine {
                            ui.add(egui::TextEdit::singleline(&mut edit.engine).font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_width(field_w).hint_text("Maker and model, e.g. TM KZ-R3"));
                        }
                    });
                    ui.end_row();
                    label(ui, "Driver");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        for d in &profiles.drivers {
                            if chip(ui, &d.name, edit.driver == Some(d.id)).clicked() {
                                edit.driver = Some(d.id);
                            }
                        }
                    });
                    ui.end_row();
                    label(ui, "MyChron");
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                            for l in &loggers {
                                if chip(ui, &logger_label(profiles, l, edit.id), &edit.logger == l).clicked() {
                                    edit.logger = l.clone();
                                }
                            }
                            if chip(ui, "None", edit.logger.is_empty()).clicked() {
                                edit.logger.clear();
                            }
                        });
                        let hint = match profiles.kart_on(&edit.logger) {
                            Some(k) if Some(k.id) != edit.id => format!("Saving moves this MyChron from {} to this kart.", k.name),
                            _ if loggers.is_empty() => "A MyChron shows up here once it's connected or its sessions are downloaded.".to_string(),
                            _ => "Sessions from this MyChron belong to this kart.".to_string(),
                        };
                        ui.label(RichText::new(hint).size(12.5).color(DIM));
                    });
                    ui.end_row();
                });
                if taken {
                    ui.label(RichText::new("There's already a kart with that name.").size(12.5).color(theme::FAVORITE));
                }
                if edit.driver.is_none() {
                    ui.label(RichText::new("Pick its driver to save it.").size(12.5).color(theme::FAVORITE));
                }
                ui.add_space(14.0);
                ui.label(RichText::new("Baseline setup").font(semibold(15.0)).color(TEXT));
                ui.label(RichText::new("What it runs before its first setup sheet. Later changes go on the session they were made for.").size(12.5).color(SECONDARY));
                ui.add_space(8.0);
                setup_ui::setup_fields(ui, "kart_baseline", &mut edit.baseline, &tyres, field_w, false);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if edit.id.is_some() {
                        let label = if edit.confirm_delete { "Yes, delete" } else { "Delete" };
                        if button(ui, None, label, false).clicked() {
                            if edit.confirm_delete { delete = true } else { edit.confirm_delete = true }
                        }
                        if edit.confirm_delete {
                            ui.label(RichText::new("Its sessions and setup sheets stay.").size(12.5).color(DIM));
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, Some(Icon::Check), if edit.id.is_some() { "Save" } else { "Add" }, true).clicked() {
                            save = true;
                        }
                        if button(ui, None, "Cancel", false).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        let ok = !name.is_empty() && !taken && edit.driver.is_some();
        let changed = if delete {
            let id = edit.id.unwrap_or_default();
            profiles.karts.retain(|k| k.id != id);
            true
        } else if save && ok {
            // A MyChron is on one kart at a time.
            if !edit.logger.is_empty() {
                for k in profiles.karts.iter_mut().filter(|k| k.logger == edit.logger && Some(k.id) != edit.id) {
                    k.logger.clear();
                }
            }
            let id = edit.id.unwrap_or_else(|| profiles.next_kart_id());
            let kart = Kart { id, name: name.clone(), engine: edit.engine.trim().to_string(), driver: edit.driver, logger: edit.logger.clone(), baseline: edit.baseline.carried() };
            match profiles.karts.iter_mut().find(|k| k.id == id) {
                Some(k) => *k = kart,
                None => profiles.karts.push(kart),
            }
            true
        } else {
            false
        };
        if changed {
            log::info!("kart {}: {name}", if delete { "deleted" } else if edit.id.is_some() { "saved" } else { "added" });
            if let Err(e) = profiles.save() {
                self.error = Some(format!("Couldn't save the kart: {e}"));
            }
            // Baselines come from the karts.
            self.setup_baselines = None;
        } else if open && !cancel {
            self.kart_edit = Some(edit);
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
