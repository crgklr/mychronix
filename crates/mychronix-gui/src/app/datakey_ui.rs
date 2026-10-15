//! The MyChron 4 data key in the app: noticed when it's plugged in, its runs
//! listed and compared with what's already saved, and new ones downloaded
//! (as .mc4, under the logger's serial) with progress.

use super::*;
use aim_link::datakey::{DataKey, RunInfo};

/// What the background key worker reports.
pub(super) enum KeyEvent {
    /// The runs on the key, and which of them aren't saved yet.
    Listed { serial: String, total: usize, new: Vec<RunInfo> },
    Progress { done: usize, of: usize },
    Downloaded { saved: Vec<PathBuf>, serial: String },
    Failed(String),
}

#[derive(Default)]
pub(super) struct KeyState {
    /// Plugged in, as of the last check.
    pub present: bool,
    last_check: Option<std::time::Instant>,
    rx: Option<std::sync::mpsc::Receiver<KeyEvent>>,
    /// What the last listing found: (logger serial, runs on the key, new runs).
    pub listing: Option<(String, usize, Vec<RunInfo>)>,
    pub busy: Option<String>,
    pub progress: Option<(usize, usize)>,
    pub message: Option<(bool, String)>,
}

/// Runs already saved for a logger, by start time ("MM/DD/YYYY", "HH:MM:SS").
fn saved_starts(dir: &std::path::Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "drk" || x == "mc4"))
        .filter_map(|p| if p.extension().is_some_and(|x| x == "drk") { aim_xrk::drk::summary(&p).ok() } else { aim_xrk::mc4::summary(&p).ok() })
        .filter_map(|s| Some((s.date.map(|d| format!("{:02}/{:02}/{}", d.month, d.day, d.year))?, s.start_time?)))
        .collect()
}

fn sessions_dir(serial: &str) -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Mychronix").join("sessions").join(serial)
}

impl App {
    /// Checks for the key about once a second and picks up worker events.
    pub(super) fn key_poll(&mut self, ctx: &egui::Context) {
        let k = &mut self.key;
        while let Some(ev) = k.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match ev {
                KeyEvent::Listed { serial, total, new } => {
                    log::info!("data key: {total} runs, {} new, logger {serial}", new.len());
                    k.listing = Some((serial, total, new));
                    k.busy = None;
                }
                KeyEvent::Progress { done, of } => k.progress = Some((done, of)),
                KeyEvent::Downloaded { saved, serial } => {
                    k.busy = None;
                    k.progress = None;
                    let n = saved.len();
                    k.message = Some((true, format!("Downloaded {n} run{} from the data key. They're on the Sessions page.", if n == 1 { "" } else { "s" })));
                    if let Some((_, _, new)) = k.listing.as_mut() {
                        new.clear();
                    }
                    self.library = None;
                    self.check_downloaded(saved, &serial, ctx);
                    return;
                }
                KeyEvent::Failed(e) => {
                    log::warn!("data key: {e}");
                    k.busy = None;
                    k.progress = None;
                    k.message = Some((false, e));
                }
            }
        }
        let k = &mut self.key;
        if k.busy.is_some() || k.last_check.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            return;
        }
        k.last_check = Some(std::time::Instant::now());
        let present = aim_link::datakey::is_present();
        if present && !k.present {
            log::info!("data key plugged in");
            k.message = None;
            self.key_list(ctx);
        } else if !present && self.key.present {
            log::info!("data key unplugged");
            self.key.listing = None;
        }
        self.key.present = present;
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    /// Lists the key's runs in the background.
    pub(super) fn key_list(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.key.rx = Some(rx);
        self.key.busy = Some("Reading the data key…".into());
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let ev = (|| -> Result<KeyEvent, aim_link::datakey::KeyError> {
                let mut key = DataKey::open()?;
                let runs = key.runs()?;
                let serial = runs.first().map(|r| r.serial.to_string()).unwrap_or_else(|| "mychron4".into());
                let have = saved_starts(&sessions_dir(&serial));
                let new: Vec<RunInfo> = runs.iter().filter(|r| r.start().is_some_and(|s| !have.contains(&s))).cloned().collect();
                Ok(KeyEvent::Listed { serial, total: runs.len(), new })
            })()
            .unwrap_or_else(|e| KeyEvent::Failed(e.to_string()));
            let _ = tx.send(ev);
            ctx.request_repaint();
        });
    }

    /// Downloads the new runs in the background.
    pub(super) fn key_download(&mut self, ctx: &egui::Context) {
        let Some((serial, _, new)) = self.key.listing.clone() else { return };
        if new.is_empty() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.key.rx = Some(rx);
        self.key.busy = Some("Downloading from the data key…".into());
        self.key.progress = Some((0, new.len()));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let dir = sessions_dir(&serial);
            let result = (|| -> Result<Vec<PathBuf>, String> {
                std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
                let mut key = DataKey::open().map_err(|e| e.to_string())?;
                let mut saved = Vec::new();
                for (k, r) in new.iter().enumerate() {
                    let run = key.download(r, |_, _| {}).map_err(|e| e.to_string())?;
                    let path = dir.join(r.file_name());
                    std::fs::write(&path, run.to_file()).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
                    log::info!("data key: saved {}", path.display());
                    saved.push(path);
                    let _ = tx.send(KeyEvent::Progress { done: k + 1, of: new.len() });
                    ctx.request_repaint();
                }
                Ok(saved)
            })();
            let _ = tx.send(match result {
                Ok(saved) => KeyEvent::Downloaded { saved, serial },
                Err(e) => KeyEvent::Failed(e),
            });
            ctx.request_repaint();
        });
    }

    /// The data key panel (Device page): status, new runs, Download.
    pub(super) fn key_panel(&mut self, ui: &mut Ui) {
        if !self.key.present && self.key.message.is_none() {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut download = false;
        Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin::symmetric(20, 16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.add(Icon::Mychron.image(if self.key.present { theme::GOOD } else { SECONDARY }, 34.0));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new("MyChron 4 data key").font(semibold(16.0)).color(TEXT));
                    let line = match (&self.key.busy, &self.key.listing, self.key.present) {
                        (Some(b), _, _) => b.clone(),
                        (None, Some((serial, total, new)), true) => {
                            let who = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).for_logger(serial).driver;
                            let whose = if who.is_empty() { format!("MyChron {serial}") } else { format!("{who}'s MyChron") };
                            match new.len() {
                                0 => format!("Plugged in · {total} runs from {whose}, all downloaded"),
                                n => format!("Plugged in · {total} runs from {whose}, {n} new"),
                            }
                        }
                        (None, _, true) => "Plugged in".into(),
                        (None, _, false) => "Not plugged in".into(),
                    };
                    ui.label(RichText::new(line).size(13.0).color(if self.key.present { theme::GOOD } else { SECONDARY }));
                    if let Some((done, of)) = self.key.progress {
                        ui.add(egui::ProgressBar::new(done as f32 / of.max(1) as f32).desired_width(260.0).text(format!("{done} of {of} runs")));
                    }
                    if let Some((ok, msg)) = &self.key.message {
                        ui.label(RichText::new(msg).size(12.5).color(if *ok { SECONDARY } else { ui.visuals().error_fg_color }));
                    }
                });
                let new = self.key.listing.as_ref().map(|l| l.2.len()).unwrap_or(0);
                if self.key.busy.is_none() && new > 0 {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, Some(Icon::FolderOpen), &format!("Download {new} new run{}", if new == 1 { "" } else { "s" }), true).clicked() {
                            download = true;
                        }
                    });
                }
            });
        });
        ui.add_space(14.0);
        if download {
            self.key_download(&ctx);
        }
    }
}
