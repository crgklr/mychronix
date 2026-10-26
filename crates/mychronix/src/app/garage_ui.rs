//! The Garage, Mychronix's home: the MyChrons, the drivers with their seat
//! time, the karts with the setup they're running, the last race day and
//! what's coming up; adding and editing drivers and karts. And the session
//! picker Analysis shows before a session is open.

use super::*;
use crate::events;
use crate::profiles::{Driver, Kart, Profiles};
use crate::setup::{self, Baselines, Setup};

/// The Garage's state: the driver, kart and MyChron dialogs, a removal
/// waiting for "Are you sure?", and which page of drivers shows.
#[derive(Default)]
pub(super) struct GarageState {
    /// The Garage's driver and kart dialogs.
    pub(super) driver_edit: Option<garage_ui::DriverEdit>,
    pub(super) kart_edit: Option<garage_ui::KartEdit>,
    /// Setting up a MyChron that's new to the Garage.
    pub(super) mychron_setup: Option<garage_manage_ui::MyChronSetup>,
    /// Deleting a driver, or removing a kart or MyChron, waiting for "Are you sure?".
    pub(super) confirm: Option<garage_manage_ui::Confirm>,
    /// MyChrons put off with "Not now" while connected (asked again once reconnected).
    pub(super) snoozed_while_on: std::collections::BTreeSet<String>,
    /// Which page of drivers the Garage shows (two drivers to a page).
    pub(super) page: usize,
}

/// One driver and their numbers, from on-track sessions (warm-ups left out)
/// on the MyChrons of their karts.
struct DriverRow {
    /// None for a MyChron that isn't on a driver's kart.
    id: Option<u32>,
    name: String,
    engine: String,
    /// Their sessions: recorded on their karts (see `LocalSession::kart`).
    sessions: Vec<usize>,
    seat_ms: u64,
    laps: u32,
    days: Vec<aim_xrk::summary::Date>,
    /// Seat time per race day, oldest first: (date, ms).
    per_day: Vec<(aim_xrk::summary::Date, u64)>,
    /// Best lap at the track raced most: (track, lap ms, session index).
    best: Option<(String, u64, usize)>,
}

fn driver_rows(lib: &Library, profiles: &Profiles) -> Vec<DriverRow> {
    // Each driver's numbers come from every MyChron their karts have had
    // (removed ones too); a MyChron with no home is set up from its own card.
    // (driver id, name, their karts in the Garage, their sessions)
    type Group<'a> = (Option<u32>, String, Vec<&'a Kart>, Vec<usize>);
    let groups: Vec<Group> = profiles
        .drivers
        .iter()
        .map(|d| {
            let karts: Vec<&Kart> = profiles.garage_karts().filter(|k| k.driver == Some(d.id)).collect();
            let theirs: Vec<u32> = profiles.karts.iter().filter(|k| k.driver == Some(d.id)).map(|k| k.id).collect();
            let sessions = (0..lib.sessions.len()).filter(|&i| lib.sessions[i].kart.is_some_and(|k| theirs.contains(&k))).collect();
            (Some(d.id), d.name.clone(), karts, sessions)
        })
        .collect();
    groups
        .into_iter()
        .map(|(id, name, karts, sessions)| {
            // Their engine (for the picture): a kart in the Garage, else one they had.
            let theirs = |k: &&Kart| id.is_some() && k.driver == id;
            let engine = karts.iter().chain(profiles.karts.iter().filter(theirs).collect::<Vec<_>>().iter()).map(|k| k.engine.clone()).find(|e| !e.trim().is_empty()).unwrap_or_default();
            let on: Vec<usize> = sessions.iter().copied().filter(|&i| !events::is_warm_up(&lib.sessions[i].summary)).collect();
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
            // The library's track (a chosen one wins over the logger's name).
            let track_of = |i: usize| lib.sessions[i].track_name(&lib.tracks);
            let mut counts: Vec<(String, usize)> = Vec::new();
            for &i in &on {
                let t = track_of(i).unwrap_or_default();
                match counts.iter_mut().find(|c| c.0 == t) {
                    Some(c) => c.1 += 1,
                    None => counts.push((t, 1)),
                }
            }
            let best = counts
                .iter()
                .filter(|(t, _)| on.iter().any(|&i| track_of(i).as_deref() == Some(t) && lib.sessions[i].summary.best_lap_ms.is_some()))
                .max_by_key(|c| c.1)
                .and_then(|(t, _)| {
                    on.iter()
                        .filter(|&&i| track_of(i).as_deref() == Some(t))
                        .filter_map(|&i| lib.sessions[i].summary.best_lap_ms.map(|b| (t.clone(), b, i)))
                        .min_by_key(|x| x.1)
                });
            DriverRow { id, name, engine, sessions, seat_ms, laps, days, per_day, best }
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
    // Its newest session, from whichever MyChron it had then.
    let newest = |k: &Kart| {
        (0..lib.sessions.len()).filter(|&i| lib.sessions[i].kart == Some(k.id)).max_by_key(|&i| {
            let s = &lib.sessions[i].summary;
            (s.date, s.start_time.clone(), s.number)
        })
    };
    profiles.garage_karts().map(|k| KartRow { kart: Some(k.clone()), logger: k.logger.clone(), newest: newest(k) }).collect()
}

/// A driver being added or edited (`driver.id` is 0 for a new one).
pub(super) struct DriverEdit {
    driver: Driver,
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
    clutch_rpm: String,
    /// Typing an engine that isn't in the list.
    other_engine: bool,
    /// Which part of the form shows: the kart, its MyChron, its baseline setup.
    step: usize,
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
                clutch_rpm: k.clutch_rpm.clone(),
                other_engine: !k.engine.trim().is_empty() && !crate::profiles::is_known_engine(&k.engine),
                step: 0,
            },
            None => KartEdit {
                id: None,
                name: String::new(),
                engine: String::new(),
                // A new kart for the only driver is theirs.
                driver: driver.or((profiles.drivers.len() == 1).then(|| profiles.drivers[0].id)),
                logger: logger.to_string(),
                baseline: Setup::default(),
                clutch_rpm: String::new(),
                other_engine: false,
                step: 0,
            },
        }
    }
}

fn short_date(d: aim_xrk::summary::Date) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}", M[(d.month.clamp(1, 12) - 1) as usize], d.day)
}

fn hours(ms: u64) -> String {
    let m = ms / 60_000;
    if m >= 60 { format!("{}h {:02}m", m / 60, m % 60) } else { format!("{m}m") }
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
        self.key_poll(&ui.ctx().clone());
        // Nothing here yet: the welcome, with the two ways to start.
        if self.first_run() {
            self.welcome_page(ui);
            return;
        }
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.take().unwrap_or_else(Profiles::load);
        let baselines = self.setup_baselines.take().unwrap_or_else(Baselines::load);
        let defs = self.events_page.events.take().unwrap_or_else(crate::events::Events::load);
        let drivers = driver_rows(&lib, &profiles);
        let karts = kart_rows(&lib, &profiles);
        let resolved = setup::resolve(&lib, &baselines);
        let all_events = events::group(&lib, &profiles, &defs);
        let today = events::today();
        let connected_logger = self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string());
        let new_count = self.new_on_logger().len();
        // MyChron models come from session digests (and the WiFi name once connected).
        let kart_sessions: Vec<PathBuf> = lib.sessions.iter().map(|s| s.path.clone()).collect();
        self.want_digests(kart_sessions.into_iter(), &ui.ctx().clone());
        let digests = self.sensors.digests.take().unwrap_or_default();
        // A MyChron's model: from its WiFi name, else its session summaries
        // (a MyChron 4's files carry its model code), else their digests.
        let model_of = |logger: &str| -> String {
            Some(profiles.for_logger(logger).model)
                .filter(|m| !m.is_empty())
                .or_else(|| lib.sessions.iter().filter(|s| s.logger == logger).find_map(|s| s.summary.model.clone()))
                .or_else(|| digests.model(logger))
                .unwrap_or_else(|| "MyChron".into())
        };
        self.key_poll(&ui.ctx().clone());
        let key = self.key.listing.as_ref().filter(|_| self.key.present && self.key.busy.is_none()).map(|(s, _, n)| (s.clone(), n.len()));
        let status = GarageStatus { connected: connected_logger.clone(), key, reading: self.device.files.is_empty(), new_count };
        let mut act = Actions::default();

        // Header.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Garage").font(semibold(28.0)).color(TEXT));
                let (n, k) = (profiles.drivers.len(), profiles.garage_karts().count());
                let days: usize = {
                    let mut d: Vec<_> = drivers.iter().flat_map(|d| d.days.clone()).collect();
                    d.sort();
                    d.dedup();
                    d.len()
                };
                ui.label(RichText::new(format!("{n} driver{}  ·  {k} kart{}  ·  {days} race day{} this season", if n == 1 { "" } else { "s" }, if k == 1 { "" } else { "s" }, if days == 1 { "" } else { "s" })).font(regular(14.5)).color(SECONDARY));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if connected_logger.is_some() && new_count > 0 && button(ui, Some(Icon::FolderOpen), &format!("Download {new_count} new session{}", if new_count == 1 { "" } else { "s" }), true).clicked() {
                    act.go = Some(Page::Device);
                }
                if button(ui, Some(Icon::Plus), "Add driver", false).clicked() {
                    act.edit_driver = Some(DriverEdit { driver: Driver::default() });
                }
            });
        });
        ui.add_space(18.0);

        self.import_banner(ui);

        // MyChrons that need a home: whose they are and which kart.
        for u in self.unplaced_mychrons(&lib, &profiles) {
            self.unplaced_card(ui, &u, &model_of(&u.logger));
            ui.add_space(12.0);
        }

        if drivers.is_empty() {
            ui.label(RichText::new("No drivers yet. Add one, then add their kart and MyChron.").color(SECONDARY));
        }

        // One driver fills the width; two split it in half; with more, a
        // carousel of pages of two, each column as wide as in the split.
        let gap = 28.0;
        let pages = drivers.len().div_ceil(2).max(1);
        let page = self.garage.page.min(pages - 1);
        let footer_h = if pages > 1 { 40.0 } else { 0.0 };
        let col_w = if drivers.len() == 1 { ui.available_width() } else { (ui.available_width() - gap) / 2.0 };
        let col_h = ui.available_height() - footer_h;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (k, d) in drivers.iter().enumerate().skip(page * 2).take(2) {
                ui.allocate_ui_with_layout(vec2(col_w, col_h), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(col_w);
                    egui::ScrollArea::vertical().id_salt(("garage_col", k)).auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_width(col_w - 12.0);
                        driver_column(ui, k, d, &lib, &profiles, &karts, &resolved, &all_events, today, &status, &model_of, &mut act);
                    });
                });
            }
        });
        if pages > 1 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let names: Vec<String> = drivers.chunks(2).map(|c| c.iter().map(|d| d.name.clone()).collect::<Vec<_>>().join(" & ")).collect();
                if let Some(p) = carousel_nav(ui, page, &names) {
                    self.garage.page = p;
                }
            });
        }

        if let Some(i) = act.edit_setup {
            let path = lib.sessions[i].path.clone();
            self.setup_edit = Some(setup_ui::SetupEdit {
                path,
                setup: resolved[i].setup.clone(),
                before: setup::before(&lib, &baselines, i),
                has_own: resolved[i].source == setup::Source::Own,
            });
        }
        if act.edit_driver.is_some() {
            self.garage.driver_edit = act.edit_driver.take();
        }
        if act.edit_kart.is_some() {
            self.garage.kart_edit = act.edit_kart.take();
        }
        if act.download_key {
            self.key_download(&ui.ctx().clone());
        }
        if let Some(p) = act.open_lap.take() {
            self.pending_open = Some(p);
        }
        self.sensors.digests = Some(digests);
        self.library = Some(lib);
        self.profiles = Some(profiles);
        self.setup_baselines = Some(baselines);
        self.events_page.events = Some(defs);
        self.setup_dialog(ui.ctx());
        if let Some(c) = act.confirm.take() {
            self.garage.confirm = Some(c);
        }
        self.driver_dialog(ui.ctx());
        self.kart_dialog(ui.ctx());
        self.mychron_setup_dialog(ui.ctx());
        self.garage_confirm_dialog(ui.ctx());
        if let Some(key) = act.open_event {
            self.events_page.sel = Some(key);
            self.page = Page::Events;
        }
        if let Some(p) = act.go {
            self.page = p;
        }
        if let Some(path) = self.pending_open.take() {
            self.load(path);
        }
    }

    /// Adding or editing a driver: name, class, birthday, weight, and their karts.
    fn driver_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.garage.driver_edit.take() else { return };
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
        let existing = edit.driver.id != 0;
        let name = edit.driver.name.trim().to_string();
        let taken = profiles.drivers.iter().any(|d| d.id != edit.driver.id && d.name.eq_ignore_ascii_case(&name));
        let karts: Vec<Kart> = profiles.garage_karts().filter(|k| existing && k.driver == Some(edit.driver.id)).cloned().collect();
        let mut classes: Vec<String> = profiles.drivers.iter().map(|d| d.class.trim().to_string()).chain(CLASSES.iter().map(|c| c.to_string())).filter(|c| !c.is_empty()).collect();
        let mut seen = std::collections::HashSet::new();
        classes.retain(|c| seen.insert(c.to_lowercase()));
        let today = events::today();
        let (mut save, mut cancel, mut delete) = (false, false, false);
        let mut open_kart: Option<KartEdit> = None;
        let label = garage_manage_ui::look::label;
        let field_w = 460.0;
        let closed = garage_manage_ui::look::dialog(ctx, "driver_edit", 720.0, |ui| {
            let (title, sub) = if existing { ("Driver", "Their details, and the karts they race.") } else { ("Add driver", "Who's racing? Add their kart next.") };
            garage_manage_ui::look::header(ui, Icon::User, SECONDARY, None, title, sub);
            {
                garage_manage_ui::look::body(ui, |ui| {
                    egui::Grid::new("driver_edit").num_columns(2).spacing([20.0, 16.0]).min_col_width(90.0).show(ui, |ui| {
                        label(ui, "Name");
                        let field = garage_manage_ui::look::text(ui, &mut edit.driver.name, "e.g. Alex", Some(field_w));
                        if !existing && edit.driver.name.is_empty() && !field.has_focus() && ui.memory(|m| m.focused().is_none()) {
                            field.request_focus();
                        }
                        ui.end_row();
                        label(ui, "Class");
                        suggest_field(ui, "driver_class", &mut edit.driver.class, "e.g. Mini Swift", &classes, field_w);
                        ui.end_row();
                        label(ui, "Birthday");
                        ui.horizontal(|ui| {
                            garage_manage_ui::look::text(ui, &mut edit.driver.born, "3/14/2017", Some(180.0));
                            let note = match (edit.driver.born.trim().is_empty(), edit.driver.age(today)) {
                                (true, _) => String::new(),
                                (false, Some(a)) => format!("age {a}"),
                                (false, None) => "month/day/year".to_string(),
                            };
                            ui.label(RichText::new(note).font(regular(14.5)).color(DIM));
                        });
                        ui.end_row();
                        label(ui, "Weight");
                        ui.horizontal(|ui| {
                            garage_manage_ui::look::text(ui, &mut edit.driver.weight_lb, "75", Some(110.0));
                            ui.label(RichText::new("lb in race gear").font(regular(14.5)).color(DIM));
                        });
                        ui.end_row();
                    });
                    if taken {
                        ui.label(RichText::new("There's already a driver with that name.").size(12.5).color(theme::FAVORITE));
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Karts").font(semibold(17.0)).color(TEXT));
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
                });
            }
            garage_manage_ui::look::footer_row(ui, |ui| {
                use garage_manage_ui::look::{Kind, big_button};
                if existing && big_button(ui, "Delete driver…", Some(Icon::Trash), Kind::Quiet, true) {
                    delete = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if big_button(ui, if existing { "Save" } else { "Add" }, Some(Icon::Check), Kind::Primary, true) {
                        save = true;
                    }
                    if big_button(ui, "Cancel", None, Kind::Quiet, true) {
                        cancel = true;
                    }
                });
            });
        });
        let open = !closed;
        let ok = !name.is_empty() && !taken;
        let mut d = edit.driver.clone();
        for f in [&mut d.name, &mut d.class, &mut d.born, &mut d.weight_lb] {
            *f = f.trim().to_string();
        }
        if delete {
            // Deleting asks twice, in its own window.
            self.garage.confirm = Some(garage_manage_ui::Confirm::DeleteDriver { id: d.id, sure: false, backed_up: None });
            return;
        }
        let changed = if (save || open_kart.is_some()) && ok {
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
            log::info!("driver {}: {name}", if existing { "saved" } else { "added" });
            if let Err(e) = profiles.save() {
                self.error = Some(format!("Couldn't save the driver: {e}"));
            }
            self.garage.kart_edit = open_kart;
        } else if open && !cancel {
            self.garage.driver_edit = Some(edit);
        }
    }

    /// Adding or editing a kart: name, engine, usual driver, the MyChron on it
    /// and its baseline setup.
    fn kart_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.garage.kart_edit.take() else { return };
        let tires = self.tire_suggestions();
        let models = self.mychron_models();
        // MyChrons known here: from sessions, karts and the one connected.
        let mut loggers: Vec<String> = self.library.as_ref().map(|l| l.sessions.iter().map(|s| s.logger.clone()).collect()).unwrap_or_default();
        loggers.extend(self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()));
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
        loggers.extend(profiles.karts.iter().map(|k| k.logger.clone()));
        loggers.retain(|l| !l.is_empty());
        loggers.sort();
        loggers.dedup();
        let name = edit.name.trim().to_string();
        let taken = profiles.karts.iter().any(|k| Some(k.id) != edit.id && k.name.eq_ignore_ascii_case(&name));
        let (mut save, mut cancel, mut delete) = (false, false, false);
        let adding = edit.id.is_none();
        let ready = !name.is_empty() && !taken && edit.driver.is_some();
        const STEPS: [&str; 3] = ["The kart", "Its MyChron", "Baseline setup"];
        let closed = garage_manage_ui::look::dialog(ctx, "kart_edit", 760.0, |ui| {
            use garage_manage_ui::look::{self, Kind, big_button};
            let (title, sub) = match (adding, edit.step) {
                (_, 0) => (if adding { "Add a kart".to_string() } else { name.clone() }, "Its name, engine and driver."),
                (_, 1) => ("Its MyChron".to_string(), "The MyChron mounted on it. Its sessions belong to this kart."),
                _ => ("Baseline setup".to_string(), "What it runs before its first setup sheet. Later changes go on the session they were made for."),
            };
            look::header(ui, Icon::Flag, SECONDARY, None, &title, sub);
            if let Some(k) = look::steps(ui, &STEPS, edit.step, !adding) {
                edit.step = k;
            }
            look::body(ui, |ui| match edit.step {
                0 => {
                    egui::Grid::new("kart_edit").num_columns(2).spacing([20.0, 16.0]).min_col_width(90.0).show(ui, |ui| {
                        look::label(ui, "Name");
                        let field = look::text(ui, &mut edit.name, "e.g. Mini Swift Cadet", Some(480.0));
                        if adding && edit.name.is_empty() && !field.has_focus() && ui.memory(|m| m.focused().is_none()) {
                            field.request_focus();
                        }
                        ui.end_row();
                        look::label(ui, "Engine");
                        ui.vertical(|ui| {
                            let shown = if edit.other_engine { "Other engine".to_string() } else if edit.engine.is_empty() { "Choose an engine".to_string() } else { edit.engine.clone() };
                            egui::ComboBox::from_id_salt("kart_engine")
                                .selected_text(RichText::new(shown).font(regular(16.5)).color(if edit.engine.is_empty() && !edit.other_engine { DIM } else { TEXT }))
                                .width(480.0)
                                .height(460.0)
                                .show_ui(ui, |ui| {
                                    for (maker, engines) in crate::profiles::ENGINES {
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(maker.to_uppercase()).font(semibold(11.5)).color(DIM).extra_letter_spacing(0.6));
                                        for e in engines {
                                            if ui.selectable_label(!edit.other_engine && edit.engine.eq_ignore_ascii_case(e), RichText::new(*e).font(regular(16.0))).clicked() {
                                                edit.engine = e.to_string();
                                                edit.other_engine = false;
                                            }
                                        }
                                    }
                                    ui.separator();
                                    if ui.selectable_label(edit.other_engine, RichText::new("Other engine").font(regular(16.0))).clicked() {
                                        edit.other_engine = true;
                                        edit.engine.clear();
                                    }
                                });
                            if edit.other_engine {
                                look::text(ui, &mut edit.engine, "Maker and model, e.g. TM KZ-R3", Some(480.0));
                            }
                        });
                        ui.end_row();
                        look::label(ui, "Clutch");
                        ui.horizontal(|ui| {
                            let typical = crate::setup::typical_clutch_rpm(&edit.engine);
                            look::text(ui, &mut edit.clutch_rpm, &format!("{typical:.0}"), Some(110.0));
                            ui.label(RichText::new("rpm when it engages (blank: the engine's typical)").font(regular(14.5)).color(DIM));
                        });
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    look::label(ui, "Driver");
                    for d in &profiles.drivers {
                        if look::option(ui, edit.driver == Some(d.id), None, &d.name, d.class.trim()) {
                            edit.driver = Some(d.id);
                        }
                    }
                    if taken {
                        look::note(ui, "There's already a kart with that name.", theme::FAVORITE);
                    }
                }
                1 => {
                    for l in &loggers {
                        let model = models.get(l).cloned().unwrap_or_else(|| "MyChron".into());
                        let sub = match profiles.mounted_on(l) {
                            Some(k) if Some(k.id) != edit.id => format!("ID {l}  ·  On {} now: it moves to this kart", k.name),
                            _ => format!("ID {l}"),
                        };
                        if look::option(ui, &edit.logger == l, Some(Icon::Mychron), &model, &sub) {
                            edit.logger = l.clone();
                        }
                    }
                    if look::option(ui, edit.logger.is_empty(), None, "No MyChron", "Its sessions so far stay with it") {
                        edit.logger.clear();
                    }
                    if loggers.is_empty() {
                        look::note(ui, "A MyChron shows up here once it's connected or its sessions are downloaded.", SECONDARY);
                    }
                }
                _ => setup_ui::setup_fields(ui, "kart_baseline", &mut edit.baseline, &tires, 480.0, false),
            });
            look::footer_row(ui, |ui| {
                if adding {
                    let back = edit.step > 0;
                    if big_button(ui, if back { "Back" } else { "Cancel" }, back.then_some(Icon::ChevronLeft), Kind::Quiet, true) {
                        if back { edit.step -= 1 } else { cancel = true }
                    }
                } else if big_button(ui, "Remove from Garage…", Some(Icon::EyeOff), Kind::Quiet, true) {
                    delete = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if adding && edit.step < 2 {
                        // The kart's name and driver come first.
                        if big_button(ui, "Next", Some(Icon::ChevronRight), Kind::Primary, edit.step > 0 || ready) {
                            edit.step += 1;
                        }
                    } else if big_button(ui, if adding { "Add kart" } else { "Save" }, Some(Icon::Check), Kind::Primary, ready) {
                        save = true;
                    }
                    if !adding && big_button(ui, "Cancel", None, Kind::Quiet, true) {
                        cancel = true;
                    }
                });
            });
        });
        let open = !closed;
        let ok = !name.is_empty() && !taken && edit.driver.is_some();
        if delete {
            // Removing asks twice, in its own window.
            self.garage.confirm = edit.id.map(|id| garage_manage_ui::Confirm::RemoveKart { id, sure: false });
            return;
        }
        let changed = if save && ok {
            let id = edit.id.unwrap_or_else(|| profiles.next_kart_id());
            // Its MyChron history stays as it was; the MyChron is changed below.
            let before = profiles.karts.iter().find(|k| k.id == id).cloned().unwrap_or_default();
            // A new baseline is what the kart runs from now on: its sessions so far keep the old one.
            let newest_of_kart = self.library.get_or_insert_with(Library::load).sessions.iter().filter(|s| s.kart == Some(id)).map(|s| s.start()).max();
            let mut kart = Kart {
                id,
                name: name.clone(),
                engine: edit.engine.trim().to_string(),
                driver: edit.driver,
                clutch_rpm: edit.clutch_rpm.trim().to_string(),
                ..before
            };
            kart.change_baseline(edit.baseline.carried(), newest_of_kart);
            match profiles.karts.iter_mut().find(|k| k.id == id) {
                Some(k) => *k = kart,
                None => profiles.karts.push(kart),
            }
            // A MyChron is on one kart at a time; the ones it replaces keep their sessions so far.
            let lib = self.library.get_or_insert_with(Library::load);
            let newest = |l: &str| lib.newest_start(l);
            if edit.logger.is_empty() {
                profiles.unmount(id, &newest);
            } else {
                profiles.mount(&edit.logger, id, &newest);
            }
            true
        } else {
            false
        };
        if changed {
            log::info!("kart {}: {name}", if edit.id.is_some() { "saved" } else { "added" });
            if let Err(e) = profiles.save() {
                self.error = Some(format!("Couldn't save the kart: {e}"));
            }
            // Baselines and each session's kart come from the karts.
            self.setup_baselines = None;
            self.reload_library();
        } else if open && !cancel {
            self.garage.kart_edit = Some(edit);
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
                ui.label(RichText::new("Pick a session to analyze").font(semibold(26.0)).color(TEXT));
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
                    let mut tracks: Vec<String> = day.iter().filter_map(|&i| lib.sessions[i].track_name(&lib.tracks)).collect();
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

/// What the Garage's columns asked for this frame.
#[derive(Default)]
struct Actions {
    go: Option<Page>,
    open_event: Option<String>,
    open_lap: Option<PathBuf>,
    edit_setup: Option<usize>,
    edit_driver: Option<DriverEdit>,
    edit_kart: Option<KartEdit>,
    download_key: bool,
    confirm: Option<garage_manage_ui::Confirm>,
}

/// The connected MyChron, if any.
struct GarageStatus {
    connected: Option<String>,
    /// A MyChron 4 data key that's plugged in: its logger's serial and new runs.
    key: Option<(String, usize)>,
    /// Connected, but its session list hasn't arrived yet.
    reading: bool,
    new_count: usize,
}

/// A small uppercase section heading, with an optional button on the right (true when clicked).
fn heading(ui: &mut Ui, title: &str, action: Option<(Icon, &str)>) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.label(RichText::new(title.to_uppercase()).font(semibold(12.0)).color(DIM).extra_letter_spacing(0.8));
        if let Some((icon, label)) = action {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| clicked = small_button(ui, icon, label).clicked());
        }
    });
    clicked
}

/// ‹ dots › page picker; returns the page clicked.
fn carousel_nav(ui: &mut Ui, page: usize, labels: &[String]) -> Option<usize> {
    let mut to = None;
    let arrow = |ui: &mut Ui, icon: Icon, enabled: bool| {
        let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(13), SURFACE_HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        icon.image(if enabled { TEXT } else { DIM.gamma_multiply(0.5) }, 16.0).paint_at(ui, r.shrink(5.0));
        enabled && resp.clicked()
    };
    if arrow(ui, Icon::ChevronLeft, page > 0) {
        to = Some(page - 1);
    }
    for (k, label) in labels.iter().enumerate() {
        let (r, resp) = ui.allocate_exact_size(vec2(14.0, 26.0), Sense::click());
        let on = k == page;
        ui.painter().circle_filled(r.center(), if on { 4.0 } else { 3.0 }, if on { TEXT } else if resp.hovered() { SECONDARY } else { DIM });
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.on_hover_text(label).clicked() {
            to = Some(k);
        }
    }
    if arrow(ui, Icon::ChevronRight, page + 1 < labels.len()) {
        to = Some(page + 1);
    }
    ui.label(RichText::new(format!("{} of {}", page + 1, labels.len())).size(12.5).color(DIM));
    to
}

/// One driver's column: who, their MyChron, seat time, kart and last race day.
#[allow(clippy::too_many_arguments)]
fn driver_column(
    ui: &mut Ui,
    k: usize,
    d: &DriverRow,
    lib: &Library,
    profiles: &Profiles,
    karts: &[KartRow],
    resolved: &[setup::Resolved],
    all_events: &[events::Event],
    today: aim_xrk::summary::Date,
    status: &GarageStatus,
    model_of: &dyn Fn(&str) -> String,
    act: &mut Actions,
) {
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    driver_tile(ui, k, d, lib, profiles, today, act);
    ui.add_space(10.0);

    // MyChron.
    heading(ui, "MyChron", None);
    // The MyChrons on their karts now (older ones' sessions still count above and below).
    let mounted: Vec<String> = profiles.garage_karts().filter(|k| d.id.is_some() && k.driver == d.id && !k.logger.is_empty()).map(|k| k.logger.clone()).collect();
    for logger in &mounted {
        let on = status.connected.as_deref() == Some(logger.as_str());
        // A MyChron 4 has no WiFi: it downloads through its data key.
        let wifi = model_of(logger) != "MyChron 4";
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add(Icon::Mychron.image(if on { theme::GOOD } else { SECONDARY }, 34.0));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(model_of(logger)).font(semibold(16.0)).color(TEXT));
                let key_new = status.key.as_ref().filter(|(s, _)| s == logger).map(|k| k.1);
                let line = if !wifi { match key_new {
                    Some(0) => "Data key plugged in · everything downloaded".to_string(),
                    Some(n) => format!("Data key plugged in · {n} new run{} to download", if n == 1 { "" } else { "s" }),
                    None => "Downloads through its data key".to_string(),
                } } else { match (on, status.reading, status.new_count) {
                    (true, true, _) => "On its WiFi · reading its sessions…".to_string(),
                    (true, false, 0) => "On its WiFi · everything downloaded".to_string(),
                    (true, false, n) => format!("On its WiFi · {n} new session{} to download", if n == 1 { "" } else { "s" }),
                    (false, ..) => "Off or out of range".to_string(),
                } };
                ui.label(RichText::new(line).size(13.0).color(if (on && wifi) || key_new.is_some() { theme::GOOD } else { SECONDARY }));
                let mine: Vec<&tracks::LocalSession> = lib.sessions.iter().filter(|s| &s.logger == logger).collect();
                let newest = mine.iter().filter_map(|s| s.summary.date).max();
                ui.label(RichText::new(format!("{} sessions downloaded{}", mine.len(), newest.map(|n| format!("  ·  newest {}", short_date(n))).unwrap_or_default())).size(12.5).color(DIM));
            });
            // One row from the right edge: Remove, then Download (data key) or Connect (WiFi).
            let key_new = status.key.as_ref().filter(|(s, _)| s == logger).map(|k| k.1).filter(|&n| n > 0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if small_button(ui, Icon::X, "Remove").on_hover_text("Take it out of the Garage (its sessions stay)").clicked() {
                    act.confirm = Some(garage_manage_ui::Confirm::RemoveMyChron { logger: logger.clone(), sure: false });
                }
                if let Some(n) = key_new && button(ui, Some(Icon::FolderOpen), &format!("Download {n}"), true).on_hover_text("Download the new runs from the data key").clicked() {
                    act.download_key = true;
                }
                if wifi && button(ui, Some(Icon::Wifi), if on { "Open" } else { "Connect" }, false).clicked() {
                    act.go = Some(Page::Device);
                }
            });
        });
    }
    if mounted.is_empty() {
        ui.label(RichText::new("No MyChron yet. A new one shows up at the top of the Garage once it's connected or its sessions are downloaded.").size(13.0).color(SECONDARY));
    }
    ui.add_space(10.0);

    // Seat time.
    heading(ui, "Seat time this season", None);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.vertical(|ui| {
            ui.set_width(230.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(hours(d.seat_ms)).font(semibold(30.0)).color(TEXT));
            ui.label(RichText::new(format!("on track  ·  {} laps  ·  {} day{}", d.laps, d.days.len(), if d.days.len() == 1 { "" } else { "s" })).size(12.5).color(DIM));
        });
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
        if d.per_day.is_empty() {
            ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, "No sessions yet", regular(12.5), DIM);
        }
        let max = d.per_day.iter().map(|x| x.1).max().unwrap_or(1).max(1) as f32;
        let slot = (rect.width() / d.per_day.len().max(1) as f32).min(70.0);
        let label_h = 16.0;
        for (j, (date, ms)) in d.per_day.iter().enumerate() {
            let h = ((*ms as f32 / max) * (rect.height() - label_h - 16.0)).max(2.0);
            let x = rect.left() + j as f32 * slot;
            let bar = Rect::from_min_max(pos2(x + 5.0, rect.bottom() - label_h - h), pos2(x + slot - 5.0, rect.bottom() - label_h));
            ui.painter().rect_filled(bar, CornerRadius::same(4), SERIES[k % SERIES.len()].gamma_multiply(0.9));
            ui.painter().text(pos2(bar.center().x, bar.top() - 7.0), egui::Align2::CENTER_CENTER, format!("{}m", ms / 60_000), regular(11.0), SECONDARY);
            ui.painter().text(pos2(bar.center().x, rect.bottom() - 5.0), egui::Align2::CENTER_CENTER, short_date(*date), regular(11.0), DIM);
        }
    });
    ui.add_space(10.0);

    // Kart(s).
    let mine: Vec<(usize, &KartRow)> = karts
        .iter()
        .enumerate()
        .filter(|(_, r)| match (&r.kart, d.id) {
            (Some(kart), Some(id)) => kart.driver == Some(id),
            _ => false,
        })
        .collect();
    let add = d.id.map(|_| (Icon::Plus, "Add kart"));
    if heading(ui, if mine.len() > 1 { "Karts" } else { "Kart" }, add) {
        act.edit_kart = Some(KartEdit::for_driver(profiles, None, "", d.id));
    }
    for (n, row) in &mine {
        kart_card(ui, *n, row, lib, profiles, resolved, model_of, act);
        ui.add_space(8.0);
    }
    if mine.is_empty() {
        ui.label(RichText::new("No kart yet.").size(13.0).color(SECONDARY));
    }
    ui.add_space(10.0);

    // Last race day: the newest event with this driver's sessions.
    heading(ui, "Last race day", None);
    let last = all_events.iter().find(|e| e.sessions.iter().any(|i| d.sessions.contains(i)));
    match last {
        Some(e) => {
            let theirs: Vec<usize> = e.sessions.iter().copied().filter(|&i| d.sessions.contains(&i) && !events::is_warm_up(&lib.sessions[i].summary)).collect();
            let best = theirs.iter().filter_map(|&i| lib.sessions[i].summary.best_lap_ms).min();
            let r = row(ui, false, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(e.title()).font(semibold(15.5)).color(TEXT));
                    ui.label(RichText::new(format!("{}  ·  {} session{} on track", long_date(e.start), theirs.len(), if theirs.len() == 1 { "" } else { "s" })).size(12.5).color(DIM));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(b) = best {
                        ui.label(RichText::new(format_lap_time(b as i64)).font(semibold(18.0)).color(theme::GOOD));
                    }
                });
            });
            if r.on_hover_text("Open this race day").clicked() {
                act.open_event = Some(e.key());
            }
        }
        None => {
            ui.label(RichText::new("No race days yet.").size(13.0).color(SECONDARY));
        }
    }
}

/// The driver's tile: engine picture, name, class and age, best lap (click to open it).
fn driver_tile(ui: &mut Ui, k: usize, d: &DriverRow, lib: &Library, profiles: &Profiles, today: aim_xrk::summary::Date, act: &mut Actions) {
    let tile_h = 84.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), tile_h), Sense::click());
    // The best lap opens on its own; the rest of the tile edits the driver.
    let lap_rect = Rect::from_min_max(pos2(rect.right() - 150.0, rect.top()), rect.max);
    let lap = d.best.as_ref().map(|_| ui.interact(lap_rect, ui.id().with(("best_lap", k)), Sense::click()));
    let lap_hovered = lap.as_ref().is_some_and(|l| l.hovered());
    ui.painter().rect_stroke(rect, CornerRadius::same(theme::RADIUS), Stroke::new(1.0, if resp.hovered() || lap_hovered { SECONDARY } else { theme::OUTLINE }), egui::StrokeKind::Inside);
    // The engine, about a third bigger than the tile and nudged left, so the tile's edges crop it.
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
    let p = ui.painter_at(rect);
    let mid = rect.center().y;
    let person = d.id.and_then(|id| profiles.drivers.iter().find(|p| p.id == id));
    let sub = [person.map(|p| p.class.trim().to_string()).unwrap_or_default(), person.and_then(|p| p.age(today)).map(|a| format!("age {a}")).unwrap_or_default()]
        .into_iter()
        .filter(|x| !x.trim().is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ");
    p.text(pos2(text_left, mid - 11.0), egui::Align2::LEFT_CENTER, &d.name, semibold(20.0), TEXT);
    p.text(pos2(text_left, mid + 13.0), egui::Align2::LEFT_CENTER, if sub.is_empty() { "Add class and age with Edit".to_string() } else { sub }, regular(13.0), SECONDARY);
    if let Some((track, ms, i)) = &d.best {
        let right = rect.right() - 18.0;
        p.text(pos2(right, mid - 12.0), egui::Align2::RIGHT_CENTER, format!("Best at {track}"), regular(12.5), DIM);
        p.text(pos2(right, mid + 10.0), egui::Align2::RIGHT_CENTER, format_lap_time(*ms as i64), semibold(22.0), if lap_hovered { TEXT } else { theme::GOOD });
        if let Some(lap) = lap {
            if lap.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if lap.on_hover_text("Open the best lap").clicked() {
                act.open_lap = Some(lib.sessions[*i].path.clone());
            }
        }
    }
    if resp.hovered() && !lap_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.on_hover_text("Edit this driver").clicked() && !lap_hovered && let Some(id) = d.id {
        act.edit_driver = profiles.drivers.iter().find(|p| p.id == id).map(|p| DriverEdit { driver: p.clone() });
    }
}

/// A kart's card: name, Setup and Edit; engine and MyChron beside the setup in use.
#[allow(clippy::too_many_arguments)]
fn kart_card(ui: &mut Ui, n: usize, row: &KartRow, lib: &Library, profiles: &Profiles, resolved: &[setup::Resolved], model_of: &dyn Fn(&str) -> String, act: &mut Actions) {
    Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin::symmetric(20, 16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        ui.spacing_mut().interact_size.y = 20.0;
        let name = row.kart.as_ref().map(|k| k.name.clone()).unwrap_or_else(|| "Not on a kart yet".into());
        let about: Vec<(&str, String)> = match &row.kart {
            Some(k) => vec![
                ("Engine", if k.engine.trim().is_empty() { "—".into() } else { k.engine.clone() }),
                ("MyChron", if k.logger.is_empty() { "None".into() } else { model_of(&k.logger) }),
            ],
            None => vec![("MyChron", format!("{} (logger {})", model_of(&row.logger), row.logger))],
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).font(semibold(18.0)).color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_button(ui, Icon::Pencil, "Edit").on_hover_text("Name, engine, driver, MyChron and baseline setup").clicked() {
                    act.edit_kart = Some(KartEdit::new(profiles, row.kart.as_ref(), &row.logger));
                }
                if let Some(newest) = row.newest && small_button(ui, Icon::Settings, "Setup").on_hover_text("Change the setup from the newest session on").clicked() {
                    act.edit_setup = Some(newest);
                }
            });
        });
        ui.add_space(6.0);
        let s = match (row.newest, &row.kart) {
            // The baseline was changed after the newest session: that's what it runs now.
            (Some(i), Some(k)) if k.past_baselines.last().is_some_and(|p| p.until.as_str() >= lib.sessions[i].start().as_str()) => k.baseline.clone(),
            (Some(i), _) => resolved[i].setup.clone(),
            (None, Some(k)) => k.baseline.clone(),
            (None, None) => Setup::default(),
        };
        let rows: Vec<(&str, String)> = [
            ("Gearing", if s.drive_teeth.trim().is_empty() && s.driven_teeth.trim().is_empty() { String::new() } else { format!("{} / {}", s.drive_teeth.trim(), s.driven_teeth.trim()) }),
            ("Tires", [s.tires.trim().to_string(), match (s.front_size.trim(), s.rear_size.trim()) {
                (f, r) if !f.is_empty() && f == r => f.to_string(),
                (f, r) if !f.is_empty() && !r.is_empty() => format!("{f} front, {r} rear"),
                (f, r) => format!("{f}{r}"),
            }]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")),
            ("Pressure", match (s.front_psi.trim(), s.rear_psi.trim()) { ("", "") => String::new(), (f, r) if f == r => format!("{f} psi"), (f, r) => format!("{f} / {r} psi") }),
            ("Weight", if s.weight_lb.trim().is_empty() { String::new() } else { format!("{} lb with driver", s.weight_lb.trim()) }),
            ("Ballast", if s.ballast_lb.trim().is_empty() { String::new() } else { format!("{} lb", s.ballast_lb.trim()) }),
            ("Jetting", s.jetting.trim().to_string()),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .collect();
        let grid = |ui: &mut Ui, id: (&str, usize), rows: &[(&str, String)]| {
            egui::Grid::new(id).num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(RichText::new(*k).size(13.0).color(DIM));
                    ui.label(RichText::new(v).size(13.5).color(TEXT));
                    ui.end_row();
                }
            });
        };
        ui.columns(2, |cols| {
            grid(&mut cols[0], ("kart_about", n), &about);
            if rows.is_empty() {
                cols[1].label(RichText::new("No setup recorded yet.").color(SECONDARY));
            } else {
                grid(&mut cols[1], ("kart_setup", n), &rows);
            }
        });
        ui.add_space(10.0);
        let when = if row.newest.is_some() { setup_last_changed(lib, resolved, &row.logger) } else { "Baseline, before its first session".to_string() };
        ui.label(RichText::new(when).size(12.5).color(DIM));
    });
}

/// "Changed in session 31 (Oct 10)", or where the setup comes from.
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
