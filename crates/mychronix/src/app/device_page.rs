//! The Device page: the MyChron over WiFi (scanning, joining, its
//! sessions, the download dialog) and the setup card for a new MyChron.

use super::*;

/// The Device page's state: the MyChron found (over WiFi or USB), its
/// session list, what's picked, and a download under way.
#[derive(Default)]
pub(super) struct DeviceState {
    pub(super) pending_setup: Option<(PathBuf, crate::setup::Setup)>,
    pub(super) worker: Option<crate::device::Worker>,
    pub(super) logger: Option<aim_link::Beacon>,
    pub(super) networks: Vec<(String, u8)>,
    pub(super) current_wifi: Option<String>,
    /// The AiM network Mychronix joined (so it can leave it).
    pub(super) joined: Option<String>,
    /// The connected MyChron is plugged in by USB rather than on WiFi.
    pub(super) usb: bool,
    /// Linux: an AiM USB device is plugged in that Mychronix may not open
    /// (checked now and then), and the answer to "Allow USB access" while it runs.
    pub(super) usb_blocked: bool,
    pub(super) usb_checked: f64,
    pub(super) usb_allow: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// When the WiFi was last checked from another page.
    pub(super) wifi_check: f64,
    pub(super) files: Vec<aim_link::DirEntry>,
    pub(super) summaries: Vec<aim_xrk::SessionSummary>,
    pub(super) selected: std::collections::BTreeSet<String>,
    /// What the worker is doing, shown while busy.
    pub(super) busy: Option<String>,
    pub(super) progress: Option<(usize, usize, String, u64, u64)>,
    /// A setup typed in the download window whose download failed: the next one starts from it.
    pub(super) unsaved_setup: Option<crate::setup::Setup>,
    pub(super) last_scan: f64,
    /// A scan is with the worker: don't queue another (one takes several seconds).
    pub(super) scanning: bool,
    /// The download dialog, while open.
    pub(super) dialog: Option<DownloadDialog>,
    /// The connected MyChron's custom tracks, as of its last listing.
    pub(super) tracks: crate::device::MyChronTracks,
}

impl App {
    /// Applies what the MyChron worker has finished and keeps scanning for a
    /// logger (the Device page and the Garage both show its status).
    pub(super) fn device_poll(&mut self, ctx: &egui::Context) {
        use crate::device::{Command, Event};
        let ctx = ctx.clone();
        let worker = self.device.worker.get_or_insert_with(|| crate::device::Worker::spawn(ctx.clone()));

        // Apply whatever the worker has finished.
        let mut events = Vec::new();
        let mut worker_died = false;
        loop {
            match worker.rx.try_recv() {
                Ok(e) => events.push(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    worker_died = true;
                    break;
                }
            }
        }
        // The worker stopped (it crashed; see crash.log beside the log): start a new one.
        if worker_died {
            self.device.worker = None;
            self.device.busy = None;
            self.device.scanning = false;
            self.device.progress = None;
            self.notices.error("The connection to the MyChron stopped unexpectedly and has been restarted. Try again.");
            return;
        }
        for e in events {
            match e {
                Event::Scanned { logger, usb, networks, current } => {
                    self.device.scanning = false;
                    // Switched to another network some other way (e.g. GNOME's menu):
                    // undo the pause on the other networks' auto-connect.
                    if self.device.joined.is_some() && current.as_deref().is_some_and(|c| !c.starts_with("AiM-")) {
                        let _ = self.device.worker.as_ref().unwrap().tx.send(Command::Leave);
                        self.notices.success(format!("You're back on {}. Your networks will connect automatically again.", current.clone().unwrap_or_default()));
                    }
                    // Newly connected, or moved between USB and WiFi: read the list again.
                    let newly = logger.is_some() && (self.device.logger.is_none() || usb != self.device.usb);
                    if logger.is_none() {
                        self.device.tracks = Default::default();
                    }
                    self.device.logger = logger;
                    self.device.usb = usb;
                    self.device.networks = networks;
                    self.device.current_wifi = current;
                    if self.device.busy.as_deref() == Some("Looking for your MyChron…") {
                        self.device.busy = None;
                    }
                    // Remember which MyChron model this logger is, from its WiFi name.
                    if let (Some(id), Some(ssid)) = (self.device.logger.as_ref().and_then(|b| b.logger_id), self.device.current_wifi.as_deref().filter(|_| !usb)) {
                        let model = model_from_ssid(ssid);
                        if model.starts_with("MyChron") {
                            let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
                            let entry = profiles.loggers.entry(id.to_string()).or_default();
                            if entry.model != model {
                                entry.model = model.to_string();
                                let _ = profiles.save();
                            }
                        }
                    }
                    if newly {
                        self.device.busy = Some("Reading the session list…".into());
                        let serial = self.device.logger.as_ref().and_then(|b| b.logger_id);
                        let dir = self.logger_dir();
                        let _ = self.device.worker.as_ref().unwrap().tx.send(Command::List { serial, dir });
                    }
                }
                Event::Joined { ssid } => {
                    self.device.joined = Some(ssid);
                    self.device.busy = Some("Looking for your MyChron…".into());
                    self.device.scanning = true;
                    let _ = self.device.worker.as_ref().unwrap().tx.send(Command::Scan);
                }
                Event::Left => {
                    self.device.joined = None;
                    self.device.logger = None;
                    self.device.tracks = Default::default();
                    self.device.files.clear();
                    self.device.summaries.clear();
                    self.device.busy = None;
                }
                Event::Listed { files, summaries, tracks } => {
                    let new_tracks = tracks.copied();
                    self.device.tracks = tracks;
                    self.device.files = files;
                    self.device.summaries = summaries;
                    self.device.busy = None;
                    if new_tracks > 0 {
                        self.reload_library(); // pick up the new custom tracks
                        self.notices.success(format!("Copied {new_tracks} custom track{} from the MyChron into your track library.", if new_tracks == 1 { "" } else { "s" }));
                    }
                    // New sessions are picked, except engine warm-ups (as RaceStudio does); All new picks those too.
                    let warm_up = |name: &str| self.device.summaries.iter().find(|s| s.file.as_deref() == Some(name)).is_some_and(listed_warm_up);
                    self.device.selected = self.new_on_logger().into_iter().filter(|e| !warm_up(&e.name)).map(|e| e.name).collect();
                    // Why each session counts as new or already downloaded.
                    let dir = self.logger_dir();
                    let sessions: Vec<_> = self.device.files.iter().filter(|e| e.is_session()).collect();
                    log::info!("{} of {} sessions on the logger are new (compared with {:?})", self.device.selected.len(), sessions.len(), dir);
                    for e in sessions {
                        let local = dir.as_ref().and_then(|d| std::fs::metadata(d.join(&e.name)).ok()).map(|m| m.len());
                        log::debug!("  {}: logger {} bytes, here {:?} → {}", e.name, e.size, local, if local == Some(e.size) { "downloaded" } else { "new" });
                    }
                }
                Event::Progress { index, count, file, done, total } => {
                    self.device.progress = Some((index, count, file, done, total));
                }
                Event::Downloaded { saved } => {
                    // Replaced sessions' saved lap summaries were dropped: don't save old ones back.
                    self.sensors.digests = None;
                    self.sensors.digests_asked.clear();
                    self.device.unsaved_setup = None;
                    if let Some((path, setup)) = self.device.pending_setup.take() {
                        // A session's own sheet is only changed from its Setup.
                        if crate::setup::Setup::path_for(&path).exists() {
                            self.notices.error("That session already has a setup, so the one entered here wasn't saved. Open the session and use Setup to change it.");
                        } else if let Err(e) = setup.save(&path) {
                            self.notices.error(format!("Couldn't save the setup: {e}"));
                        } else if let Some(id) = self.device.logger.as_ref().and_then(|b| b.logger_id) {
                            // The kart's next setup is now that session's.
                            let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
                            if let Some(k) = profiles.karts.iter_mut().find(|k| k.logger == id.to_string() && k.next_setup.is_some()) {
                                k.next_setup = None;
                                if let Err(e) = profiles.save() {
                                    self.notices.error(format!("Couldn't save: {e}"));
                                }
                            }
                        }
                    }
                    self.device.busy = None;
                    self.device.progress = None;
                    let mut done = format!("Downloaded {}.", plural(saved.len(), "session"));
                    // Check the new sessions' sensors in the background: the files as saved.
                    if let Some(id) = self.device.logger.as_ref().and_then(|b| b.logger_id) {
                        self.check_downloaded(saved, &id.to_string(), &ctx);
                    }
                    self.reload_library(); // reload sessions and tracks
                    self.device.selected.clear();
                    // Done with the MyChron: back to the computer's own WiFi.
                    if self.device.joined.is_some() {
                        log::info!("download finished: leaving the MyChron's WiFi");
                        self.device.busy = Some("Reconnecting to your network…".into());
                        let _ = self.device.worker.as_ref().unwrap().tx.send(Command::Leave);
                        done.push_str(" Mychronix is reconnecting you to your WiFi.");
                    } else if cfg!(target_os = "macos") && self.device.current_wifi.as_deref().is_some_and(|w| w.starts_with("AiM-")) {
                        done.push_str(" You can switch back to your WiFi from the menu bar.");
                    }
                    self.notices.success(done).go_to("Open Sessions", Page::Sessions);
                }
                Event::Error(msg) => {
                    // A setup typed for a download that failed is offered again next time.
                    if let Some((_, setup)) = self.device.pending_setup.take() {
                        self.device.unsaved_setup = Some(setup);
                    }
                    self.device.busy = None;
                    self.device.progress = None;
                    self.notices.error(msg);
                }
            }
        }

        // MYCHRONIX_FAKE_DEVICE=<logger id> shows that logger's downloaded sessions as if it were
        // connected, for checking this page's layout without the MyChron (development only).
        if self.device.logger.is_none() && let Some(id) = crate::hooks::get().fake_device {
            let mut raw = vec![0u8; 236];
            raw[0] = 0xec;
            raw[84..87].copy_from_slice(b"idn");
            raw[96..100].copy_from_slice(&id.to_le_bytes());
            self.device.logger = aim_link::Beacon::parse(AP_ADDRESS_FAKE, std::time::Duration::from_millis(3), raw).ok();
            self.device.current_wifi = Some(format!("AiM-MYC6-{:06}", id % 1_000_000));
            if let Some(dir) = self.logger_dir() {
                let mut files = Vec::new();
                let mut summaries = Vec::new();
                for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    let p = e.path();
                    let name = p.file_name().unwrap().to_string_lossy().into_owned();
                    if name.ends_with(".xrz") {
                        // Listed as the download noted it, so saved sessions read as downloaded.
                        let listed = crate::device::SessionMeta::load(&p).map(|m| m.listed).unwrap_or_default();
                        let (date, time) = listed.split_once(' ').map(|(d, t)| (d.to_string(), t.to_string())).unwrap_or_default();
                        files.push(aim_link::DirEntry { name: name.clone(), size: e.metadata().map(|m| m.len()).unwrap_or(0), date, time });
                        if let Some(mut sum) = std::fs::read(p.with_extension("xri")).ok().and_then(|b| aim_xrk::parse_xri(&b).ok()) {
                            sum.file = Some(name);
                            summaries.push(sum);
                        }
                    }
                }
                // Pretend the newest two aren't downloaded yet.
                files.sort_by(|a, b| a.name.cmp(&b.name));
                if let Some(last) = files.last_mut() {
                    last.size += 1;
                }
                self.device.files = files;
                self.device.summaries = summaries;
                let warm_up = |name: &str| self.device.summaries.iter().find(|s| s.file.as_deref() == Some(name)).is_some_and(listed_warm_up);
                self.device.selected = self.new_on_logger().into_iter().filter(|e| !warm_up(&e.name)).map(|e| e.name).collect();
                // Its custom tracks: the ones on this computer.
                let custom = crate::paths::data("tracks/custom").and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten();
                let names = custom.filter_map(|e| aim_track::read_tkk(e.path()).ok()).map(|t| (t.name, crate::device::TrackCopy::Here));
                self.device.tracks = crate::device::MyChronTracks { custom: names.collect() };
            }
            self.device.last_scan = f64::MAX;
        }

        // Keep looking while nothing is connected; check now and then that a logger is still there.
        let now = ctx.input(|i| i.time);
        // An AiM device Linux won't let Mychronix use (no USB rule yet).
        if now - self.device.usb_checked > 2.0 {
            self.device.usb_checked = now;
            self.device.usb_blocked = crate::usb_access::blocked() || crate::hooks::get().fake_usb_blocked;
        }
        if let Some(rx) = &self.device.usb_allow && let Ok(result) = rx.try_recv() {
            self.device.usb_allow = None;
            match result {
                Ok(()) => {
                    self.device.usb_blocked = false;
                    self.device.usb_checked = now;
                    self.notices.success("USB access allowed. Your MyChron will show up in a moment.");
                    // Look again now rather than at the next scan.
                    self.device.last_scan = 0.0;
                }
                Err(e) => {
                    self.notices.error(e);
                }
            }
        }
        let every = if self.device.logger.is_some() { 15.0 } else { 4.0 };
        if self.device.busy.is_none() && !self.device.scanning && self.device.last_scan != f64::MAX && now - self.device.last_scan > every {
            self.device.last_scan = now;
            self.device.scanning = true;
            let _ = self.device.worker.as_ref().unwrap().tx.send(Command::Scan);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    pub(super) fn device_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.device_poll(&ctx);


        ui.label(RichText::new("Device").font(semibold(28.0)).color(TEXT));
        ui.label(RichText::new("Your MyChrons, how to download from them, and their sessions.").font(regular(14.5)).color(SECONDARY));
        ui.add_space(18.0);


        if crate::hooks::get().fake_dialog.is_some() && self.device.dialog.is_none() && self.frames < 3 && self.device.logger.is_some() {
            let multi = crate::hooks::get().fake_dialog.as_deref() == Some("multi");
            let id = self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
            let p = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).for_logger(&id);
            self.device.dialog = Some(DownloadDialog {
                meta: crate::device::SessionMeta {
                    driver: p.driver,
                    kart: p.kart,
                    championship: p.championship,
                    track: if multi { String::new() } else { "Club Circuit".into() },
                    ..Default::default()
                },
                detected: if multi { vec!["Club North".into(), "Club Circuit".into()] } else { vec!["Club Circuit".into()] },
                setup: self.profiles.get_or_insert_with(crate::profiles::Profiles::load).karts.iter().find(|k| k.logger == id).and_then(|k| k.next_setup.clone()).unwrap_or_else(|| self.latest_setup(&id)),
                setup_before: self.latest_setup(&id),
                setup_open: false,
                replace: true,
                setup_target: None,
                adding_driver: false,
            });
        }
        // MYCHRONIX_ADD_MYCHRON=wifi|key opens the "Add a MyChron" window at its steps (for testing).
        if self.frames < 3 && self.add_mychron.is_none() {
            match crate::hooks::get().add_mychron.as_deref() {
                Some("wifi") => self.open_add_mychron(Some(device_ui::Way::Wifi), "MyChron 6"),
                Some("key") => self.open_add_mychron(Some(device_ui::Way::DataKey), "MyChron 4"),
                Some(_) => self.open_add_mychron(None, ""),
                None => {}
            }
        }
        self.usb_access_card(ui);
        self.unplaced_connected(ui);
        self.key_panel(ui);
        if self.device.logger.is_some() {
            self.device_status(ui);
            ui.add_space(14.0);
            self.device_sessions(ui);
        } else {
            self.device_guide(ui);
        }
        self.download_dialog(&ctx);
        if self.device.dialog.is_none() {
            self.add_mychron_dialog(&ctx);
        }
        self.mychron_setup_dialog(&ctx);
    }

    /// The Garage's "New MyChron detected" card, here too, for a connected
    /// MyChron (WiFi or data key) that isn't on a kart yet.
    fn unplaced_connected(&mut self, ui: &mut Ui) {
        let lib = self.lib();
        let profiles = self.profiles.take().unwrap_or_else(crate::profiles::Profiles::load);
        let connected: Vec<String> = self
            .device.logger
            .as_ref()
            .and_then(|b| b.logger_id)
            .map(|i| i.to_string())
            .into_iter()
            .chain(self.key.listing.as_ref().filter(|_| self.key.present).map(|(s, ..)| s.clone()))
            .collect();
        let wifi_id = self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string());
        let wifi_model = self.device.current_wifi.as_deref().map(model_from_ssid).filter(|m| m.starts_with("MyChron"));
        let cards: Vec<_> = self
            .unplaced_mychrons(&lib, &profiles)
            .into_iter()
            .filter(|u| connected.contains(&u.logger))
            .map(|u| {
                let model = Some(profiles.for_logger(&u.logger).model)
                    .filter(|m| !m.is_empty())
                    .or_else(|| wifi_model.filter(|_| wifi_id.as_deref() == Some(u.logger.as_str())).map(String::from))
                    .or_else(|| lib.sessions.iter().filter(|s| s.logger == u.logger).find_map(|s| s.summary.model.clone()))
                    .unwrap_or_else(|| "MyChron".into());
                (u, model)
            })
            .collect();
        self.profiles = Some(profiles);
        for (u, model) in cards {
            if self.unplaced_card(ui, &u, &model) {
                self.snooze(&u);
            }
            ui.add_space(14.0);
        }
    }

    /// Linux only: an AiM device is plugged in, but there's no rule letting
    /// Mychronix use it. One button installs the rule (the computer asks for
    /// an administrator's password once).
    fn usb_access_card(&mut self, ui: &mut Ui) {
        if !self.device.usb_blocked {
            return;
        }
        let asking = self.device.usb_allow.is_some();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(Icon::Usb.image(theme::FAVORITE, 30.0));
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    // Leave room for the button.
                    ui.set_max_width((ui.available_width() - 270.0).max(240.0));
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new("A MyChron is plugged in. Linux needs your OK first").font(semibold(17.0)).color(TEXT));
                    ui.add(
                        egui::Label::new(
                            RichText::new("Linux lets an app use a USB device only when a rule allows it. This adds AiM's rule, once, so Mychronix can talk to MyChrons and the MyChron 4 data key. Your computer will ask for your password.")
                                .size(13.5)
                                .color(SECONDARY),
                        )
                        .wrap(),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if asking { "Waiting for your password…" } else { "Allow USB access" };
                    if look::big_button(ui, label, Some(Icon::Check), look::Kind::Primary, !asking) {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let ctx = ui.ctx().clone();
                        crate::jobs::spawn("usb-access", move || {
                            let _ = tx.send(crate::usb_access::allow());
                            ctx.request_repaint();
                        });
                        self.device.usb_allow = Some(rx);
                    }
                });
            });
        });
        ui.add_space(14.0);
    }

    fn logger_dir(&self) -> Option<PathBuf> {
        let id = self.device.logger.as_ref()?.logger_id?;
        Some(crate::paths::sessions_dir()?.join(id.to_string()))
    }

    /// Sessions on the logger not yet saved here (see `device::downloaded`).
    pub(super) fn new_on_logger(&self) -> Vec<aim_link::DirEntry> {
        let dir = self.logger_dir();
        self.device.files
            .iter()
            .filter(|e| e.is_session() && !crate::device::listed_twice(&self.device.files, e))
            .filter(|e| !dir.as_ref().is_some_and(|d| crate::device::downloaded(d, e).is_some()))
            .cloned()
            .collect()
    }

    /// The session picked for download that a setup entered with them is
    /// saved on, with its start (see `device::setup_target`); None when
    /// none of them is newer than what's here.
    fn download_setup_target(&self, lib: &Library) -> Option<(PathBuf, String)> {
        let dir = self.logger_dir()?;
        let id = self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
        let tz = jiff::tz::TimeZone::system();
        let picked: Vec<(aim_link::DirEntry, String)> = self
            .device.files
            .iter()
            .filter(|e| self.device.selected.contains(&e.name))
            .map(|e| (e.clone(), crate::device::start_of(e, self.device.summaries.iter().find(|s| s.file.as_deref() == Some(e.name.as_str())), &tz)))
            .collect();
        crate::device::setup_target(&dir, &picked, &lib.newest_start(&id))
    }

    pub(super) fn device_status(&mut self, ui: &mut Ui) {
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        card(ui, |ui| {
            match &self.device.logger {
                Some(b) => {
                    let id = b.logger_id.map(|i| i.to_string()).unwrap_or_default();
                    // On USB there's no network name to tell the model: what the Garage knows.
                    let known = profiles.get(&id).map(|p| p.model.clone()).filter(|m| !m.is_empty());
                    let model = match (self.device.usb, known) {
                        (true, Some(m)) => m,
                        (true, None) => "MyChron".to_string(),
                        (false, _) => self.device.joined.as_deref().or(self.device.current_wifi.as_deref()).map(model_from_ssid).unwrap_or("MyChron").to_string(),
                    };
                    let who = profiles.get(&id).map(|p| p.driver.clone()).filter(|d| !d.is_empty());
                    ui.horizontal(|ui| {
                        ui.add(Icon::Mychron.image(theme::GOOD, 32.0));
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(format!("{model} connected")).font(semibold(17.0)).color(TEXT));
                            let how = if self.device.usb { Some("USB".to_string()) } else { self.device.current_wifi.clone() };
                            let line = [Some(format!("Logger {id}")), who, how, Some(format!("answered in {} ms", b.round_trip.as_millis()))]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join("  ·  ");
                            ui.label(RichText::new(line).color(SECONDARY));
                            // Its own tracks (made on it or sent from RaceStudio), when it has any.
                            if !self.device.tracks.custom.is_empty() {
                                let list = self
                                    .device
                                    .tracks
                                    .custom
                                    .iter()
                                    .map(|(name, state)| match state {
                                        crate::device::TrackCopy::Here => format!("{name} (on this computer)"),
                                        crate::device::TrackCopy::Copied => format!("{name} (copied just now)"),
                                        crate::device::TrackCopy::Failed => format!("{name} (couldn't be copied; see the log)"),
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                ui.label(RichText::new(format!("Custom tracks on it: {list}")).color(SECONDARY));
                            }
                        });
                        if self.device.joined.is_some() {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(ui, Some(Icon::Wifi), "Disconnect", false).clicked() && self.device.busy.is_none() {
                                    self.device.busy = Some("Reconnecting to your network…".into());
                                    let _ = self.device.worker.as_ref().unwrap().tx.send(crate::device::Command::Leave);
                                }
                            });
                        }
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.add(Icon::Mychron.image(DIM, 32.0));
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("No MyChron found yet").font(semibold(17.0)).color(TEXT));
                            let hint = match &self.device.busy {
                                Some(b) => b.clone(),
                                None => crate::device::JOIN_HINT.into(),
                            };
                            ui.label(RichText::new(hint).color(SECONDARY));
                        });
                    });
                    if !self.device.networks.is_empty() {
                        ui.add_space(12.0);
                        hairline(ui);
                        ui.add_space(10.0);
                        let current = self.device.current_wifi.clone().unwrap_or_else(|| "your current network".into());
                        ui.label(RichText::new(format!("MyChrons in range. Joining one takes this computer off {current} until you disconnect.")).size(12.5).color(DIM));
                        ui.add_space(6.0);
                        for (ssid, signal) in self.device.networks.clone() {
                            ui.horizontal(|ui| {
                                ui.add(Icon::Wifi.image(SECONDARY, 18.0));
                                ui.label(RichText::new(model_from_ssid(&ssid)).font(medium(14.5)).color(TEXT));
                                ui.label(RichText::new(format!("{ssid}  ·  {signal}% signal")).color(DIM));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if button(ui, None, "Join", true).clicked() && self.device.busy.is_none() {
                                        self.device.busy = Some(format!("Joining {ssid}…"));
                                        let _ = self.device.worker.as_ref().unwrap().tx.send(crate::device::Command::Join { ssid: ssid.clone() });
                                    }
                                });
                            });
                        }
                    }
                }
            }
        });
    }

    fn device_sessions(&mut self, ui: &mut Ui) {
        let new: std::collections::BTreeSet<String> = self.new_on_logger().into_iter().map(|e| e.name).collect();
        let sessions: Vec<aim_link::DirEntry> = {
            let mut v: Vec<_> = self.device.files.iter().filter(|e| e.is_session()).cloned().collect();
            v.sort_by(|a, b| b.name.cmp(&a.name));
            v
        };
        let logger_id = self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
        let setup_now = self.latest_setup(&logger_id);
        let next_setup = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).karts.iter().find(|k| k.logger == logger_id).and_then(|k| k.next_setup.clone());
        let summary_of = |name: &str| self.device.summaries.iter().find(|s| s.file.as_deref() == Some(name)).cloned();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sessions on the logger").font(semibold(16.0)).color(TEXT));
                ui.label(RichText::new(format!("{} total  ·  {} new", sessions.len(), new.len())).color(SECONDARY));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let n = self.device.selected.len();
                    let label = if n == 0 { "Download".to_string() } else { format!("Download {n}") };
                    // Quiet until something is picked: it does nothing then.
                    if button(ui, Some(Icon::Download), &label, n > 0).clicked() && n > 0 && self.device.busy.is_none() {
                        let id = self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
                        let p = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).for_logger(&id);
                        // The track comes from the logger: one track pre-fills the field, several defer to each session.
                        let mut detected: Vec<String> = self.device.selected.iter().filter_map(|n| summary_of(n)).filter_map(|s| s.track).filter(|t| !t.is_empty()).collect();
                        detected.sort();
                        detected.dedup();
                        let track = if detected.len() == 1 { detected[0].clone() } else { String::new() };
                        self.device.dialog = Some(DownloadDialog {
                            meta: crate::device::SessionMeta {
                                driver: p.driver,
                                kart: p.kart,
                                championship: p.championship,
                                track,
                                ..Default::default()
                            },
                            detected,
                            // A setup typed for a failed download, else the kart's next one (Garage), else what it runs.
                            setup: self.device.unsaved_setup.clone().or(next_setup).unwrap_or_else(|| setup_now.clone()),
                            setup_before: setup_now.clone(),
                            setup_open: false,
                            replace: true,
                            setup_target: None,
                            adding_driver: false,
                        });
                    }
                    if chip(ui, "None", false).clicked() {
                        self.device.selected.clear();
                    }
                    if chip(ui, "All new", false).clicked() {
                        self.device.selected = new.clone();
                    }
                });
            });
            if let Some((index, count, file, done, total)) = &self.device.progress {
                ui.add_space(8.0);
                let frac = (*index as f32 + *done as f32 / (*total).max(1) as f32) / (*count).max(1) as f32;
                ui.label(RichText::new(format!("Downloading {file}  ({} of {count})", index + 1)).color(SECONDARY));
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                ui.painter().rect_filled(bar, 3.0, SURFACE_ACTIVE);
                ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height())), 3.0, TEXT);
            } else if let Some(b) = &self.device.busy {
                ui.add_space(6.0);
                ui.label(RichText::new(b).color(SECONDARY));
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("logger_sessions").auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for e in &sessions {
                    let sum = summary_of(&e.name);
                    let is_new = new.contains(&e.name);
                    let mut on = self.device.selected.contains(&e.name);
                    let response = row(ui, false, |ui| {
                        let (cb, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                        ui.painter().rect_stroke(cb, CornerRadius::same(4), Stroke::new(1.5, if on { TEXT } else { DIM }), egui::StrokeKind::Inside);
                        if on {
                            Icon::Check.image(TEXT, 14.0).paint_at(ui, cb.shrink(2.0));
                        }
                        ui.add_space(6.0);
                        let number = aim_xrk::summary::session_number(&e.name).map(|n| format!("Session {n}")).unwrap_or(e.name.clone());
                        ui.label(RichText::new(number).font(medium(14.0)).color(TEXT));
                        let when = sum.as_ref().map(|s| [s.date.map(crate::events::us_short), s.start_time.as_deref().map(crate::events::clock_time)].into_iter().flatten().collect::<Vec<_>>().join("  ·  ")).unwrap_or(e.date.clone());
                        ui.label(RichText::new(when).color(DIM));
                        if let Some(t) = sum.as_ref().and_then(|s| s.track.clone()) {
                            ui.label(RichText::new(t).color(SECONDARY));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if is_new {
                                tag(ui, "New", TEXT);
                                if sum.as_ref().is_some_and(listed_warm_up) {
                                    tag(ui, "Warm-up", DIM);
                                }
                            } else {
                                ui.label(RichText::new("Downloaded").size(12.5).color(DIM));
                            }
                            ui.label(RichText::new(format!("{:.1} MB", e.size as f64 / 1e6)).size(12.5).color(DIM));
                            if let Some(s) = &sum {
                                let best = s.best_lap_ms.map(|b| format_lap_time(b as i64)).unwrap_or_else(|| "—".into());
                                ui.label(RichText::new(best).font(medium(14.0)).color(TEXT));
                                let laps = s.laps.unwrap_or(0);
                                ui.label(RichText::new(plural(laps as usize, "lap")).size(12.5).color(DIM));
                            }
                        });
                    });
                    if response.clicked() {
                        on = !on;
                        if on {
                            self.device.selected.insert(e.name.clone());
                        } else {
                            self.device.selected.remove(&e.name);
                        }
                    }
                }
            });
        });
    }

    /// RaceStudio-style dialog: who, what, which championship, session type and
    /// track, before downloading. Every field but the comment suggests values
    /// already in use.
    fn download_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.device.dialog.take() else { return };
        let lib = self.lib();
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let s = Suggestions::gather(&lib, &profiles);
        let tires = self.tire_suggestions();
        let lib = self.lib();
        let multiple = dlg.detected.len() > 1;
        let mut go = false;
        let mut cancel = false;
        let n = self.device.selected.len();
        let field_w = 460.0;
        // Picked sessions already saved here, as "Session 10".
        let dir = self.logger_dir();
        let again: Vec<String> = self
            .device.files
            .iter()
            .filter(|e| self.device.selected.contains(&e.name) && dir.as_ref().is_some_and(|d| crate::device::downloaded(d, e).is_some()))
            .map(|e| aim_xrk::summary::session_number(&e.name).map(|k| format!("Session {k}")).unwrap_or(e.name.clone()))
            .collect();
        // Where a changed setup is saved, shown with it so a wrong session is caught before downloading.
        let target = dlg.setup_target.get_or_insert_with(|| self.download_setup_target(&lib)).clone();
        let setup_from = match &target {
            Some((_, start)) => format!("Saved with the session that started {}. Sessions after it use it until it's changed.", start_words(start)),
            None => "None of these sessions is newer than the ones already here, so this setup isn't saved with them. To change an earlier session's setup, open it and use Setup.".into(),
        };
        // The Garage's drivers and karts, and what this MyChron is on now.
        let drivers: Vec<String> = profiles.drivers.iter().map(|d| d.name.clone()).collect();
        let karts: Vec<String> = profiles.garage_karts().map(|k| k.name.clone()).collect();
        let current = profiles.for_logger(&self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default());
                let closed = look::dialog(ctx, "download", 760.0, |ui| {
            use look;
            let title = format!("Download {n} session{}", if n == 1 { "" } else { "s" });
            look::header(ui, Icon::Download, SECONDARY, None, &title, "Who drove, in which kart, and where. These are remembered for next time.");
            // Two steps, either one a click away: the details, then the setup.
            if let Some(k) = look::steps(ui, &["Details", "Setup"], usize::from(dlg.setup_open), true) {
                dlg.setup_open = k == 1;
            }
            look::body(ui, |ui| {
                if dlg.setup_open {
                    setup_ui::setup_fields(ui, "dl_setup", &mut dlg.setup, &tires, 480.0, true);
                    let changes = dlg.setup.changes_from(&dlg.setup_before);
                    if changes.is_empty() {
                        look::note(ui, "Same as last time.", DIM);
                    } else {
                        look::note(ui, &format!("Changed: {}", changes.join(", ")), theme::FAVORITE);
                    }
                    // Changes and notes are both saved on one session: say which.
                    if !changes.is_empty() || !dlg.setup.notes.trim().is_empty() {
                        look::note(ui, &setup_from, DIM);
                    }
                    return;
                }
                egui::Grid::new("download_meta").num_columns(2).spacing([20.0, 16.0]).min_col_width(120.0).show(ui, |ui| {
                    // Driver and kart come from the Garage, so a typo can't make a new
                    // driver or kart, or move a MyChron; a new driver is added on purpose.
                    look::label(ui, "Driver");
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        if dlg.adding_driver || drivers.is_empty() {
                            suggest_field(ui, "dl_driver", &mut dlg.meta.driver, "New driver's name", &[], field_w);
                            if !drivers.is_empty() && ui.add(egui::Label::new(RichText::new("Pick from the Garage instead").size(12.5).color(SECONDARY)).sense(Sense::click())).clicked() {
                                dlg.adding_driver = false;
                                dlg.meta.driver = current.driver.clone();
                            }
                            if !dlg.meta.driver.trim().is_empty() && !drivers.iter().any(|d| d.eq_ignore_ascii_case(dlg.meta.driver.trim())) {
                                look::note(ui, &format!("{} is added to the Garage.", dlg.meta.driver.trim()), DIM);
                            }
                        } else if pick_field(ui, "dl_driver", &mut dlg.meta.driver, &drivers, "Pick a driver", Some("Add a driver…"), field_w) {
                            dlg.adding_driver = true;
                            dlg.meta.driver.clear();
                        }
                    });
                    ui.end_row();
                    look::label(ui, "Kart");
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        if karts.is_empty() {
                            look::note(ui, "No karts in the Garage yet. Add one there, and its MyChron's sessions become its.", DIM);
                        } else {
                            pick_field(ui, "dl_kart", &mut dlg.meta.kart, &karts, "No kart", None, field_w);
                            let picked = dlg.meta.kart.trim();
                            if !picked.is_empty() && !picked.eq_ignore_ascii_case(&current.kart) {
                                let from = if current.kart.is_empty() { String::new() } else { format!(" from {}", current.kart) };
                                look::note(ui, &format!("This MyChron moves to {picked}{from}: these sessions and later ones are its."), theme::FAVORITE);
                            }
                        }
                    });
                    ui.end_row();
                    look::label(ui, "Championship");
                    suggest_field(ui, "dl_champ", &mut dlg.meta.championship, "e.g. 2026 Club Championship", &s.championships, field_w);
                    ui.end_row();
                    look::label(ui, "Session type");
                    suggest_field(ui, "dl_type", &mut dlg.meta.session_type, "Practice 1, Heat 2, Final…", &s.session_types, field_w);
                    ui.end_row();
                    look::label(ui, "Track");
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        if multiple {
                            ui.label(RichText::new(format!("Several: {}", dlg.detected.join(", "))).font(regular(16.5)).color(TEXT));
                            look::note(ui, "Each session keeps the track the MyChron recorded for it.", DIM);
                        } else {
                            suggest_field(ui, "dl_track", &mut dlg.meta.track, "Track", &s.tracks, field_w);
                            let from_logger = dlg.detected.first().cloned().unwrap_or_default();
                            let note = if !from_logger.is_empty() && dlg.meta.track == from_logger {
                                "From the MyChron.".to_string()
                            } else if !from_logger.is_empty() {
                                format!("The MyChron recorded {from_logger}; these sessions will be filed under {} instead.", dlg.meta.track)
                            } else {
                                "The MyChron didn't recognize a track for these sessions.".into()
                            };
                            look::note(ui, &note, DIM);
                        }
                    });
                    ui.end_row();
                    look::label(ui, "Comment");
                    look::text_area(ui, &mut dlg.meta.comment, "Optional: conditions, how it went", field_w, 2);
                    ui.end_row();
                    // Sessions picked again that are already here: replace them, or keep both.
                    if !again.is_empty() {
                        look::label(ui, "Already here");
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                if chip(ui, "Replace", dlg.replace).clicked() {
                                    dlg.replace = true;
                                }
                                if chip(ui, "Keep both", !dlg.replace).clicked() {
                                    dlg.replace = false;
                                }
                            });
                            let which = if again.len() == 1 { format!("{} is", again[0]) } else { format!("{} of these are", again.len()) };
                            let what = if dlg.replace { "Its copy here is updated with the details filled in above; blank ones keep what it had." } else { "A second copy is saved beside it." };
                            look::note(ui, &format!("{which} already on this computer. {what}"), DIM);
                        });
                        ui.end_row();
                    }
                    look::label(ui, "Setup");
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        let summary = dlg.setup.summary();
                        ui.add(egui::Label::new(RichText::new(if summary.is_empty() { "None recorded yet".to_string() } else { summary }).font(regular(16.0)).color(TEXT)).wrap());
                        let changes = dlg.setup.changes_from(&dlg.setup_before);
                        if changes.is_empty() {
                            look::note(ui, "Same as last time.", DIM);
                        } else {
                            look::note(ui, &format!("Changed: {}", changes.join(", ")), theme::FAVORITE);
                        }
                        // Changes and notes are both saved on one session: say which.
                        if !changes.is_empty() || !dlg.setup.notes.trim().is_empty() {
                            look::note(ui, &setup_from, DIM);
                        }
                    });
                    ui.end_row();
                });
            });
            look::footer_row(ui, |ui| {
                // Back from the setup; Cancel from the details.
                if look::big_button(ui, if dlg.setup_open { "Back" } else { "Cancel" }, None, look::Kind::Quiet, true) {
                    if dlg.setup_open {
                        dlg.setup_open = false;
                    } else {
                        cancel = true;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if look::big_button(ui, "Download", Some(Icon::Download), look::Kind::Primary, true) {
                        go = true;
                    }
                    if !dlg.setup_open && look::big_button(ui, "Change setup", Some(Icon::Settings), look::Kind::Quiet, true) {
                        dlg.setup_open = true;
                    }
                });
            });
        });
        let open = !closed;
        if go {
            let id = self.device.logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
            // Remember these for next time (not the comment or session type, which change every time).
            // The MyChron is now on the kart named, so the kart's baseline applies to it.
            // A MyChron moving to another kart leaves its sessions so far with the old one.
            let lib = self.lib();
            let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
            profiles.note_download(&id, &dlg.meta.driver, &dlg.meta.kart, &dlg.meta.championship, (!multiple).then_some(dlg.meta.track.as_str()), &|l: &str| lib.newest_start(l));
            if let Err(e) = profiles.save() {
                self.notices.error(format!("Couldn't save the driver and kart: {e}"));
            }
            self.setup_baselines = None;
            self.reload_library();
            // Each file gets the dialog's details; with several tracks, its own track from the logger.
            let files: Vec<(aim_link::DirEntry, crate::device::SessionMeta)> = self
                .device.files
                .iter()
                .filter(|e| self.device.selected.contains(&e.name))
                .map(|e| {
                    let mut meta = dlg.meta.clone();
                    if multiple || meta.track.is_empty() {
                        meta.track = self.device.summaries.iter().find(|s| s.file.as_deref() == Some(e.name.as_str())).and_then(|s| s.track.clone()).unwrap_or_default();
                    }
                    (e.clone(), meta)
                })
                .collect();
            // A setup sheet goes on the earliest new session only, and only when there's something new.
            let changed = !dlg.setup.same_setup(&dlg.setup_before) || !dlg.setup.notes.trim().is_empty();
            self.device.pending_setup = match (changed, target) {
                (true, Some((path, _))) => Some((path, dlg.setup.clone())),
                _ => None,
            };
            if crate::hooks::get().fake_device.is_some() {
                // Preview mode: there's no MyChron to download from.
                self.notices.info(format!(
                    "Preview only: would download {} session{} with these details. Turn on the MyChron to do it for real.",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ));
            } else if let Some(dir) = self.logger_dir() {
                self.device.busy = Some("Downloading…".into());
                let _ = self.device.worker.as_ref().unwrap().tx.send(crate::device::Command::Download { files, dir, replace: dlg.replace });
            }
        } else if open && !cancel {
            self.device.dialog = Some(dlg);
        }
    }
}

const AP_ADDRESS_FAKE: std::net::Ipv4Addr = std::net::Ipv4Addr::new(10, 0, 0, 1);

/// Whether a session on the logger was an engine warm-up, by its own
/// summary's top speed (read for new sessions only; unknown counts as not).
fn listed_warm_up(s: &aim_xrk::SessionSummary) -> bool {
    s.max_speed.is_some() && crate::events::is_warm_up(s)
}

/// "2026-10-31 08:58:10" → "Oct 31, 2026 at 8:58 AM".
fn start_words(start: &str) -> String {
    let (date, time) = start.split_once(' ').unwrap_or((start, ""));
    let mut ymd = date.split('-').map(|p| p.parse::<u16>().ok());
    let day = match (ymd.next().flatten(), ymd.next().flatten(), ymd.next().flatten()) {
        (Some(year), Some(month), Some(day)) => crate::events::us_short(aim_xrk::summary::Date { year, month: month as u8, day: day as u8 }),
        _ => return start.to_string(),
    };
    if time.is_empty() { day } else { format!("{day} at {}", crate::events::clock_time(time)) }
}

/// "AiM-MYC6-345678" → "MyChron 6".
pub(super) fn model_from_ssid(ssid: &str) -> &'static str {
    match ssid.split('-').nth(1).unwrap_or("") {
        "MYC6" => "MyChron 6",
        "MYC5S" => "MyChron 5S",
        "MYC5" => "MyChron 5",
        _ => "AiM logger",
    }
}
