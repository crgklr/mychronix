//! Looking after the Garage's people and things: setting up a MyChron that
//! shows up without a kart (new, or one a kart had before), and taking
//! drivers, karts and MyChrons away.
//!
//! Everything belongs to a driver. Deleting a driver deletes their karts and
//! sends their sessions to the Trash. Removing a kart or a MyChron only takes
//! it out of the Garage: its sessions keep it (and still show its setup).
//! A session belongs to the kart its MyChron was on when it was recorded.

use super::*;
use crate::profiles::{Kart, Profiles};

/// A removal waiting for "Are you sure?" (`sure`: the second step).
pub(super) enum Confirm {
    DeleteDriver {
        id: u32,
        sure: bool,
        backed_up: Option<Result<String, String>>,
    },
    RemoveKart {
        id: u32,
        sure: bool,
    },
    RemoveMyChron {
        logger: String,
        sure: bool,
    },
}

/// Setting up a MyChron: a driver (or a new one), a kart (or a new one), and
/// what to do when that kart already has a MyChron.
pub(super) struct MyChronSetup {
    logger: String,
    model: String,
    step: Step,
    driver: Choice,
    kart: Choice,
    /// Replace the MyChron on the chosen kart (else: a new kart for it).
    replace: bool,
    new_kart_name: String,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Step {
    Driver,
    Kart,
    Replace,
}

#[derive(PartialEq, Eq, Clone)]
enum Choice {
    None,
    Existing(u32),
    New(String),
}

impl MyChronSetup {
    pub(super) fn new(logger: &str, model: &str) -> Self {
        Self {
            logger: logger.into(),
            model: model.into(),
            step: Step::Driver,
            driver: Choice::None,
            kart: Choice::None,
            replace: true,
            new_kart_name: String::new(),
        }
    }
}

/// A MyChron that needs a home: never set up, or back without a kart.
pub(super) struct Unplaced {
    pub logger: String,
    /// Its sessions that belong to no kart.
    pub sessions: usize,
    /// It had a kart before: (driver, kart, when it came off).
    pub before: Option<(String, String, Option<crate::dates::Stamp>)>,
}

use look::Kind;

/// A session's files: the recording and everything kept beside it under
/// its name (`a_0001.xrz`, `a_0001.xri`, `a_0001.meta.json`, …).
/// The sessions a driver drove: named for them at download, else on their kart then
/// (`session_driver`), so a sibling's sessions on a shared kart aren't theirs.
fn driver_sessions<'a>(
    lib: &'a Library,
    profiles: &Profiles,
    name: &str,
) -> Vec<&'a tracks::LocalSession> {
    lib.sessions
        .iter()
        .filter(|s| !name.is_empty() && s.driver(profiles).eq_ignore_ascii_case(name))
        .collect()
}

fn session_files(path: &std::path::Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (
        path.parent(),
        path.file_stem().map(|s| s.to_string_lossy().into_owned()),
    ) else {
        return Vec::new();
    };
    let prefix = format!("{stem}.");
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Copies sessions' files into a new folder inside `into`, by MyChron; returns it.
fn back_up(
    driver: &str,
    sessions: &[(String, PathBuf)],
    into: &std::path::Path,
) -> Result<PathBuf, String> {
    let today = crate::dates::today();
    let folder = into.join(format!(
        "Mychronix backup - {} - {:04}-{:02}-{:02}",
        driver.replace(['/', '\\'], "-"),
        today.year,
        today.month,
        today.day
    ));
    let mut files = 0;
    for (logger, path) in sessions {
        let to = folder.join(logger);
        std::fs::create_dir_all(&to)
            .map_err(|e| format!("Couldn't create {}: {e}", to.display()))?;
        for f in session_files(path) {
            std::fs::copy(&f, to.join(f.file_name().unwrap_or_default()))
                .map_err(|e| format!("Couldn't copy {}: {e}", f.display()))?;
            files += 1;
        }
    }
    log::info!(
        "backed up {files} files of {driver}'s sessions to {}",
        folder.display()
    );
    Ok(folder)
}

impl App {
    /// Every known MyChron's model ("MyChron 4"), by ID: from its WiFi name,
    /// else its session summaries, else their digests.
    pub(super) fn mychron_models(&mut self) -> BTreeMap<String, String> {
        let lib = self.lib();
        let profiles = profiles(&mut self.profiles);
        let mut ids: Vec<String> = profiles
            .karts
            .iter()
            .flat_map(|k| k.all_loggers().cloned())
            .chain(lib.sessions.iter().map(|s| s.logger.clone()))
            .collect();
        ids.extend(
            self.device
                .logger
                .as_ref()
                .and_then(|b| b.logger_id)
                .map(|i| i.to_string()),
        );
        ids.sort();
        ids.dedup();
        let digests = self.sensors.digests.as_ref();
        ids.into_iter()
            .filter_map(|id| {
                let m = Some(profiles.for_logger(&id).model)
                    .filter(|m| !m.is_empty())
                    .or_else(|| {
                        lib.sessions
                            .iter()
                            .filter(|s| s.logger == id)
                            .find_map(|s| s.summary.model.clone())
                    })
                    .or_else(|| digests.and_then(|d| d.model(&id)))?;
                Some((id, m))
            })
            .collect()
    }

    /// MyChrons that need a home: ones with sessions that belong to no kart,
    /// or connected (over WiFi or by data key) without being on a kart in the
    /// Garage. One put off with "Not now" waits until it has more sessions
    /// without a kart, or is connected again.
    pub(super) fn unplaced_mychrons(&self, lib: &Library, profiles: &Profiles) -> Vec<Unplaced> {
        let connected: Vec<String> = self
            .device
            .logger
            .as_ref()
            .and_then(|b| b.logger_id)
            .map(|i| i.to_string())
            .into_iter()
            .chain(
                self.key
                    .listing
                    .as_ref()
                    .filter(|_| self.key.present)
                    .map(|(s, ..)| s.clone()),
            )
            .collect();
        let mut ids: Vec<String> = lib
            .sessions
            .iter()
            .filter(|s| s.kart.is_none())
            .map(|s| s.logger.clone())
            .chain(connected.iter().cloned())
            .collect();
        ids.retain(|l| {
            !l.is_empty()
                && l.chars().all(|c| c.is_ascii_digit())
                && profiles.mounted_on(l).is_none()
        });
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .filter_map(|logger| {
                let sessions = lib
                    .sessions
                    .iter()
                    .filter(|s| s.logger == logger && s.kart.is_none())
                    .count();
                let on_now = connected.contains(&logger);
                let snoozed = profiles.snoozed.get(&logger).is_some_and(|&n| {
                    sessions <= n && !(on_now && !self.garage.snoozed_while_on.contains(&logger))
                });
                if snoozed {
                    return None;
                }
                let before = profiles.last_kart_of(&logger).map(|(k, until)| {
                    (
                        profiles.driver_name(k.driver).unwrap_or("").to_string(),
                        k.name.clone(),
                        until,
                    )
                });
                Some(Unplaced {
                    logger,
                    sessions,
                    before,
                })
            })
            .collect()
    }

    /// The card at the top of the Garage for a MyChron that needs a home.
    /// A MyChron that needs a kart, with Set up and Not now. True when Not now
    /// was clicked: the caller then calls `snooze` once its profiles are back
    /// in `self` (the Garage holds them aside while it draws).
    #[must_use]
    pub(super) fn unplaced_card(&mut self, ui: &mut Ui, u: &Unplaced, model: &str) -> bool {
        let mut not_now = false;
        let (title, line) = match &u.before {
            Some((driver, kart, until)) => {
                let whose = if driver.is_empty() {
                    kart.clone()
                } else {
                    format!("{driver}'s {kart}")
                };
                let when = until.map(crate::dates::stamp_month_day).unwrap_or_default();
                (
                    format!("Previously used {model} detected"),
                    format!(
                        "Last on {whose}{}",
                        if when.is_empty() {
                            String::new()
                        } else {
                            format!(", until {when}")
                        }
                    ),
                )
            }
            None => (
                format!("New {model} detected"),
                "Not set up yet".to_string(),
            ),
        };
        let more = if u.sessions == 0 {
            String::new()
        } else {
            format!("  ·  {} without a kart", plural(u.sessions, "session"))
        };
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, theme::GOOD.gamma_multiply(0.5)))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::symmetric(22, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::hover());
                    ui.painter()
                        .circle_filled(r.center(), 23.0, theme::GOOD.gamma_multiply(0.16));
                    Icon::Mychron
                        .image(theme::GOOD, 26.0)
                        .paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(26.0)));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(RichText::new(title).font(semibold(17.0)).color(TEXT));
                        ui.label(
                            RichText::new(format!("ID {}  ·  {line}{more}", u.logger))
                                .font(regular(14.0))
                                .color(SECONDARY),
                        );
                        ui.label(
                            RichText::new("What would you like to do?")
                                .font(regular(13.5))
                                .color(DIM),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if look::big_button(ui, "Set up", None, Kind::Primary, true) {
                            // "the new MyChron" until its model is known.
                            let named = if model == "MyChron" && u.before.is_none() {
                                "new MyChron"
                            } else {
                                model
                            };
                            self.garage.mychron_setup = Some(MyChronSetup::new(&u.logger, named));
                        }
                        if look::big_button(ui, "Not now", None, Kind::Quiet, true) {
                            not_now = true;
                        }
                    });
                });
            });
        not_now
    }

    /// "Not now" for a MyChron that needs a kart: not asked again until it has new sessions.
    pub(super) fn snooze(&mut self, u: &Unplaced) {
        let connected = self
            .device
            .logger
            .as_ref()
            .and_then(|b| b.logger_id)
            .map(|i| i.to_string());
        if connected.as_deref() == Some(u.logger.as_str()) {
            self.garage.snoozed_while_on.insert(u.logger.clone());
        }
        self.edit_profiles(|p| {
            p.snoozed.insert(u.logger.clone(), u.sessions);
            true
        });
    }

    /// Setting up a MyChron: whose it is, which kart, and (if that kart has
    /// one) whether to replace it or give this one a new kart.
    pub(super) fn mychron_setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut s) = self.garage.mychron_setup.take() else {
            return;
        };
        let models = self.mychron_models();
        let model_of = |l: &str| models.get(l).cloned().unwrap_or_else(|| "MyChron".into());
        let lib = self.lib();
        let profiles = profiles(&mut self.profiles);
        let model = s.model.clone();
        let logger = s.logger.clone();
        let (mut cancel, mut finish) = (false, false);
        let driver_name = |p: &Profiles, c: &Choice| match c {
            Choice::Existing(id) => p.driver_name(Some(*id)).unwrap_or("").to_string(),
            Choice::New(n) => n.trim().to_string(),
            Choice::None => String::new(),
        };
        // A kart with another MyChron on it adds a third step.
        let occupied = |p: &Profiles, c: &Choice| match c {
            Choice::Existing(id) => p
                .karts
                .iter()
                .find(|k| k.id == *id && !k.logger.is_empty() && k.logger != logger)
                .cloned(),
            _ => None,
        };
        let steps = if s.step == Step::Replace || occupied(profiles, &s.kart).is_some() {
            3
        } else {
            2
        };
        let closed = look::dialog(ctx, "mychron_setup", look::WIDTH, |ui| match s.step {
            Step::Driver => {
                look::header(
                    ui,
                    Icon::Mychron,
                    theme::GOOD,
                    Some(&format!("Step 1 of {steps}")),
                    &format!("Set up the {model}"),
                    &format!("Whose {model} is it? (ID {logger})"),
                );
                look::body(ui, |ui| {
                    for d in profiles.drivers.clone() {
                        let karts = profiles
                            .garage_karts()
                            .filter(|k| k.driver == Some(d.id))
                            .count();
                        let sub = [d.class.trim().to_string(), plural(karts, "kart")]
                            .into_iter()
                            .filter(|x| !x.is_empty())
                            .collect::<Vec<_>>()
                            .join("  ·  ");
                        if look::option(ui, s.driver == Choice::Existing(d.id), None, &d.name, &sub)
                        {
                            s.driver = Choice::Existing(d.id);
                        }
                    }
                    let new = matches!(s.driver, Choice::New(_));
                    if look::option(ui, new, Some(Icon::Plus), "A new driver", "") && !new {
                        s.driver = Choice::New(String::new());
                    }
                    if let Choice::New(name) = &mut s.driver {
                        look::field(ui, name, "Their name").request_focus();
                    }
                    if matches!(&s.driver, Choice::New(n) if profiles.drivers.iter().any(|d| d.name.eq_ignore_ascii_case(n.trim())))
                    {
                        look::note(
                            ui,
                            "There's already a driver with that name: pick them above.",
                            theme::FAVORITE,
                        );
                    }
                });
                let taken = matches!(&s.driver, Choice::New(n) if profiles.drivers.iter().any(|d| d.name.eq_ignore_ascii_case(n.trim())));
                let ready = !driver_name(profiles, &s.driver).is_empty() && !taken;
                let (l, r) = look::footer(
                    ui,
                    "Cancel",
                    ("Next", Some(Icon::ChevronRight), Kind::Primary, ready),
                );
                cancel |= l;
                if r {
                    // A new driver has no karts: straight to naming one.
                    s.kart = if matches!(s.driver, Choice::New(_)) {
                        Choice::New(String::new())
                    } else {
                        Choice::None
                    };
                    s.step = Step::Kart;
                }
            }
            Step::Kart => {
                let who = driver_name(profiles, &s.driver);
                look::header(
                    ui,
                    Icon::Mychron,
                    theme::GOOD,
                    Some(&format!("Step 2 of {steps}")),
                    &format!("Which of {who}'s karts?"),
                    &format!("The kart the {model} goes on."),
                );
                look::body(ui, |ui| {
                    let theirs: Vec<Kart> = match s.driver {
                        Choice::Existing(id) => profiles
                            .garage_karts()
                            .filter(|k| k.driver == Some(id))
                            .cloned()
                            .collect(),
                        _ => Vec::new(),
                    };
                    for k in &theirs {
                        let on = if k.logger.is_empty() || k.logger == logger {
                            "No MyChron on it".to_string()
                        } else {
                            format!("Has a {} (ID {})", model_of(&k.logger), k.logger)
                        };
                        let sub = [k.engine.trim().to_string(), on]
                            .into_iter()
                            .filter(|x| !x.is_empty())
                            .collect::<Vec<_>>()
                            .join("  ·  ");
                        if look::option(ui, s.kart == Choice::Existing(k.id), None, &k.name, &sub) {
                            s.kart = Choice::Existing(k.id);
                        }
                    }
                    let new = matches!(s.kart, Choice::New(_));
                    if look::option(ui, new, Some(Icon::Plus), "A new kart", "") && !new {
                        s.kart = Choice::New(String::new());
                    }
                    if let Choice::New(name) = &mut s.kart {
                        look::field(ui, name, "Kart name, e.g. Mini Swift Cadet").request_focus();
                    }
                    if matches!(&s.kart, Choice::New(n) if profiles.karts.iter().any(|k| k.name.eq_ignore_ascii_case(n.trim())))
                    {
                        look::note(
                            ui,
                            "There's already a kart with that name.",
                            theme::FAVORITE,
                        );
                    }
                });
                let taken = matches!(&s.kart, Choice::New(n) if profiles.karts.iter().any(|k| k.name.eq_ignore_ascii_case(n.trim())));
                let ready = match &s.kart {
                    Choice::Existing(_) => true,
                    Choice::New(n) => !n.trim().is_empty() && !taken,
                    Choice::None => false,
                };
                let next = occupied(profiles, &s.kart).is_some();
                let action = if next {
                    ("Next", Some(Icon::ChevronRight), Kind::Primary, ready)
                } else {
                    ("Done", Some(Icon::Check), Kind::Primary, ready)
                };
                let (l, r) = look::footer(ui, "Back", action);
                if l {
                    s.step = Step::Driver;
                }
                if r {
                    if next {
                        s.step = Step::Replace
                    } else {
                        finish = true
                    }
                }
            }
            Step::Replace => {
                let kart = occupied(profiles, &s.kart).unwrap_or_default();
                let old = model_of(&kart.logger);
                let who = driver_name(profiles, &s.driver);
                look::header(
                    ui,
                    Icon::Mychron,
                    theme::GOOD,
                    Some("Step 3 of 3"),
                    &format!("{} already has a {old}", kart.name),
                    &format!(
                        "Its {old} (ID {}) can make way for the {model}, or the {model} can go on a new kart.",
                        kart.logger
                    ),
                );
                look::body(ui, |ui| {
                    if look::option(
                        ui,
                        s.replace,
                        None,
                        &format!("Replace the {old} with the {model}"),
                        &format!("The {old}'s sessions stay {who}'s, on {}.", kart.name),
                    ) {
                        s.replace = true;
                    }
                    if look::option(
                        ui,
                        !s.replace,
                        None,
                        &format!("Put the {model} on a new kart"),
                        &format!("{} keeps its {old}.", kart.name),
                    ) {
                        s.replace = false;
                    }
                    if !s.replace {
                        look::field(ui, &mut s.new_kart_name, "New kart's name").request_focus();
                        if profiles
                            .karts
                            .iter()
                            .any(|k| k.name.eq_ignore_ascii_case(s.new_kart_name.trim()))
                        {
                            look::note(
                                ui,
                                "There's already a kart with that name.",
                                theme::FAVORITE,
                            );
                        }
                    }
                });
                let taken = !s.replace
                    && profiles
                        .karts
                        .iter()
                        .any(|k| k.name.eq_ignore_ascii_case(s.new_kart_name.trim()));
                let ready = s.replace || (!s.new_kart_name.trim().is_empty() && !taken);
                let (l, r) = look::footer(
                    ui,
                    "Back",
                    ("Done", Some(Icon::Check), Kind::Primary, ready),
                );
                if l {
                    s.step = Step::Kart;
                }
                finish |= r;
            }
        });
        if finish {
            let mut kart = None;
            self.edit_profiles(|profiles| {
                let driver = match &s.driver {
                    Choice::Existing(id) => Some(*id),
                    Choice::New(n) => Some(profiles.driver_named(n)),
                    Choice::None => None,
                };
                kart = driver.and_then(|driver| {
                    let mut new_kart = |name: &str| {
                        let id = profiles.next_kart_id();
                        profiles.karts.push(Kart {
                            id,
                            name: name.trim().into(),
                            driver: Some(driver),
                            ..Default::default()
                        });
                        id
                    };
                    match (&s.kart, s.step) {
                        (Choice::Existing(_), Step::Replace) if !s.replace => {
                            Some(new_kart(&s.new_kart_name))
                        }
                        (Choice::Existing(id), _) => Some(*id),
                        (Choice::New(n), _) => Some(new_kart(n)),
                        (Choice::None, _) => None,
                    }
                });
                if let Some(kart) = kart {
                    // The MyChron it replaces keeps its sessions so far; this one's
                    // sessions without a kart become this kart's.
                    profiles.mount(&s.logger, kart, &|l: &str| {
                        crate::profiles::moved_now(lib.newest_start(l))
                    });
                }
                // Saved (and its sessions worked out again) when a kart got it or a
                // driver was added.
                kart.is_some() || matches!(s.driver, Choice::New(_))
            });
            if let Some(kart) = kart {
                log::info!("MyChron {} set up on kart {kart}", s.logger);
                return;
            }
        }
        if !cancel && !closed && !finish {
            self.garage.mychron_setup = Some(s);
        }
    }

    /// The two steps for deleting a driver or removing a kart or MyChron.
    pub(super) fn garage_confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut c) = self.garage.confirm.take() else {
            return;
        };
        let models = self.mychron_models();
        let lib = self.lib();
        let profiles = profiles(&mut self.profiles);
        let (mut keep, mut done) = (true, false);
        let closed = match &mut c {
            Confirm::DeleteDriver {
                id,
                sure,
                backed_up,
            } => {
                let name = profiles
                    .driver_name(Some(*id))
                    .unwrap_or("this driver")
                    .to_string();
                let karts: Vec<u32> = profiles
                    .karts
                    .iter()
                    .filter(|k| k.driver == Some(*id))
                    .map(|k| k.id)
                    .collect();
                let theirs: Vec<(String, PathBuf)> = driver_sessions(&lib, profiles, &name)
                    .into_iter()
                    .map(|s| (s.logger.clone(), s.path.clone()))
                    .collect();
                let mut mychrons: Vec<&String> = theirs.iter().map(|(l, _)| l).collect();
                mychrons.sort();
                mychrons.dedup();
                look::dialog(ctx, "delete_driver", look::WIDTH, |ui| {
                    if !*sure {
                        look::header(
                            ui,
                            Icon::Trash,
                            theme::DANGER,
                            None,
                            &format!("Delete {name}?"),
                            &format!(
                                "{name} and everything of theirs goes. Session files go to your computer's Trash."
                            ),
                        );
                        look::body(ui, |ui| {
                            Frame::new()
                                .fill(theme::DANGER.gamma_multiply(0.08))
                                .stroke(Stroke::new(1.0, theme::DANGER.gamma_multiply(0.35)))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(Margin::same(16))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = 10.0;
                                    look::fact(
                                        ui,
                                        Icon::User,
                                        theme::DANGER,
                                        &format!("{name}'s profile"),
                                    );
                                    look::fact(
                                        ui,
                                        Icon::Flag,
                                        theme::DANGER,
                                        &plural(karts.len(), "kart"),
                                    );
                                    look::fact(
                                        ui,
                                        Icon::Mychron,
                                        theme::DANGER,
                                        &format!(
                                            "{} recorded on {}",
                                            plural(theirs.len(), "session"),
                                            plural(mychrons.len(), "MyChron")
                                        ),
                                    );
                                });
                            ui.add_space(6.0);
                            Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(16)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_max_width(ui.available_width() - 170.0);
                                        ui.label(RichText::new("Back up their sessions first").font(semibold(16.0)).color(TEXT));
                                        ui.add(egui::Label::new(RichText::new("Copies them to a folder you pick, so you can keep them or open them later.").font(regular(13.5)).color(SECONDARY)).wrap());
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let back_up_clicked = look::big_button(ui, "Back up…", Some(Icon::FolderOpen), Kind::Quiet, !theirs.is_empty());
                                        if back_up_clicked && let Some(into) = rfd::FileDialog::new().set_title("Back up sessions to…").pick_folder() {
                                            *backed_up = Some(back_up(&name, &theirs, &into).map(|p| p.display().to_string()));
                                        }
                                    });
                                });
                                match backed_up {
                                    Some(Ok(p)) => {
                                        ui.add_space(8.0);
                                        look::fact(ui, Icon::CircleCheck, theme::GOOD, &format!("Backed up to {p}"));
                                    }
                                    Some(Err(e)) => look::note(ui, e, theme::DANGER),
                                    None => {}
                                }
                            });
                        });
                        let (l, r) = look::footer(
                            ui,
                            "Cancel",
                            (
                                &format!("Delete {name}…"),
                                Some(Icon::Trash),
                                Kind::Danger,
                                true,
                            ),
                        );
                        keep &= !l;
                        *sure |= r;
                    } else {
                        look::header(
                            ui,
                            Icon::TriangleAlert,
                            theme::DANGER,
                            Some("Last step"),
                            "Are you sure?",
                            &format!(
                                "{name}, their karts and MyChrons, and {} will be deleted.",
                                plural(theirs.len(), "session")
                            ),
                        );
                        look::body(ui, |ui| {
                            look::note(
                                ui,
                                "You can only get the session files back from your computer's Trash.",
                                SECONDARY,
                            );
                            if !matches!(backed_up, Some(Ok(_))) && !theirs.is_empty() {
                                look::fact(
                                    ui,
                                    Icon::TriangleAlert,
                                    theme::FAVORITE,
                                    "You haven't backed up their sessions.",
                                );
                            }
                        });
                        let (l, r) = look::footer(
                            ui,
                            "Go back",
                            (
                                &format!("Yes, delete {name}"),
                                Some(Icon::Trash),
                                Kind::Danger,
                                true,
                            ),
                        );
                        if l {
                            *sure = false;
                        }
                        done |= r;
                    }
                })
            }
            Confirm::RemoveKart { id, sure } => {
                let kart = profiles
                    .karts
                    .iter()
                    .find(|k| k.id == *id)
                    .cloned()
                    .unwrap_or_default();
                let sessions = lib.sessions.iter().filter(|s| s.kart == Some(*id)).count();
                look::dialog(ctx, "remove_kart", look::WIDTH, |ui| {
                    if !*sure {
                        look::header(
                            ui,
                            Icon::EyeOff,
                            SECONDARY,
                            None,
                            &format!("Remove {} from the Garage?", kart.name),
                            "Nothing is deleted.",
                        );
                        look::body(ui, |ui| {
                            look::fact(
                                ui,
                                Icon::Check,
                                theme::GOOD,
                                &format!(
                                    "Its {} stay, and still show this kart and its setup",
                                    plural(sessions, "session")
                                ),
                            );
                            if !kart.logger.is_empty() {
                                let m = models
                                    .get(&kart.logger)
                                    .cloned()
                                    .unwrap_or_else(|| "MyChron".into());
                                look::fact(
                                    ui,
                                    Icon::EyeOff,
                                    SECONDARY,
                                    &format!("The {m} on it leaves the Garage with it"),
                                );
                            }
                        });
                        let (l, r) = look::footer(
                            ui,
                            "Cancel",
                            ("Remove…", Some(Icon::EyeOff), Kind::Danger, true),
                        );
                        keep &= !l;
                        *sure |= r;
                    } else {
                        look::header(
                            ui,
                            Icon::TriangleAlert,
                            theme::DANGER,
                            Some("Last step"),
                            "Are you sure?",
                            &format!(
                                "{} won't show in the Garage anymore. Its sessions stay.",
                                kart.name
                            ),
                        );
                        look::body(ui, |_| {});
                        let (l, r) = look::footer(
                            ui,
                            "Go back",
                            ("Yes, remove it", Some(Icon::EyeOff), Kind::Danger, true),
                        );
                        if l {
                            *sure = false;
                        }
                        done |= r;
                    }
                })
            }
            Confirm::RemoveMyChron { logger, sure } => {
                let model = models
                    .get(logger.as_str())
                    .cloned()
                    .unwrap_or_else(|| "MyChron".into());
                let kart = profiles.kart_on(logger).map(|k| k.name.clone());
                let sessions = lib
                    .sessions
                    .iter()
                    .filter(|s| &s.logger == logger && s.kart.is_some())
                    .count();
                look::dialog(ctx, "remove_mychron", look::WIDTH, |ui| {
                    if !*sure {
                        look::header(
                            ui,
                            Icon::EyeOff,
                            SECONDARY,
                            None,
                            &format!("Remove the {model} from the Garage?"),
                            &format!("Nothing is deleted. (ID {logger})"),
                        );
                        look::body(ui, |ui| {
                            let on = kart
                                .as_ref()
                                .map(|k| format!(", on {k}"))
                                .unwrap_or_default();
                            look::fact(
                                ui,
                                Icon::Check,
                                theme::GOOD,
                                &format!(
                                    "Its {} stay{on}, and still show this MyChron",
                                    plural(sessions, "session")
                                ),
                            );
                            look::fact(
                                ui,
                                Icon::Mychron,
                                SECONDARY,
                                "If it records more, it's asked about again",
                            );
                        });
                        let (l, r) = look::footer(
                            ui,
                            "Cancel",
                            ("Remove…", Some(Icon::EyeOff), Kind::Danger, true),
                        );
                        keep &= !l;
                        *sure |= r;
                    } else {
                        look::header(
                            ui,
                            Icon::TriangleAlert,
                            theme::DANGER,
                            Some("Last step"),
                            "Are you sure?",
                            &format!(
                                "The {model} won't show in the Garage anymore. Its sessions stay."
                            ),
                        );
                        look::body(ui, |_| {});
                        let (l, r) = look::footer(
                            ui,
                            "Go back",
                            ("Yes, remove it", Some(Icon::EyeOff), Kind::Danger, true),
                        );
                        if l {
                            *sure = false;
                        }
                        done |= r;
                    }
                })
            }
        };
        if done {
            self.apply_confirm(c, &lib);
            return;
        }
        if keep && !closed {
            self.garage.confirm = Some(c);
        }
    }

    fn apply_confirm(&mut self, c: Confirm, lib: &Library) {
        let profiles = profiles(&mut self.profiles);
        let newest = |l: &str| crate::profiles::moved_now(lib.newest_start(l));
        match c {
            Confirm::DeleteDriver { id, .. } => {
                let name = profiles
                    .driver_name(Some(id))
                    .unwrap_or_default()
                    .to_string();
                let karts: Vec<u32> = profiles
                    .karts
                    .iter()
                    .filter(|k| k.driver == Some(id))
                    .map(|k| k.id)
                    .collect();
                let theirs = driver_sessions(lib, profiles, &name);
                let files: Vec<PathBuf> =
                    theirs.iter().flat_map(|s| session_files(&s.path)).collect();
                let open_is_theirs = self
                    .path
                    .as_ref()
                    .is_some_and(|p| theirs.iter().any(|s| &s.path == p));
                if !files.is_empty()
                    && let Err(e) = trash::delete_all(&files)
                {
                    self.notices.error(format!(
                        "{name} was deleted, but some session files couldn't go to the Trash: {e}"
                    ));
                }
                // A MyChron's folder left empty goes too.
                let mut dirs: Vec<&std::path::Path> =
                    theirs.iter().filter_map(|s| s.path.parent()).collect();
                dirs.sort();
                dirs.dedup();
                for d in dirs {
                    if std::fs::read_dir(d).is_ok_and(|mut rd| rd.next().is_none()) {
                        let _ = std::fs::remove_dir(d);
                    }
                }
                log::info!(
                    "deleted driver {name}: {} karts, {} sessions ({} files) to the Trash",
                    karts.len(),
                    theirs.len(),
                    files.len()
                );
                // Saved once the files are gone, so the library reloads without them.
                self.edit_profiles(|p| {
                    p.delete_driver(id);
                    true
                });
                if open_is_theirs {
                    self.act(Action::Close);
                    self.page = Page::Garage;
                }
                self.garage.page = 0;
            }
            Confirm::RemoveKart { id, .. } => {
                self.edit_profiles(|p| {
                    p.remove_kart(id);
                    true
                });
            }
            Confirm::RemoveMyChron { logger, .. } => {
                self.edit_profiles(|p| {
                    p.remove_logger(&logger, &newest);
                    true
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sessions_files_are_everything_under_its_name() {
        let dir = std::env::temp_dir().join(format!("mychronix-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "a_0001.xrz",
            "a_0001.xri",
            "a_0001.meta.json",
            "a_0001-2.xrz",
            "a_0001-2.xri",
            "a_0010.xrz",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let mut got: Vec<String> = session_files(&dir.join("a_0001.xrz"))
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        got.sort();
        assert_eq!(got, ["a_0001.meta.json", "a_0001.xri", "a_0001.xrz"]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            crate::dates::stamp_month_day(
                crate::dates::Stamp::parse("2026-12-20 10:00:00").unwrap()
            ),
            "Dec 20"
        );
    }
}
