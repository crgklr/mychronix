//! The sensor check in the app: a report after downloading (and from the
//! session header) listing sensors that stopped recording, with what to check.

use super::*;
use crate::digest::{Digest, Digests};
use crate::sensor_check::{self, Checked, Ignored};

/// Sensor check state.
#[derive(Default)]
pub(super) struct SensorState {
    /// A check running in the background (after a download).
    pub rx: Option<std::sync::mpsc::Receiver<Vec<Checked>>>,
    /// The report window, when open.
    pub report: Option<Vec<Checked>>,
    pub ignored: Option<Ignored>,
    /// Session digests (lap times, sensors), loaded once and filled in the background.
    pub digests: Option<Digests>,
    pub digest_rx: Option<std::sync::mpsc::Receiver<(PathBuf, Digest)>>,
    /// Sessions already looked up (digest known, or asked for), so the pages
    /// that want digests on every frame don't touch the disk again for them.
    pub digests_asked: std::collections::HashSet<PathBuf>,
    /// Weather being fetched, and the sessions already asked about this run.
    pub weather_rx: Option<std::sync::mpsc::Receiver<(PathBuf, crate::weather::Weather)>>,
    pub weather_asked: std::collections::HashSet<PathBuf>,
    /// The open session's reportable issues, computed once per session.
    pub open: Option<(PathBuf, Checked)>,
    /// The open session's notes (lap trend, consistency, temperatures).
    pub notes: Option<(PathBuf, Vec<aim_xrk::notes::Note>)>,
}

impl App {
    /// Checks freshly downloaded sessions in the background.
    pub(super) fn check_downloaded(&mut self, paths: Vec<PathBuf>, logger: &str, ctx: &egui::Context) {
        let known: Vec<PathBuf> = self.library.as_ref().map(|l| l.sessions.iter().filter(|s| s.logger == logger).map(|s| s.path.clone()).collect()).unwrap_or_default();
        self.sensors.rx = Some(sensor_check::spawn(paths, known, ctx.clone()));
        // Reload the digests once the check has added the new sessions.
        self.sensors.digests = None;
        self.sensors.digests_asked.clear();
    }

    /// The open session's sensor problems (worth reporting and not ignored).
    pub(super) fn open_session_check(&mut self, session: &Session) -> Option<Checked> {
        let path = self.path.clone()?;
        if self.sensors.open.as_ref().is_none_or(|(p, _)| *p != path) {
            let digests = self.sensors.digests.get_or_insert_with(Digests::load);
            let digest = digests.get(&path).cloned().unwrap_or_else(|| Digest::of(session));
            let logger = digest.logger.clone();
            let issues = digests.reportable(&digest);
            let number = path.file_name().and_then(|n| aim_xrk::summary::session_number(&n.to_string_lossy()));
            self.sensors.open = Some((path, Checked { logger, number, issues }));
        }
        let checked = self.sensors.open.as_ref().map(|(_, c)| c.clone())?;
        let ignored = self.sensors.ignored.get_or_insert_with(Ignored::load);
        let issues: Vec<_> = checked.issues.iter().filter(|i| !ignored.contains(&checked.logger, &i.channel)).cloned().collect();
        (!issues.is_empty()).then_some(Checked { issues, ..checked })
    }

    /// Fetches the weather for library sessions `rows` that don't have it yet,
    /// and puts arrived weather into the library.
    pub(super) fn want_weather(&mut self, rows: &[usize], ctx: &egui::Context) {
        let Some(lib) = self.library.as_mut() else { return };
        if let Some(rx) = &self.sensors.weather_rx {
            loop {
                match rx.try_recv() {
                    Ok((p, w)) => {
                        if let Some(s) = lib.sessions.iter_mut().find(|s| s.path == p) {
                            s.weather = Some(w);
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.sensors.weather_rx = None;
                        break;
                    }
                }
            }
        }
        if self.sensors.weather_rx.is_some() || !crate::weather::enabled() {
            return;
        }
        let wants: Vec<crate::weather::Want> = rows
            .iter()
            .filter_map(|&i| {
                let s = lib.sessions.get(i)?;
                if s.weather.is_some() || self.sensors.weather_asked.contains(&s.path) {
                    return None;
                }
                let pos = s.summary.start_finish.or_else(|| s.track.and_then(|t| lib.tracks[t].start_finish).map(|p| (p.lat, p.lon)))?;
                let date = s.summary.date.map(crate::events::iso)?;
                let hour = s.summary.start_time.as_deref().and_then(|t| t.get(..2)).and_then(|h| h.parse().ok()).unwrap_or(12usize).min(23);
                Some(crate::weather::Want { session: s.path.clone(), lat: pos.0, lon: pos.1, date, hour })
            })
            .collect();
        if !wants.is_empty() {
            self.sensors.weather_asked.extend(wants.iter().map(|w| w.session.clone()));
            self.sensors.weather_rx = Some(crate::weather::spawn(wants, ctx.clone()));
        }
    }

    /// Asks for the digests of `paths` not yet known; picks up finished ones.
    pub(super) fn want_digests(&mut self, paths: impl Iterator<Item = PathBuf>, ctx: &egui::Context) {
        let digests = self.sensors.digests.get_or_insert_with(Digests::load);
        if let Some(rx) = &self.sensors.digest_rx {
            loop {
                match rx.try_recv() {
                    Ok((p, d)) => digests.insert(&p, d),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.sensors.digest_rx = None;
                        break;
                    }
                }
            }
        }
        // Only sessions not seen before: looking one up reads its file's size.
        let mut need = Vec::new();
        for p in paths {
            if self.sensors.digests_asked.contains(&p) {
                continue;
            }
            if digests.get(&p).is_none() {
                need.push(p);
            } else {
                self.sensors.digests_asked.insert(p);
            }
        }
        if !need.is_empty() && self.sensors.digest_rx.is_none() {
            self.sensors.digests_asked.extend(need.iter().cloned());
            self.sensors.digest_rx = Some(crate::digest::spawn(need, ctx.clone()));
        }
    }

    /// The open session's notes, as (short text, explanation, needs attention).
    pub(super) fn open_session_notes(&mut self, session: &Session) -> Vec<(String, String, bool)> {
        use aim_xrk::notes::Note;
        let Some(path) = self.path.clone() else { return Vec::new() };
        if self.sensors.notes.as_ref().is_none_or(|(p, _)| *p != path) {
            self.sensors.notes = Some((path, aim_xrk::notes::session_notes(session)));
        }
        let notes = self.sensors.notes.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
        // The open session's channels are already in display units.
        let deg = |u: &str| if u.to_uppercase().contains('F') { "°F" } else { "°C" };
        notes
            .iter()
            .map(|n| match n {
                Note::LapTrend { per_lap_s, first_lap, last_lap } if *per_lap_s > 0.0 => (
                    format!("Laps slowed ~{per_lap_s:.2} s each"),
                    format!("Over the racing laps ({first_lap}–{last_lap}), lap times rose steadily by about {per_lap_s:.2} s a lap. Tires going off, the driver tiring, traffic, or a change in conditions."),
                    true,
                ),
                Note::LapTrend { per_lap_s, first_lap, last_lap } => (
                    format!("Laps got faster ~{:.2} s each", -per_lap_s),
                    format!("Over the racing laps ({first_lap}–{last_lap}), lap times fell steadily by about {:.2} s a lap: finding speed as the session went on.", -per_lap_s),
                    false,
                ),
                Note::Consistency { sd_s, laps } => (
                    format!("Lap times ±{sd_s:.2} s"),
                    format!("How much the {laps} racing laps varied (standard deviation; out/in laps and laps over 115% of the median left out). Smaller is more consistent."),
                    false,
                ),
                Note::TempTrend { channel, units, from, to, per_lap } => {
                    let name = crate::report::short_name(channel);
                    let word = if *per_lap > 0.0 { "rose" } else { "fell" };
                    let u = deg(units);
                    (
                        format!("{name} {word} {:.0}{u} over the session", (to - from).abs()),
                        format!(
                            "{name}'s peak each lap {word} steadily, from about {from:.0}{u} to {to:.0}{u} across the racing laps. {}",
                            if name == "EGT" {
                                "Rising exhaust temperature can come from a leaner mixture, warmer air, or the engine still coming up to temperature; worth comparing with the jetting on the setup sheet."
                            } else {
                                "Normal while warming up; if it keeps climbing late in a session, check cooling."
                            }
                        ),
                        *per_lap > 0.0 && name == "EGT",
                    )
                }
            })
            .collect()
    }

    /// The report window; also picks up a finished background check.
    pub(super) fn sensor_dialog(&mut self, ctx: &egui::Context) {
        if let Some(checked) = self.sensors.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.sensors.rx = None;
            let ignored = self.sensors.ignored.get_or_insert_with(Ignored::load);
            if !sensor_check::problems(&checked, ignored).is_empty() {
                self.sensors.report = Some(checked);
            }
        }
        let Some(checked) = self.sensors.report.take() else { return };
        let ignored = self.sensors.ignored.get_or_insert_with(Ignored::load);
        let problems = sensor_check::problems(&checked, ignored);
        let sessions = checked.len();
        let mut done = false;
        let mut ignore: Option<(String, String)> = None;
        let closed = garage_manage_ui::look::dialog(ctx, "sensor_check", garage_manage_ui::look::WIDTH, |ui| {
            use garage_manage_ui::look;
            let (title, sub) = if sessions == 1 {
                ("A sensor stopped recording", "A sensor that has worked before on this MyChron didn't record properly in this session.")
            } else {
                ("Sensors stopped recording", "Sensors that have worked before on this MyChron didn't record properly in some of these sessions.")
            };
            look::header(ui, Icon::TriangleAlert, theme::FAVORITE, None, title, sub);
            look::body(ui, |ui| {
                for (k, p) in problems.iter().enumerate() {
                    if k > 0 {
                        ui.add_space(10.0);
                    }
                    ui.label(RichText::new(&p.channel).font(semibold(18.0)).color(TEXT));
                    ui.add(egui::Label::new(RichText::new(sensor_check::summary(p)).font(regular(15.5)).color(TEXT)).wrap());
                    ui.add(egui::Label::new(RichText::new(aim_xrk::checks::advice(&p.channel)).font(regular(14.5)).color(SECONDARY)).wrap());
                    let link = ui.add(egui::Label::new(RichText::new(format!("Don't warn about {} on this MyChron", p.channel)).font(regular(14.5)).color(theme::ACCENT)).sense(Sense::click()));
                    if link.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if link.clicked() {
                        ignore = Some((p.logger.clone(), p.channel.clone()));
                    }
                }
                ui.add_space(6.0);
                look::note(ui, "The sessions are saved either way; only those channels are affected.", DIM);
            });
            look::footer_row(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if look::big_button(ui, "OK", Some(Icon::Check), look::Kind::Primary, true) {
                        done = true;
                    }
                });
            });
        });
        let open = !closed;
        if let Some((logger, channel)) = ignore {
            log::info!("sensor check: not warning about {channel} on logger {logger}");
            let ignored = self.sensors.ignored.get_or_insert_with(Ignored::load);
            ignored.set(&logger, &channel, true);
            if let Err(e) = ignored.save() {
                self.error = Some(format!("Couldn't save that: {e}"));
            }
        }
        let still = !sensor_check::problems(&checked, self.sensors.ignored.get_or_insert_with(Ignored::load)).is_empty();
        if open && !done && still {
            self.sensors.report = Some(checked);
        }
    }
}
