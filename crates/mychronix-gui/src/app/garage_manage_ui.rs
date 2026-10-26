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
    DeleteDriver { id: u32, sure: bool, backed_up: Option<Result<String, String>> },
    RemoveKart { id: u32, sure: bool },
    RemoveMyChron { logger: String, sure: bool },
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
        Self { logger: logger.into(), model: model.into(), step: Step::Driver, driver: Choice::None, kart: Choice::None, replace: true, new_kart_name: String::new() }
    }
}

/// A MyChron that needs a home: never set up, or back without a kart.
pub(super) struct Unplaced {
    pub logger: String,
    /// Its sessions that belong to no kart.
    pub sessions: usize,
    /// It had a kart before: (driver, kart, when it came off).
    pub before: Option<(String, String, String)>,
}

/// The look shared by the Garage's windows: a darkened backdrop, a raised
/// panel with generous spacing, a header with an icon, a footer with the
/// actions.
pub(super) mod look {
    use super::*;

    pub const WIDTH: f32 = 600.0;
    const PAD: i8 = 32;

    /// A modal window: everything behind it dimmed and blocked. True when
    /// the person asked to close it (Escape, or a click on the backdrop).
    pub fn dialog(ctx: &egui::Context, id: &str, width: f32, body: impl FnOnce(&mut Ui)) -> bool {
        let frame = Frame::new()
            .fill(SIDEBAR)
            .stroke(Stroke::new(1.0, theme::OUTLINE))
            .corner_radius(CornerRadius::same(18))
            .shadow(egui::epaint::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(150) });
        let r = egui::Modal::new(egui::Id::new(id)).backdrop_color(Color32::from_black_alpha(215)).frame(frame).show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui);
        });
        r.should_close()
    }

    /// The top of a window: a tinted icon badge, an optional small label
    /// above the title (e.g. "Step 1 of 3"), the title and a line under it.
    pub fn header(ui: &mut Ui, icon: Icon, tint: Color32, eyebrow: Option<&str>, title: &str, subtitle: &str) {
        Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 30, bottom: 8 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 26.0, tint.gamma_multiply(0.18));
                icon.image(tint, 26.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(26.0)));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    if let Some(e) = eyebrow {
                        ui.label(RichText::new(e.to_uppercase()).font(semibold(11.5)).color(DIM).extra_letter_spacing(0.9));
                    }
                    ui.add(egui::Label::new(RichText::new(title).font(semibold(24.0)).color(TEXT)).wrap());
                    if !subtitle.is_empty() {
                        ui.add(egui::Label::new(RichText::new(subtitle).font(regular(15.5)).color(SECONDARY)).wrap());
                    }
                });
            });
        });
    }

    /// The middle of a window.
    pub fn body(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 16, bottom: 24 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            add(ui);
        });
    }

    #[derive(Clone, Copy, PartialEq)]
    pub enum Kind {
        Primary,
        Danger,
        Quiet,
    }

    /// A large button: 44 points tall.
    pub fn big_button(ui: &mut Ui, label: &str, icon: Option<Icon>, kind: Kind, enabled: bool) -> bool {
        let (fill, fg) = match kind {
            Kind::Primary => (TEXT, ON_TEXT),
            Kind::Danger => (theme::DANGER, Color32::WHITE),
            Kind::Quiet => (Color32::TRANSPARENT, TEXT),
        };
        let galley = ui.painter().layout_no_wrap(label.into(), semibold(15.0), fg);
        let icon_w = if icon.is_some() { 26.0 } else { 0.0 };
        let (rect, resp) = ui.allocate_exact_size(vec2((galley.size().x + 44.0 + icon_w).max(110.0), 44.0), if enabled { Sense::click() } else { Sense::hover() });
        let hovered = enabled && resp.hovered();
        let fill = match kind {
            Kind::Quiet if hovered => SURFACE_HOVER,
            _ if hovered => fill.gamma_multiply(0.88),
            _ => fill,
        };
        let alpha = if enabled { 1.0 } else { 0.35 };
        ui.painter().rect_filled(rect, CornerRadius::same(12), fill.gamma_multiply(alpha));
        if kind == Kind::Quiet {
            ui.painter().rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, theme::OUTLINE), egui::StrokeKind::Inside);
        }
        // Icons lead the label, except an arrow forward, which follows it.
        let trailing = icon == Some(Icon::ChevronRight);
        let mut x = rect.center().x - (galley.size().x + icon_w) / 2.0;
        let paint_icon = |ui: &Ui, i: Icon, x: f32| i.image(fg.gamma_multiply(alpha), 18.0).paint_at(ui, Rect::from_min_size(pos2(x, rect.center().y - 9.0), Vec2::splat(18.0)));
        if let Some(i) = icon.filter(|_| !trailing) {
            paint_icon(ui, i, x);
            x += icon_w;
        }
        let text_w = galley.size().x;
        ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg.gamma_multiply(alpha));
        if let Some(i) = icon.filter(|_| trailing) {
            paint_icon(ui, i, x + text_w + 8.0);
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        enabled && resp.clicked()
    }

    /// The bottom of a window: a divider, a quiet button on the left and the
    /// main action on the right. Returns (left clicked, right clicked).
    pub fn footer(ui: &mut Ui, left: &str, right: (&str, Option<Icon>, Kind, bool)) -> (bool, bool) {
        let (mut l, mut r) = (false, false);
        let top = ui.cursor().top();
        ui.painter().hline(ui.max_rect().x_range(), top, Stroke::new(1.0, theme::OUTLINE));
        Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 18, bottom: 22 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                l = big_button(ui, left, None, Kind::Quiet, true);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    r = big_button(ui, right.0, right.1, right.2, right.3);
                });
            });
        });
        (l, r)
    }

    /// The bottom of a window with buttons of its own: a divider and padding.
    pub fn footer_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        let top = ui.cursor().top();
        ui.painter().hline(ui.max_rect().x_range(), top, Stroke::new(1.0, theme::OUTLINE));
        Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 18, bottom: 22 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add);
        });
    }

    /// One choice in a list: a full-width row with a radio mark (or an icon),
    /// a title and a line under it. True when picked.
    pub fn option(ui: &mut Ui, selected: bool, icon: Option<Icon>, title: &str, subtitle: &str) -> bool {
        let h = if subtitle.is_empty() { 54.0 } else { 66.0 };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
        let hovered = resp.hovered();
        let (fill, stroke) = match (selected, hovered) {
            (true, _) => (SURFACE_ACTIVE, Stroke::new(1.5, TEXT)),
            (false, true) => (SURFACE_HOVER, Stroke::new(1.0, theme::OUTLINE)),
            (false, false) => (SURFACE, Stroke::new(1.0, theme::OUTLINE)),
        };
        ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
        ui.painter().rect_stroke(rect, CornerRadius::same(12), stroke, egui::StrokeKind::Inside);
        let mark = pos2(rect.left() + 28.0, rect.center().y);
        match icon {
            Some(i) => i.image(if selected { TEXT } else { SECONDARY }, 20.0).paint_at(ui, Rect::from_center_size(mark, Vec2::splat(20.0))),
            None => {
                ui.painter().circle_stroke(mark, 9.0, Stroke::new(2.0, if selected { TEXT } else { DIM }));
                if selected {
                    ui.painter().circle_filled(mark, 4.5, TEXT);
                }
            }
        }
        let x = rect.left() + 54.0;
        if subtitle.is_empty() {
            ui.painter().text(pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, title, semibold(16.0), TEXT);
        } else {
            ui.painter().text(pos2(x, rect.center().y - 10.0), egui::Align2::LEFT_CENTER, title, semibold(16.0), TEXT);
            ui.painter().text(pos2(x, rect.center().y + 12.0), egui::Align2::LEFT_CENTER, subtitle, regular(13.5), SECONDARY);
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked()
    }

    /// A form label beside or above a field.
    pub fn label(ui: &mut Ui, text: &str) {
        ui.label(RichText::new(text).font(medium(16.0)).color(SECONDARY));
    }

    /// A text field `width` wide (or the row's width when None).
    pub fn text(ui: &mut Ui, value: &mut String, hint: &str, width: Option<f32>) -> egui::Response {
        let w = width.unwrap_or_else(|| ui.available_width());
        ui.add(egui::TextEdit::singleline(value).font(regular(16.5)).margin(Margin::symmetric(14, 12)).desired_width(w).hint_text(hint))
    }

    /// The steps of a form, as a bar of tabs under the header: done, current
    /// and to come. When `jump` is set (editing), any step can be clicked.
    pub fn steps(ui: &mut Ui, names: &[&str], current: usize, jump: bool) -> Option<usize> {
        let mut to = None;
        Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 8, bottom: 4 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let w = (ui.available_width() - 8.0 * (names.len() - 1) as f32) / names.len() as f32;
                for (k, name) in names.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 44.0), if jump { Sense::click() } else { Sense::hover() });
                    let on = k == current;
                    let done = k < current && !jump;
                    let bar = Rect::from_min_size(rect.left_top(), vec2(rect.width(), 4.0));
                    ui.painter().rect_filled(bar, CornerRadius::same(2), if on || done { TEXT } else { theme::OUTLINE });
                    let color = if on { TEXT } else if jump && resp.hovered() { SECONDARY } else { DIM };
                    ui.painter().text(pos2(rect.left(), rect.top() + 24.0), egui::Align2::LEFT_CENTER, format!("{}  {name}", k + 1), semibold(14.0), color);
                    if jump && resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if jump && resp.clicked() {
                        to = Some(k);
                    }
                }
            });
        });
        to
    }

    /// A text box of several lines, `width` wide, styled like `text`.
    pub fn text_area(ui: &mut Ui, value: &mut String, hint: &str, width: f32, rows: usize) -> egui::Response {
        ui.add(egui::TextEdit::multiline(value).font(regular(16.5)).margin(Margin::symmetric(14, 12)).desired_width(width).desired_rows(rows).hint_text(hint))
    }

    /// A text field sized for a window.
    pub fn field(ui: &mut Ui, text: &mut String, hint: &str) -> egui::Response {
        ui.add(egui::TextEdit::singleline(text).font(regular(16.0)).margin(Margin::symmetric(14, 12)).desired_width(ui.available_width()).hint_text(hint))
    }

    /// A line of guidance or warning.
    pub fn note(ui: &mut Ui, text: &str, color: Color32) {
        ui.add(egui::Label::new(RichText::new(text).font(medium(14.0)).color(color)).wrap());
    }

    /// A row of what an action touches: an icon and a line.
    pub fn fact(ui: &mut Ui, icon: Icon, tint: Color32, text: &str) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add(icon.image(tint, 18.0));
            ui.add(egui::Label::new(RichText::new(text).font(regular(15.5)).color(TEXT)).wrap());
        });
    }
}

use look::Kind;

/// A session's files: the recording and everything kept beside it under
/// its name (`a_0001.xrz`, `a_0001.xri`, `a_0001.meta.json`, …).
fn session_files(path: &std::path::Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().map(|s| s.to_string_lossy().into_owned())) else { return Vec::new() };
    let prefix = format!("{stem}.");
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix))).collect())
        .unwrap_or_default()
}

/// Copies sessions' files into a new folder inside `into`, by MyChron; returns it.
fn back_up(driver: &str, sessions: &[(String, PathBuf)], into: &std::path::Path) -> Result<PathBuf, String> {
    let today = crate::events::today();
    let folder = into.join(format!("Mychronix backup - {} - {:04}-{:02}-{:02}", driver.replace(['/', '\\'], "-"), today.year, today.month, today.day));
    let mut files = 0;
    for (logger, path) in sessions {
        let to = folder.join(logger);
        std::fs::create_dir_all(&to).map_err(|e| format!("Couldn't create {}: {e}", to.display()))?;
        for f in session_files(path) {
            std::fs::copy(&f, to.join(f.file_name().unwrap_or_default())).map_err(|e| format!("Couldn't copy {}: {e}", f.display()))?;
            files += 1;
        }
    }
    log::info!("backed up {files} files of {driver}'s sessions to {}", folder.display());
    Ok(folder)
}

/// "Dec 20".
fn short_day(stamp: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut p = stamp.get(..10).unwrap_or("").split('-').map(|x| x.parse::<usize>().ok());
    match (p.next().flatten(), p.next().flatten(), p.next().flatten()) {
        (Some(_), Some(m), Some(d)) if (1..=12).contains(&m) => format!("{} {d}", M[m - 1]),
        _ => String::new(),
    }
}

impl App {
    /// Every known MyChron's model ("MyChron 4"), by ID: from its WiFi name,
    /// else its session summaries, else their digests.
    pub(super) fn mychron_models(&mut self) -> BTreeMap<String, String> {
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
        let lib = self.library.get_or_insert_with(Library::load);
        let mut ids: Vec<String> = profiles.karts.iter().flat_map(|k| k.all_loggers().cloned()).chain(lib.sessions.iter().map(|s| s.logger.clone())).collect();
        ids.extend(self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()));
        ids.sort();
        ids.dedup();
        let digests = self.sensors.digests.as_ref();
        ids.into_iter()
            .filter_map(|id| {
                let m = Some(profiles.for_logger(&id).model)
                    .filter(|m| !m.is_empty())
                    .or_else(|| lib.sessions.iter().filter(|s| s.logger == id).find_map(|s| s.summary.model.clone()))
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
            .dev_logger
            .as_ref()
            .and_then(|b| b.logger_id)
            .map(|i| i.to_string())
            .into_iter()
            .chain(self.key.listing.as_ref().filter(|_| self.key.present).map(|(s, ..)| s.clone()))
            .collect();
        let mut ids: Vec<String> = lib.sessions.iter().filter(|s| s.kart.is_none()).map(|s| s.logger.clone()).chain(connected.iter().cloned()).collect();
        ids.retain(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()) && profiles.mounted_on(l).is_none());
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .filter_map(|logger| {
                let sessions = lib.sessions.iter().filter(|s| s.logger == logger && s.kart.is_none()).count();
                let on_now = connected.contains(&logger);
                let snoozed = profiles.snoozed.get(&logger).is_some_and(|&n| sessions <= n && !(on_now && !self.snoozed_while_on.contains(&logger)));
                if snoozed {
                    return None;
                }
                let before = profiles.last_kart_of(&logger).map(|(k, until)| (profiles.driver_name(k.driver).unwrap_or("").to_string(), k.name.clone(), until.to_string()));
                Some(Unplaced { logger, sessions, before })
            })
            .collect()
    }

    /// The card at the top of the Garage for a MyChron that needs a home.
    pub(super) fn unplaced_card(&mut self, ui: &mut Ui, u: &Unplaced, model: &str) {
        let (title, line) = match &u.before {
            Some((driver, kart, until)) => {
                let whose = if driver.is_empty() { kart.clone() } else { format!("{driver}'s {kart}") };
                let when = short_day(until);
                (format!("Previously used {model} detected"), format!("Last on {whose}{}", if when.is_empty() { String::new() } else { format!(", until {when}") }))
            }
            None => (format!("New {model} detected"), "Not set up yet".to_string()),
        };
        let more = if u.sessions == 0 { String::new() } else { format!("  ·  {} without a kart", plural(u.sessions, "session")) };
        Frame::new().fill(SURFACE).stroke(Stroke::new(1.0, theme::GOOD.gamma_multiply(0.5))).corner_radius(CornerRadius::same(14)).inner_margin(Margin::symmetric(22, 16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 23.0, theme::GOOD.gamma_multiply(0.16));
                Icon::Mychron.image(theme::GOOD, 26.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(26.0)));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new(title).font(semibold(17.0)).color(TEXT));
                    ui.label(RichText::new(format!("ID {}  ·  {line}{more}", u.logger)).font(regular(14.0)).color(SECONDARY));
                    ui.label(RichText::new("What would you like to do?").font(regular(13.5)).color(DIM));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if look::big_button(ui, "Set up", None, Kind::Primary, true) {
                        // "the new MyChron" until its model is known.
                        let named = if model == "MyChron" && u.before.is_none() { "new MyChron" } else { model };
                        self.mychron_setup = Some(MyChronSetup::new(&u.logger, named));
                    }
                    if look::big_button(ui, "Not now", None, Kind::Quiet, true) {
                        let connected = self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string());
                        if connected.as_deref() == Some(u.logger.as_str()) {
                            self.snoozed_while_on.insert(u.logger.clone());
                        }
                        let profiles = self.profiles.get_or_insert_with(Profiles::load);
                        profiles.snoozed.insert(u.logger.clone(), u.sessions);
                        if let Err(e) = profiles.save() {
                            self.error = Some(format!("Couldn't save: {e}"));
                        }
                    }
                });
            });
        });
    }

    /// Setting up a MyChron: whose it is, which kart, and (if that kart has
    /// one) whether to replace it or give this one a new kart.
    pub(super) fn mychron_setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut s) = self.mychron_setup.take() else { return };
        let models = self.mychron_models();
        let model_of = |l: &str| models.get(l).cloned().unwrap_or_else(|| "MyChron".into());
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
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
            Choice::Existing(id) => p.karts.iter().find(|k| k.id == *id && !k.logger.is_empty() && k.logger != logger).cloned(),
            _ => None,
        };
        let steps = if s.step == Step::Replace || occupied(profiles, &s.kart).is_some() { 3 } else { 2 };
        let closed = look::dialog(ctx, "mychron_setup", look::WIDTH, |ui| match s.step {
            Step::Driver => {
                look::header(ui, Icon::Mychron, theme::GOOD, Some(&format!("Step 1 of {steps}")), &format!("Set up the {model}"), &format!("Whose {model} is it? (ID {logger})"));
                look::body(ui, |ui| {
                    for d in profiles.drivers.clone() {
                        let karts = profiles.garage_karts().filter(|k| k.driver == Some(d.id)).count();
                        let sub = [d.class.trim().to_string(), plural(karts, "kart")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("  ·  ");
                        if look::option(ui, s.driver == Choice::Existing(d.id), None, &d.name, &sub) {
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
                    if matches!(&s.driver, Choice::New(n) if profiles.drivers.iter().any(|d| d.name.eq_ignore_ascii_case(n.trim()))) {
                        look::note(ui, "There's already a driver with that name: pick them above.", theme::FAVORITE);
                    }
                });
                let taken = matches!(&s.driver, Choice::New(n) if profiles.drivers.iter().any(|d| d.name.eq_ignore_ascii_case(n.trim())));
                let ready = !driver_name(profiles, &s.driver).is_empty() && !taken;
                let (l, r) = look::footer(ui, "Cancel", ("Next", Some(Icon::ChevronRight), Kind::Primary, ready));
                cancel |= l;
                if r {
                    // A new driver has no karts: straight to naming one.
                    s.kart = if matches!(s.driver, Choice::New(_)) { Choice::New(String::new()) } else { Choice::None };
                    s.step = Step::Kart;
                }
            }
            Step::Kart => {
                let who = driver_name(profiles, &s.driver);
                look::header(ui, Icon::Mychron, theme::GOOD, Some(&format!("Step 2 of {steps}")), &format!("Which of {who}'s karts?"), &format!("The kart the {model} goes on."));
                look::body(ui, |ui| {
                    let theirs: Vec<Kart> = match s.driver {
                        Choice::Existing(id) => profiles.garage_karts().filter(|k| k.driver == Some(id)).cloned().collect(),
                        _ => Vec::new(),
                    };
                    for k in &theirs {
                        let on = if k.logger.is_empty() || k.logger == logger { "No MyChron on it".to_string() } else { format!("Has a {} (ID {})", model_of(&k.logger), k.logger) };
                        let sub = [k.engine.trim().to_string(), on].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("  ·  ");
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
                    if matches!(&s.kart, Choice::New(n) if profiles.karts.iter().any(|k| k.name.eq_ignore_ascii_case(n.trim()))) {
                        look::note(ui, "There's already a kart with that name.", theme::FAVORITE);
                    }
                });
                let taken = matches!(&s.kart, Choice::New(n) if profiles.karts.iter().any(|k| k.name.eq_ignore_ascii_case(n.trim())));
                let ready = match &s.kart {
                    Choice::Existing(_) => true,
                    Choice::New(n) => !n.trim().is_empty() && !taken,
                    Choice::None => false,
                };
                let next = occupied(profiles, &s.kart).is_some();
                let action = if next { ("Next", Some(Icon::ChevronRight), Kind::Primary, ready) } else { ("Done", Some(Icon::Check), Kind::Primary, ready) };
                let (l, r) = look::footer(ui, "Back", action);
                if l {
                    s.step = Step::Driver;
                }
                if r {
                    if next { s.step = Step::Replace } else { finish = true }
                }
            }
            Step::Replace => {
                let kart = occupied(profiles, &s.kart).unwrap_or_default();
                let old = model_of(&kart.logger);
                let who = driver_name(profiles, &s.driver);
                look::header(ui, Icon::Mychron, theme::GOOD, Some("Step 3 of 3"), &format!("{} already has a {old}", kart.name), &format!("Its {old} (ID {}) can make way for the {model}, or the {model} can go on a new kart.", kart.logger));
                look::body(ui, |ui| {
                    if look::option(ui, s.replace, None, &format!("Replace the {old} with the {model}"), &format!("The {old}'s sessions stay {who}'s, on {}.", kart.name)) {
                        s.replace = true;
                    }
                    if look::option(ui, !s.replace, None, &format!("Put the {model} on a new kart"), &format!("{} keeps its {old}.", kart.name)) {
                        s.replace = false;
                    }
                    if !s.replace {
                        look::field(ui, &mut s.new_kart_name, "New kart's name").request_focus();
                        if profiles.karts.iter().any(|k| k.name.eq_ignore_ascii_case(s.new_kart_name.trim())) {
                            look::note(ui, "There's already a kart with that name.", theme::FAVORITE);
                        }
                    }
                });
                let taken = !s.replace && profiles.karts.iter().any(|k| k.name.eq_ignore_ascii_case(s.new_kart_name.trim()));
                let ready = s.replace || (!s.new_kart_name.trim().is_empty() && !taken);
                let (l, r) = look::footer(ui, "Back", ("Done", Some(Icon::Check), Kind::Primary, ready));
                if l {
                    s.step = Step::Kart;
                }
                finish |= r;
            }
        });
        if finish {
            let driver = match &s.driver {
                Choice::Existing(id) => Some(*id),
                Choice::New(n) => Some(profiles.driver_named(n)),
                Choice::None => None,
            };
            let kart = driver.and_then(|driver| {
                let mut new_kart = |name: &str| {
                    let id = profiles.next_kart_id();
                    profiles.karts.push(Kart { id, name: name.trim().into(), driver: Some(driver), ..Default::default() });
                    id
                };
                match (&s.kart, s.step) {
                    (Choice::Existing(_), Step::Replace) if !s.replace => Some(new_kart(&s.new_kart_name)),
                    (Choice::Existing(id), _) => Some(*id),
                    (Choice::New(n), _) => Some(new_kart(n)),
                    (Choice::None, _) => None,
                }
            });
            if let Some(kart) = kart {
                // The MyChron it replaces keeps its sessions so far; this one's
                // sessions without a kart become this kart's.
                profiles.mount(&s.logger, kart, &|l: &str| lib.newest_start(l));
                log::info!("MyChron {} set up on kart {kart}", s.logger);
                if let Err(e) = profiles.save() {
                    self.error = Some(format!("Couldn't save: {e}"));
                }
                self.setup_baselines = None;
                return;
            }
        }
        self.library = Some(lib);
        if !cancel && !closed && !finish {
            self.mychron_setup = Some(s);
        }
    }

    /// The two steps for deleting a driver or removing a kart or MyChron.
    pub(super) fn garage_confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut c) = self.garage_confirm.take() else { return };
        let models = self.mychron_models();
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
        let (mut keep, mut done) = (true, false);
        let closed = match &mut c {
            Confirm::DeleteDriver { id, sure, backed_up } => {
                let name = profiles.driver_name(Some(*id)).unwrap_or("this driver").to_string();
                let karts: Vec<u32> = profiles.karts.iter().filter(|k| k.driver == Some(*id)).map(|k| k.id).collect();
                let theirs: Vec<(String, PathBuf)> = lib.sessions.iter().filter(|s| s.kart.is_some_and(|k| karts.contains(&k))).map(|s| (s.logger.clone(), s.path.clone())).collect();
                let mut mychrons: Vec<&String> = theirs.iter().map(|(l, _)| l).collect();
                mychrons.sort();
                mychrons.dedup();
                look::dialog(ctx, "delete_driver", look::WIDTH, |ui| {
                    if !*sure {
                        look::header(ui, Icon::Trash, theme::DANGER, None, &format!("Delete {name}?"), &format!("{name} and everything of theirs goes. Session files go to your computer's Trash."));
                        look::body(ui, |ui| {
                            Frame::new().fill(theme::DANGER.gamma_multiply(0.08)).stroke(Stroke::new(1.0, theme::DANGER.gamma_multiply(0.35))).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(16)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 10.0;
                                look::fact(ui, Icon::User, theme::DANGER, &format!("{name}'s profile"));
                                look::fact(ui, Icon::Flag, theme::DANGER, &plural(karts.len(), "kart"));
                                look::fact(ui, Icon::Mychron, theme::DANGER, &format!("{} recorded on {}", plural(theirs.len(), "session"), plural(mychrons.len(), "MyChron")));
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
                                        if look::big_button(ui, "Back up…", Some(Icon::FolderOpen), Kind::Quiet, !theirs.is_empty()) {
                                            if let Some(into) = rfd::FileDialog::new().set_title("Back up sessions to…").pick_folder() {
                                                *backed_up = Some(back_up(&name, &theirs, &into).map(|p| p.display().to_string()));
                                            }
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
                        let (l, r) = look::footer(ui, "Cancel", (&format!("Delete {name}…"), Some(Icon::Trash), Kind::Danger, true));
                        keep &= !l;
                        *sure |= r;
                    } else {
                        look::header(ui, Icon::TriangleAlert, theme::DANGER, Some("Last step"), "Are you sure?", &format!("{name}, their karts and MyChrons, and {} will be deleted.", plural(theirs.len(), "session")));
                        look::body(ui, |ui| {
                            look::note(ui, "You can only get the session files back from your computer's Trash.", SECONDARY);
                            if !matches!(backed_up, Some(Ok(_))) && !theirs.is_empty() {
                                look::fact(ui, Icon::TriangleAlert, theme::FAVORITE, "You haven't backed up their sessions.");
                            }
                        });
                        let (l, r) = look::footer(ui, "Go back", (&format!("Yes, delete {name}"), Some(Icon::Trash), Kind::Danger, true));
                        if l {
                            *sure = false;
                        }
                        done |= r;
                    }
                })
            }
            Confirm::RemoveKart { id, sure } => {
                let kart = profiles.karts.iter().find(|k| k.id == *id).cloned().unwrap_or_default();
                let sessions = lib.sessions.iter().filter(|s| s.kart == Some(*id)).count();
                look::dialog(ctx, "remove_kart", look::WIDTH, |ui| {
                    if !*sure {
                        look::header(ui, Icon::EyeOff, SECONDARY, None, &format!("Remove {} from the Garage?", kart.name), "Nothing is deleted.");
                        look::body(ui, |ui| {
                            look::fact(ui, Icon::Check, theme::GOOD, &format!("Its {} stay, and still show this kart and its setup", plural(sessions, "session")));
                            if !kart.logger.is_empty() {
                                let m = models.get(&kart.logger).cloned().unwrap_or_else(|| "MyChron".into());
                                look::fact(ui, Icon::EyeOff, SECONDARY, &format!("The {m} on it leaves the Garage with it"));
                            }
                        });
                        let (l, r) = look::footer(ui, "Cancel", ("Remove…", Some(Icon::EyeOff), Kind::Danger, true));
                        keep &= !l;
                        *sure |= r;
                    } else {
                        look::header(ui, Icon::TriangleAlert, theme::DANGER, Some("Last step"), "Are you sure?", &format!("{} won't show in the Garage anymore. Its sessions stay.", kart.name));
                        look::body(ui, |_| {});
                        let (l, r) = look::footer(ui, "Go back", ("Yes, remove it", Some(Icon::EyeOff), Kind::Danger, true));
                        if l {
                            *sure = false;
                        }
                        done |= r;
                    }
                })
            }
            Confirm::RemoveMyChron { logger, sure } => {
                let model = models.get(logger.as_str()).cloned().unwrap_or_else(|| "MyChron".into());
                let kart = profiles.kart_on(logger).map(|k| k.name.clone());
                let sessions = lib.sessions.iter().filter(|s| &s.logger == logger && s.kart.is_some()).count();
                look::dialog(ctx, "remove_mychron", look::WIDTH, |ui| {
                    if !*sure {
                        look::header(ui, Icon::EyeOff, SECONDARY, None, &format!("Remove the {model} from the Garage?"), &format!("Nothing is deleted. (ID {logger})"));
                        look::body(ui, |ui| {
                            let on = kart.as_ref().map(|k| format!(", on {k}")).unwrap_or_default();
                            look::fact(ui, Icon::Check, theme::GOOD, &format!("Its {} stay{on}, and still show this MyChron", plural(sessions, "session")));
                            look::fact(ui, Icon::Mychron, SECONDARY, "If it records more, it's asked about again");
                        });
                        let (l, r) = look::footer(ui, "Cancel", ("Remove…", Some(Icon::EyeOff), Kind::Danger, true));
                        keep &= !l;
                        *sure |= r;
                    } else {
                        look::header(ui, Icon::TriangleAlert, theme::DANGER, Some("Last step"), "Are you sure?", &format!("The {model} won't show in the Garage anymore. Its sessions stay."));
                        look::body(ui, |_| {});
                        let (l, r) = look::footer(ui, "Go back", ("Yes, remove it", Some(Icon::EyeOff), Kind::Danger, true));
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
        self.library = Some(lib);
        if keep && !closed {
            self.garage_confirm = Some(c);
        }
    }

    fn apply_confirm(&mut self, c: Confirm, lib: &Library) {
        let profiles = self.profiles.get_or_insert_with(Profiles::load);
        match c {
            Confirm::DeleteDriver { id, .. } => {
                let name = profiles.driver_name(Some(id)).unwrap_or_default().to_string();
                let karts: Vec<u32> = profiles.karts.iter().filter(|k| k.driver == Some(id)).map(|k| k.id).collect();
                let theirs: Vec<&tracks::LocalSession> = lib.sessions.iter().filter(|s| s.kart.is_some_and(|k| karts.contains(&k))).collect();
                let files: Vec<PathBuf> = theirs.iter().flat_map(|s| session_files(&s.path)).collect();
                let open_is_theirs = self.path.as_ref().is_some_and(|p| theirs.iter().any(|s| &s.path == p));
                profiles.delete_driver(id);
                if !files.is_empty() {
                    if let Err(e) = trash::delete_all(&files) {
                        self.error = Some(format!("{name} was deleted, but some session files couldn't go to the Trash: {e}"));
                    }
                }
                // A MyChron's folder left empty goes too.
                let mut dirs: Vec<&std::path::Path> = theirs.iter().filter_map(|s| s.path.parent()).collect();
                dirs.sort();
                dirs.dedup();
                for d in dirs {
                    if std::fs::read_dir(d).is_ok_and(|mut rd| rd.next().is_none()) {
                        let _ = std::fs::remove_dir(d);
                    }
                }
                log::info!("deleted driver {name}: {} karts, {} sessions ({} files) to the Trash", karts.len(), theirs.len(), files.len());
                if open_is_theirs {
                    self.session = None;
                    self.path = None;
                    self.page = Page::Garage;
                }
                self.kart_page = 0;
            }
            Confirm::RemoveKart { id, .. } => profiles.remove_kart(id),
            Confirm::RemoveMyChron { logger, .. } => profiles.remove_logger(&logger, &|l: &str| lib.newest_start(l)),
        }
        if let Err(e) = self.profiles.as_ref().map(|p| p.save()).unwrap_or(Ok(())) {
            self.error = Some(format!("Couldn't save: {e}"));
        }
        self.library = None;
        self.setup_baselines = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sessions_files_are_everything_under_its_name() {
        let dir = std::env::temp_dir().join(format!("mychronix-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["a_0001.xrz", "a_0001.xri", "a_0001.meta.json", "a_0001-2.xrz", "a_0001-2.xri", "a_0010.xrz"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let mut got: Vec<String> = session_files(&dir.join("a_0001.xrz")).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        got.sort();
        assert_eq!(got, ["a_0001.meta.json", "a_0001.xri", "a_0001.xrz"]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(short_day("2026-12-20 10:00:00"), "Dec 20");
    }
}
