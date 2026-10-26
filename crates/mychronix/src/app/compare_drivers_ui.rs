//! Compare's Drivers mode: one driver's best lap against another's at the
//! open session's track, over all race days or one both drove.

use super::*;
use super::sessions_filter_ui::session_driver;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum CompareMode {
    #[default]
    Laps,
    Drivers,
}

#[derive(Default)]
pub(super) struct DriverPick {
    pub a: String,
    pub b: String,
    /// None: all race days at this track.
    pub day: Option<aim_xrk::summary::Date>,
    /// What the compare list was last built from, to rebuild on change.
    built: Option<(String, String, Option<aim_xrk::summary::Date>, Option<usize>)>,
}


impl App {
    /// The drivers with sessions in the library.
    pub(super) fn compare_driver_names(&mut self) -> Vec<String> {
        let lib = self.library.get_or_insert_with(Library::load);
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let mut names: Vec<String> = lib.sessions.iter().map(|s| session_driver(s, &profiles)).filter(|n| !n.is_empty()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// Laps / Drivers, when there's more than one driver.
    pub(super) fn compare_mode_toggle(&mut self, ui: &mut Ui) {
        if self.compare_driver_names().len() < 2 {
            self.comparison.mode = CompareMode::Laps;
            return;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (mode, label) in [(CompareMode::Laps, "Laps"), (CompareMode::Drivers, "Drivers")] {
                if chip(ui, label, self.comparison.mode == mode).clicked() && self.comparison.mode != mode {
                    self.comparison.mode = mode;
                    self.comparison.laps.clear();
                    self.comparison.driver_pick.built = None;
                }
            }
        });
        ui.add_space(10.0);
    }

    /// Driver pickers and race day; rebuilds the compare list from them.
    pub(super) fn compare_drivers_bar(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        let names = self.compare_driver_names();
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let track = self.path.as_ref().and_then(|p| lib.sessions.iter().find(|s| &s.path == p)).and_then(|s| s.track);
        let p = &mut self.comparison.driver_pick;
        // Two drivers: fill them in.
        if p.a.is_empty() || !names.contains(&p.a) {
            p.a = names.first().cloned().unwrap_or_default();
        }
        if p.b.is_empty() || !names.contains(&p.b) || p.b == p.a {
            p.b = names.iter().find(|n| **n != p.a).cloned().unwrap_or_default();
        }
        // Race days at this track where both drove.
        let days_of = |who: &str| -> Vec<aim_xrk::summary::Date> {
            let mut d: Vec<_> = lib.sessions.iter().filter(|s| s.track == track && session_driver(s, &profiles) == who).filter_map(|s| s.summary.date).collect();
            d.sort();
            d.dedup();
            d
        };
        let (da, db) = (days_of(&p.a), days_of(&p.b));
        let both: Vec<_> = da.iter().filter(|d| db.contains(d)).copied().collect();
        if p.day.is_some_and(|d| !both.contains(&d)) {
            p.day = None;
        }
        let track_name = track.map(|t| lib.tracks[t].name.clone()).unwrap_or_else(|| "this track".into());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            egui::ComboBox::from_id_salt("driver_a").width(150.0).selected_text(&p.a).show_ui(ui, |ui| {
                for n in &names {
                    ui.selectable_value(&mut p.a, n.clone(), n);
                }
            });
            ui.label(RichText::new("vs").color(SECONDARY));
            egui::ComboBox::from_id_salt("driver_b").width(150.0).selected_text(&p.b).show_ui(ui, |ui| {
                for n in names.iter().filter(|n| **n != p.a) {
                    ui.selectable_value(&mut p.b, n.clone(), n);
                }
            });
            ui.add_space(12.0);
            ui.label(RichText::new(format!("Best laps at {track_name},")).color(SECONDARY));
            let day_text = p.day.map(crate::events::us_short).unwrap_or_else(|| "all race days".into());
            egui::ComboBox::from_id_salt("driver_day").width(170.0).selected_text(day_text).show_ui(ui, |ui| {
                ui.selectable_value(&mut p.day, None, "All race days");
                for d in both.iter().rev() {
                    ui.selectable_value(&mut p.day, Some(*d), crate::events::us_short(*d));
                }
            });
        });
        ui.add_space(10.0);
        let key = (p.a.clone(), p.b.clone(), p.day, track);
        let rebuild = p.built.as_ref() != Some(&key);
        // Each driver's fastest session here (by its best lap).
        let picks: Vec<PathBuf> = if rebuild {
            let (a, b, day) = (self.comparison.driver_pick.a.clone(), self.comparison.driver_pick.b.clone(), self.comparison.driver_pick.day);
            [a, b]
                .iter()
                .filter_map(|who| {
                    lib.sessions
                        .iter()
                        .filter(|s| track.is_some() && s.track == track && session_driver(s, &profiles) == *who)
                        .filter(|s| day.is_none() || s.summary.date == day)
                        .filter(|s| s.summary.best_lap_ms.is_some())
                        .min_by_key(|s| s.summary.best_lap_ms)
                        .map(|s| s.path.clone())
                })
                .collect()
        } else {
            Vec::new()
        };
        self.library = Some(lib);
        if rebuild {
            self.comparison.driver_pick.built = Some(key);
            self.comparison.laps.clear();
            for path in picks {
                let opened = if self.path.as_ref() == Some(&path) {
                    Some(session.clone())
                } else {
                    crate::jobs::open_session(&path).ok().map(|mut x| {
                        self.fill_names(&mut x, &path);
                        std::sync::Arc::new(x)
                    })
                };
                if let Some(sess) = opened && let Some(lap) = sess.best_lap(MIN_LAP_MS).copied() {
                    self.add_compare_lap(path, sess, lap);
                }
            }
        }
    }

    /// Yellow notes for laps whose speed and distance were worked out from RPM.
    pub(super) fn compare_calculated_notes(&self, ui: &mut Ui) {
        let mut said: Vec<String> = Vec::new();
        for c in &self.comparison.laps {
            if c.session.gps.is_none() && c.session.channel(crate::setup::SPEED_FROM_RPM).is_some() {
                let who = c.session.info.driver.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "One driver".into());
                if !said.contains(&who) {
                    ui.label(
                        RichText::new(format!("{who}'s session uses calculated values (speed and distance from RPM, gearing and tire size), so it may not be precise."))
                            .size(12.5)
                            .color(theme::FAVORITE),
                    );
                    said.push(who);
                }
            }
        }
        if !said.is_empty() {
            ui.add_space(8.0);
        }
    }
}
