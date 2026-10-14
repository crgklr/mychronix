//! The sensor check in the app: a report after downloading (and from the
//! session header) listing sensors that stopped recording, with what to check.

use super::*;
use crate::sensor_check::{self, Checked, History, Ignored};

/// Sensor check state.
#[derive(Default)]
pub(super) struct SensorState {
    /// A check running in the background (after a download).
    pub rx: Option<std::sync::mpsc::Receiver<Vec<Checked>>>,
    /// The report window, when open.
    pub report: Option<Vec<Checked>>,
    pub ignored: Option<Ignored>,
    pub history: Option<History>,
    /// The open session's reportable issues, computed once per session.
    pub open: Option<(PathBuf, Checked)>,
}

impl App {
    /// Checks freshly downloaded sessions in the background.
    pub(super) fn check_downloaded(&mut self, paths: Vec<PathBuf>, logger: &str, ctx: &egui::Context) {
        let known: Vec<PathBuf> = self.library.as_ref().map(|l| l.sessions.iter().filter(|s| s.logger == logger).map(|s| s.path.clone()).collect()).unwrap_or_default();
        self.sensors.rx = Some(sensor_check::spawn(paths, known, ctx.clone()));
        self.sensors.history = None;
    }

    /// The open session's sensor problems (worth reporting and not ignored).
    pub(super) fn open_session_check(&mut self, session: &Session) -> Option<Checked> {
        let path = self.path.clone()?;
        if self.sensors.open.as_ref().is_none_or(|(p, _)| *p != path) {
            let logger = session.info.logger_id.map(|i| i.to_string()).unwrap_or_default();
            let history = self.sensors.history.get_or_insert_with(History::load);
            let issues = aim_xrk::checks::sensor_issues(session)
                .into_iter()
                .filter(|i| aim_xrk::checks::worth_reporting(i, |c| history.worked(&logger, c)))
                .collect();
            let number = path.file_name().and_then(|n| aim_xrk::summary::session_number(&n.to_string_lossy()));
            self.sensors.open = Some((path, Checked { logger, number, issues }));
        }
        let checked = self.sensors.open.as_ref().map(|(_, c)| c.clone())?;
        let ignored = self.sensors.ignored.get_or_insert_with(Ignored::load);
        let issues: Vec<_> = checked.issues.iter().filter(|i| !ignored.contains(&checked.logger, &i.channel)).cloned().collect();
        (!issues.is_empty()).then_some(Checked { issues, ..checked })
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
        let (mut open, mut done) = (true, false);
        let mut ignore: Option<(String, String)> = None;
        egui::Window::new(RichText::new("Sensor check").font(semibold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.label(
                    RichText::new(if sessions == 1 {
                        "A sensor that has worked before on this MyChron didn't record properly in this session.".to_string()
                    } else {
                        "Sensors that have worked before on this MyChron didn't record properly in some of these sessions.".to_string()
                    })
                    .size(13.0)
                    .color(SECONDARY),
                );
                ui.add_space(12.0);
                for p in &problems {
                    ui.horizontal(|ui| {
                        ui.add(Icon::TriangleAlert.image(theme::FAVORITE, 16.0));
                        ui.label(RichText::new(&p.channel).font(semibold(15.0)).color(TEXT));
                    });
                    ui.label(RichText::new(sensor_check::summary(p)).size(13.5).color(TEXT));
                    ui.label(RichText::new(aim_xrk::checks::advice(&p.channel)).size(12.5).color(SECONDARY));
                    let link = ui.add(egui::Label::new(RichText::new(format!("Don't warn about {} on this MyChron", p.channel)).size(12.5).color(DIM).underline()).sense(Sense::click()));
                    if link.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if link.clicked() {
                        ignore = Some((p.logger.clone(), p.channel.clone()));
                    }
                    ui.add_space(12.0);
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("The sessions are saved either way; only those channels are affected.").size(12.0).color(DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, Some(Icon::Check), "OK", true).clicked() {
                            done = true;
                        }
                    });
                });
            });
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
