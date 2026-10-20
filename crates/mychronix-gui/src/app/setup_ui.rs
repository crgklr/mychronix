//! Setup sheets in the app: the strip under a session's summary, the editor,
//! and the fields shared with the download dialog.

use super::*;
use crate::setup::{self, Baselines, Setup, Source};

/// A setup sheet being edited for one session.
pub(super) struct SetupEdit {
    pub path: PathBuf,
    pub setup: Setup,
    /// What the session would have without its own sheet.
    pub before: Setup,
    pub has_own: bool,
}

impl App {
    /// The session panel's Setup section: what it was, where it came from,
    /// what changed, and whether RPM and speed agree with the gearing.
    pub(super) fn setup_section(&mut self, ui: &mut Ui) {
        let Some(path) = self.path.clone() else { return };
        let lib = self.library.get_or_insert_with(Library::load);
        let Some(i) = lib.sessions.iter().position(|s| s.path == path) else { return };
        let baselines = self.setup_baselines.get_or_insert_with(Baselines::load);
        let resolved = setup::resolve(lib, baselines);
        let r = &resolved[i];
        let source = match r.source {
            Source::Own => "Recorded for this session".to_string(),
            Source::From(j) => {
                let s = &lib.sessions[j].summary;
                format!(
                    "Same as session {}{}",
                    s.number.unwrap_or(0),
                    s.date.map(|d| format!(" ({} {})", short_month(d.month), d.day)).unwrap_or_default()
                )
            }
            Source::Baseline => "This kart's baseline setup".to_string(),
            Source::Unknown => "No setup recorded yet".to_string(),
        };
        let check = gearing_check(&r.setup, self.measured_ratio);
        let mut edit = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(RichText::new("SETUP").font(semibold(11.5)).color(DIM).extra_letter_spacing(0.8));
            let link = ui.add(egui::Label::new(RichText::new("Edit setup").size(13.0).color(theme::ACCENT)).sense(Sense::click()));
            if link.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            edit = link.clicked();
        });
        ui.add_space(4.0);
        let summary = r.setup.summary();
        ui.add(egui::Label::new(RichText::new(if summary.is_empty() { "No setup recorded".to_string() } else { summary }).size(15.0).color(TEXT)).wrap());
        let mut line = vec![source];
        if !r.changes.is_empty() {
            line.push(format!("Changed: {}", r.changes.join(", ")));
        }
        ui.add(egui::Label::new(RichText::new(line.join("  ·  ")).size(12.5).color(if r.changes.is_empty() { DIM } else { theme::FAVORITE })).wrap());
        if let Some((ok, text)) = check {
            ui.add(egui::Label::new(RichText::new(text).size(12.5).color(if ok { DIM } else { theme::FAVORITE })).wrap());
        }
        if !r.setup.notes.trim().is_empty() {
            ui.add(egui::Label::new(RichText::new(r.setup.notes.trim()).size(13.0).color(SECONDARY)).wrap());
        }
        if edit {
            self.setup_edit = Some(SetupEdit {
                path,
                setup: r.setup.clone(),
                before: setup::before(lib, baselines, i),
                has_own: r.source == Source::Own,
            });
        }
    }

    /// The setup in force for the open session.
    pub(super) fn current_setup(&mut self) -> Option<Setup> {
        let path = self.path.clone()?;
        let lib = self.library.get_or_insert_with(Library::load);
        let i = lib.sessions.iter().position(|s| s.path == path)?;
        let baselines = self.setup_baselines.get_or_insert_with(Baselines::load);
        Some(setup::resolve(lib, baselines).swap_remove(i).setup)
    }

    /// The weather at the track when the session started, with its details on hover.
    pub(super) fn weather_line(&mut self, ui: &mut Ui) {
        let Some(path) = self.path.clone() else { return };
        let lib = self.library.get_or_insert_with(Library::load);
        let Some(w) = lib.sessions.iter().find(|s| s.path == path).and_then(|s| s.weather) else { return };
        ui.label(RichText::new(w.summary()).size(15.0).color(TEXT)).on_hover_text(format!(
            "Weather at the track when the session started (Open-Meteo): {:.0}{u}, {:.0}% humidity, {:.0} hPa air pressure, dew point {:.0}{u}.",
            crate::units::temp(w.air_c),
            w.humidity_pct,
            w.pressure_hpa,
            crate::units::temp(w.dew_point_c),
            u = crate::units::temp_unit()
        ));
    }

    /// When RPM and speed don't fit the setup's gearing: why, for a warning beside the pill.
    pub(super) fn gearing_warning(&mut self) -> Option<String> {
        let path = self.path.clone()?;
        let lib = self.library.get_or_insert_with(Library::load);
        let i = lib.sessions.iter().position(|s| s.path == path)?;
        let baselines = self.setup_baselines.get_or_insert_with(Baselines::load);
        let r = setup::resolve(lib, baselines).swap_remove(i);
        match gearing_check(&r.setup, self.measured_ratio) {
            Some((false, text)) => Some(text),
            _ => None,
        }
    }

    pub(super) fn setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.setup_edit.take() else { return };
        let tires = self.tire_suggestions();
        let (mut save, mut cancel, mut revert) = (false, false, false);
        let closed = garage_manage_ui::look::dialog(ctx, "setup_edit", 760.0, |ui| {
            use garage_manage_ui::look;
            look::header(ui, Icon::Settings, SECONDARY, None, "Setup", "Sessions after this one use this setup until you change it again.");
            look::body(ui, |ui| {
                setup_fields(ui, "setup_edit", &mut edit.setup, &tires, 480.0, true);
                ui.add_space(4.0);
                let changes = edit.setup.changes_from(&edit.before);
                if changes.is_empty() {
                    look::note(ui, "Same as the session before.", DIM);
                } else {
                    look::note(ui, &format!("Changes: {}", changes.join(", ")), theme::FAVORITE);
                }
            });
            look::footer_row(ui, |ui| {
                if edit.has_own && look::big_button(ui, "Use the previous setup", None, look::Kind::Quiet, true) {
                    revert = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if look::big_button(ui, "Save", Some(Icon::Check), look::Kind::Primary, true) {
                        save = true;
                    }
                    if look::big_button(ui, "Cancel", None, look::Kind::Quiet, true) {
                        cancel = true;
                    }
                });
            });
        });
        let open = !closed;
        let result = if revert || (save && edit.setup.same_setup(&edit.before) && edit.setup.notes.trim().is_empty()) {
            // Nothing of its own to keep: inherit again.
            Some(Setup::remove(&edit.path).map(|_| None))
        } else if save {
            Some(edit.setup.save(&edit.path).map(|_| Some(edit.setup.clone())))
        } else {
            None
        };
        match result {
            Some(Ok(sheet)) => {
                if let Some(s) = self.library.as_mut().and_then(|l| l.sessions.iter_mut().find(|s| s.path == edit.path)) {
                    s.setup = sheet;
                }
            }
            Some(Err(e)) => self.error = Some(format!("Couldn't save the setup: {e}")),
            None if open && !cancel => self.setup_edit = Some(edit),
            None => {}
        }
    }

    /// Tires already entered anywhere, for suggestions.
    pub(super) fn tire_suggestions(&mut self) -> Vec<String> {
        let mut v: Vec<String> = self.setup_baselines.get_or_insert_with(Baselines::load).loggers.values().map(|s| s.tires.clone()).collect();
        if let Some(lib) = &self.library {
            v.extend(lib.sessions.iter().filter_map(|s| s.setup.as_ref()).map(|s| s.tires.clone()));
        }
        let mut seen = std::collections::HashSet::new();
        v.retain(|t| !t.trim().is_empty() && seen.insert(t.to_lowercase()));
        v
    }
}

/// The setup fields, as a two-column grid; `notes` adds the session's notes.
/// A tire size: common kart sizes, or typed.
fn size_field(ui: &mut Ui, id: &str, value: &mut String, which: &str) {
    let known = setup::TIRE_SIZES.contains(&value.trim());
    let other = !value.trim().is_empty() && !known;
    egui::ComboBox::from_id_salt(id)
        .width(130.0)
        .selected_text(RichText::new(if value.trim().is_empty() { "Size".to_string() } else if other { "Other".into() } else { value.clone() }).font(regular(15.0)))
        .show_ui(ui, |ui| {
            for s in setup::TIRE_SIZES {
                if ui.selectable_label(value.trim() == s, s).clicked() {
                    *value = s.to_string();
                }
            }
            ui.separator();
            if ui.selectable_label(other, "Other…").clicked() && known {
                value.clear();
                value.push(' ');
            }
        });
    if other || value == " " {
        ui.add(egui::TextEdit::singleline(value).font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_width(100.0).hint_text("e.g. 10x4.60-5"));
    }
    ui.label(RichText::new(which).color(DIM));
}

pub(super) fn setup_fields(ui: &mut Ui, id: &str, s: &mut Setup, tires: &[String], width: f32, notes: bool) {
    let label = |ui: &mut Ui, t: &str| ui.label(RichText::new(t).font(medium(16.0)).color(SECONDARY));
    let small = |ui: &mut Ui, value: &mut String, hint: &str, unit: &str| {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(value).font(regular(16.5)).margin(Margin::symmetric(14, 12)).desired_width(100.0).hint_text(hint));
            ui.label(RichText::new(unit).color(DIM));
        });
    };
    egui::Grid::new(id).num_columns(2).spacing([20.0, 14.0]).show(ui, |ui| {
        label(ui, "Gearing");
        ui.horizontal(|ui| {
            small(ui, &mut s.drive_teeth, "10", "drive");
            ui.add_space(8.0);
            small(ui, &mut s.driven_teeth, "80", "rear teeth");
        });
        ui.end_row();
        label(ui, "Tires");
        suggest_field(ui, &format!("{id}_tires"), &mut s.tires, "Brand and compound, e.g. MG Yellow SM2", tires, width);
        ui.end_row();
        label(ui, "Tire sizes");
        ui.horizontal(|ui| {
            size_field(ui, &format!("{id}_front_size"), &mut s.front_size, "front");
            ui.add_space(8.0);
            size_field(ui, &format!("{id}_rear_size"), &mut s.rear_size, "rear");
        });
        ui.end_row();
        label(ui, "Pressure");
        ui.horizontal(|ui| {
            small(ui, &mut s.front_psi, "10", "front");
            ui.add_space(8.0);
            small(ui, &mut s.rear_psi, "10", "rear psi");
        });
        ui.end_row();
        label(ui, "Weight");
        ui.horizontal(|ui| {
            small(ui, &mut s.weight_lb, "250", "lb with driver");
            ui.add_space(8.0);
            small(ui, &mut s.ballast_lb, "0", "lb ballast");
        });
        ui.end_row();
        label(ui, "Jetting");
        ui.add(egui::TextEdit::singleline(&mut s.jetting).font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_width(width).hint_text("From the IAME Swift app, or main jet / needle"));
        ui.end_row();
        if !notes {
            return;
        }
        label(ui, "Notes");
        ui.add(
            egui::TextEdit::multiline(&mut s.notes)
                .font(regular(15.0))
                .margin(Margin::symmetric(12, 9))
                .desired_width(width)
                .desired_rows(2)
                .hint_text("Why it changed, how it felt (this session only)"),
        );
        ui.end_row();
    });
}

/// Does the recorded gearing match RPM vs speed? (ok, explanation)
fn gearing_check(s: &Setup, measured: Option<f64>) -> Option<(bool, String)> {
    let m = measured?;
    // Shown per mph or per km/h; the check itself works in km/h.
    let per = |rpm_per_kmh: f64| rpm_per_kmh / crate::units::speed(1.0);
    let unit = crate::units::speed_unit();
    match s.expected_rpm_per_kmh() {
        Some(e) => {
            let off = (m - e) / e;
            // One rear tooth is ~1.25% and a tire's rolling size is uncertain by ~2%,
            // so this catches a wrong drive sprocket or tire size, not a single rear tooth.
            if off.abs() < 0.03 {
                Some((true, format!("RPM and speed are consistent with {}/{} ({:.0} rpm per {unit} measured, {:.0} expected for nominal tire size).", s.drive_teeth.trim(), s.driven_teeth.trim(), per(m), per(e))))
            } else {
                let implied = setup::number(&s.driven_teeth).and_then(|d| setup::implied_drive_teeth(m, d, s.rear_tire()));
                let hint = implied.map(|t| format!(" With {}T rear that's a {t:.1}-tooth drive.", s.driven_teeth.trim())).unwrap_or_default();
                Some((false, format!("RPM and speed don't match {}/{}: {:.0} rpm per {unit} measured, {:.0} expected.{hint} Check the sheet.", s.drive_teeth.trim(), s.driven_teeth.trim(), per(m), per(e))))
            }
        }
        None => {
            let implied = setup::number(&s.driven_teeth).and_then(|d| setup::implied_drive_teeth(m, d, s.rear_tire()))?;
            Some((true, format!("RPM and speed suggest a {:.0}-tooth drive sprocket ({implied:.1}) with {}T rear.", implied.round(), s.driven_teeth.trim())))
        }
    }
}

fn short_month(m: u8) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m.clamp(1, 12) - 1) as usize]
}
