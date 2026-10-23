//! The Device page's worker: finds the MyChron (plugged in by USB, or its
//! WiFi, which it joins), talks to the logger and downloads sessions, all on
//! a background thread so the window never stalls. The UI sends [`Command`]s
//! and reads [`Event`]s.
//!
//! On Linux, WiFi goes through NetworkManager (`nmcli`). While joined to the logger,
//! other WiFi networks stop auto-joining so the computer doesn't wander back
//! to one with internet; they're restored when you leave, or at the next
//! start if Mychronix quit while joined (see [`restore_after_crash`]). On a
//! Mac you join the MyChron's network from the menu bar (see `mod wifi`).

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use aim_link::{AP_ADDRESS, Beacon, DirEntry, Link};
use aim_xrk::SessionSummary;
use serde::{Deserialize, Serialize};

pub enum Command {
    /// Look for a logger on USB, then at 10.0.0.1, and list AiM networks in range.
    Scan,
    Join { ssid: String },
    Leave,
    /// Read the logger's session list (folder listing + summary file).
    List,
    /// Each file with the details to save beside it.
    Download { files: Vec<(DirEntry, SessionMeta)>, dir: PathBuf },
}

pub enum Event {
    /// Result of a scan: the logger if one answered (`usb` when it's plugged
    /// in), AiM networks in range, and the current WiFi network.
    Scanned { logger: Option<Beacon>, usb: bool, networks: Vec<(String, u8)>, current: Option<String> },
    Joined { ssid: String },
    Left,
    /// The logger's sessions, plus how many custom tracks were copied from it into the library.
    Listed { files: Vec<DirEntry>, summaries: Vec<SessionSummary>, new_tracks: usize },
    Progress { index: usize, count: usize, file: String, done: u64, total: u64 },
    Downloaded { saved: usize },
    Error(String),
}

/// What RaceStudio's download dialog asks for, kept beside each session as `a_NNNN.meta.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SessionMeta {
    pub driver: String,
    pub kart: String,
    pub championship: String,
    /// The track the session belongs to: the logger's own unless changed at download.
    pub track: String,
    /// RaceStudio's session types: "Practice 1", "Heat 2", "Final", …
    #[serde(default)]
    pub session_type: String,
    #[serde(default)]
    pub comment: String,
    /// The file's date and time as the MyChron listed it when downloaded, so
    /// a different recording under the same name (after the MyChron was
    /// formatted) is told apart.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub listed: String,
}

/// A listed file's date and time ("10/10/2026 14:39:36").
fn listed(entry: &DirEntry) -> String {
    format!("{} {}", entry.date.trim(), entry.time.trim()).trim().to_string()
}

/// `a_0001.xrz`, then `a_0001-2.xrz`, `a_0001-3.xrz`…: where a recording
/// named `name` may be saved in `dir`.
fn candidates(dir: &Path, name: &str) -> impl Iterator<Item = PathBuf> {
    let p = Path::new(name);
    let (stem, ext) = (p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default());
    let dir = dir.to_path_buf();
    (1..).map(move |k| dir.join(if k == 1 { name.to_string() } else { format!("{stem}-{k}.{ext}") }))
}

/// The local copy of `entry`, if it's been downloaded into `dir`: the same
/// size, and (when the download noted it) the same listed date and time.
pub fn downloaded(dir: &Path, entry: &DirEntry) -> Option<PathBuf> {
    candidates(dir, &entry.name).take_while(|p| p.exists()).find(|p| {
        let same_size = std::fs::metadata(p).is_ok_and(|m| m.len() == entry.size);
        let same_time = SessionMeta::load(p).map(|m| m.listed).filter(|l| !l.is_empty()).is_none_or(|l| l == listed(entry));
        same_size && same_time
    })
}

/// Where a new download of `entry` goes: its own name, or the next free
/// numbered one when that's taken. Nothing already downloaded is replaced.
fn target_for(dir: &Path, entry: &DirEntry) -> PathBuf {
    candidates(dir, &entry.name).find(|p| !p.exists()).expect("an unused name")
}

impl SessionMeta {
    pub fn path_for(session: &Path) -> PathBuf {
        session.with_extension("meta.json")
    }

    pub fn load(session: &Path) -> Option<Self> {
        crate::storage::load(&Self::path_for(session))
    }

    pub fn save(&self, session: &Path) -> std::io::Result<()> {
        crate::storage::save(&Self::path_for(session), self)
    }
}

pub struct Worker {
    pub tx: Sender<Command>,
    pub rx: Receiver<Event>,
}

impl Worker {
    pub fn spawn(ctx: eframe::egui::Context) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (ev_tx, ev_rx) = channel::<Event>();
        crate::jobs::spawn("mychron", move || {
            let send = |e: Event| {
                let _ = ev_tx.send(e);
                ctx.request_repaint();
            };
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    Command::Scan => {
                        // A MyChron plugged in by USB comes first: no WiFi to join.
                        if let Some(logger) = usb_logger() {
                            send(Event::Scanned { logger: Some(logger), usb: true, networks: Vec::new(), current: current_wifi() });
                            continue;
                        }
                        let logger = aim_link::probe(AP_ADDRESS, Duration::from_millis(700)).ok();
                        let (networks, current) = if logger.is_some() { (Vec::new(), current_wifi()) } else { (aim_networks(), current_wifi()) };
                        log::debug!(
                            "scan: logger {:?}, on WiFi {:?}, MyChron networks {:?}",
                            logger.as_ref().map(|b| (b.logger_id, b.round_trip)),
                            current,
                            networks
                        );
                        send(Event::Scanned { logger, usb: false, networks, current });
                    }
                    Command::Join { ssid } => {
                        log::info!("joining {ssid}");
                        match join(&ssid) {
                            Ok(()) => {
                                log::info!("joined {ssid}");
                                send(Event::Joined { ssid })
                            }
                            Err(e) => {
                                log::error!("joining {ssid} failed: {e}");
                                send(Event::Error(e))
                            }
                        }
                    }
                    Command::Leave => {
                        log::info!("leaving the MyChron's WiFi");
                        leave();
                        send(Event::Left);
                    }
                    Command::List => {
                        let started = std::time::Instant::now();
                        match list() {
                            Ok((files, summaries, new_tracks)) => {
                                log::info!("listed {} files, {} summaries, {} new custom tracks in {:?}", files.len(), summaries.len(), new_tracks, started.elapsed());
                                for f in &files {
                                    log::debug!("  {} {} bytes {} {}", f.name, f.size, f.date, f.time);
                                }
                                send(Event::Listed { files, summaries, new_tracks })
                            }
                            Err(e) => {
                                log::error!("listing failed: {e}");
                                send(Event::Error(e))
                            }
                        }
                    }
                    Command::Download { files, dir } => {
                        log::info!("downloading {} sessions to {}", files.len(), dir.display());
                        match download(&files, &dir, &send) {
                            Ok(saved) => {
                                log::info!("downloaded {saved} sessions");
                                send(Event::Downloaded { saved })
                            }
                            Err(e) => {
                                log::error!("download failed: {e}");
                                send(Event::Error(e))
                            }
                        }
                    }
                }
            }
        });
        Self { tx: cmd_tx, rx: ev_rx }
    }
}

/// A MyChron plugged in by USB, if there is one, as a scan would find it on WiFi.
fn usb_logger() -> Option<Beacon> {
    if !aim_link::usb::is_present() {
        return None;
    }
    let started = std::time::Instant::now();
    let found = aim_link::usb::UsbLogger::open().and_then(|mut l| l.logger_id());
    match found {
        Ok(logger_id) => {
            log::debug!("scan: MyChron {logger_id:?} on USB");
            Some(Beacon { address: std::net::Ipv4Addr::UNSPECIFIED, logger_id, round_trip: started.elapsed(), raw: Vec::new() })
        }
        Err(e) => {
            log::warn!("scan: a MyChron is plugged in by USB but didn't answer: {e}");
            None
        }
    }
}

fn connect() -> Result<Link, String> {
    Link::connect().map_err(|e| format!("Couldn't connect to the MyChron: {e}"))
}

fn list() -> Result<(Vec<DirEntry>, Vec<SessionSummary>, usize), String> {
    let mut logger = connect()?;
    let listing = logger.list(aim_link::SESSIONS_DIR).map_err(|e| format!("Couldn't list sessions: {e}"))?;
    let summaries = listing
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case("summary1.csv"))
        .and_then(|_| logger.read_file("1:/mem/summary1.csv", |_, _| {}).ok())
        .map(|csv| aim_xrk::parse_summary_csv(&String::from_utf8_lossy(&csv)))
        .unwrap_or_default();
    let new_tracks = copy_custom_tracks(&mut logger);
    copy_track_list(&mut logger);
    Ok((listing, summaries, new_tracks))
}

/// Copies the logger's custom tracks (listed in `0:/tkk/dev.ria`, stored as
/// `0:/tkk/o/<id>.tkk`) into `~/Mychronix/tracks/custom` when they're new, so
/// sessions recorded on them have a track to attach to. RaceStudio does the
/// same while connecting. Returns how many were copied.
fn copy_custom_tracks(logger: &mut Link) -> usize {
    let Some(dir) = crate::paths::data("tracks/custom") else { return 0 };
    let Ok(index) = logger.read_file("0:/tkk/dev.ria", |_, _| {}) else { return 0 };
    let Ok(tracks) = aim_track::parse_ria(&index) else { return 0 };
    let mut copied = 0;
    for t in tracks {
        // The track id comes from the logger: only plain names make a file name.
        if !aim_link::is_safe_file_name(&t.id) {
            continue;
        }
        let target = dir.join(format!("{}.tkk", t.id));
        if target.exists() {
            continue;
        }
        match logger.read_file(&format!("0:/tkk/o/{}.tkk", t.id), |_, _| {}) {
            Ok(bytes) if aim_track::parse_tkk(&bytes).is_ok() => match crate::storage::write_atomic(&target, &bytes) {
                Ok(()) => copied += 1,
                Err(e) => log::warn!("couldn't save custom track {}: {e}", target.display()),
            },
            Ok(_) => log::warn!("the MyChron's custom track {} couldn't be read as a track; skipped", t.id),
            Err(e) => log::warn!("couldn't read custom track {} from the MyChron: {e}", t.id),
        }
    }
    copied
}

/// Keeps a copy of the logger's own AiM track list (`0:/tkk/al.ria`), so a
/// computer that has never been online still knows the tracks by name and
/// start/finish line. Read only when it isn't saved yet (it's ~340 KB).
fn copy_track_list(logger: &mut Link) {
    let Some(target) = crate::aim_tracks::mychron_list_target() else { return };
    if target.exists() {
        return;
    }
    match logger.read_file("0:/tkk/al.ria", |_, _| {}) {
        Ok(bytes) if aim_track::parse_ria(&bytes).is_ok_and(|r| !r.is_empty()) => {
            if let Err(e) = crate::storage::write_atomic(&target, &bytes) {
                log::warn!("couldn't save the MyChron's track list to {}: {e}", target.display());
            }
        }
        Ok(_) => log::warn!("the MyChron's track list couldn't be read; not saved"),
        Err(e) => log::warn!("couldn't read the MyChron's track list: {e}"),
    }
}

/// Downloads each session (and its `.xri` summary) into `dir`, writing the
/// metadata beside it. Files land under a temporary name first, so a
/// canceled download never looks complete.
fn download(files: &[(DirEntry, SessionMeta)], dir: &Path, send: &impl Fn(Event)) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let mut logger = connect()?;
    let count = files.len();
    let mut saved = 0;
    for (index, (entry, meta)) in files.iter().enumerate() {
        // The name comes from the logger: never let it point outside this folder.
        if !aim_link::is_safe_file_name(&entry.name) {
            return Err(format!("The MyChron listed a file with an unusable name ({:?}), so it wasn't downloaded.", entry.name));
        }
        let path = format!("{}/{}", aim_link::SESSIONS_DIR, entry.name);
        let data = logger
            .read_file(&path, |done, total| send(Event::Progress { index, count, file: entry.name.clone(), done, total }))
            .map_err(|e| format!("Downloading {} failed: {e}", entry.name))?;
        let target = target_for(dir, entry);
        crate::storage::write_atomic(&target, &data).map_err(|e| format!("Couldn't save {}: {e}", target.display()))?;
        log::info!("saved {} ({} bytes)", target.display(), data.len());
        // The small summary beside it, used by the Sessions and Tracks pages
        // (without it, Mychronix works the summary out from the session).
        let xri = Path::new(&entry.name).with_extension("xri");
        match logger.read_file(&format!("{}/{}", aim_link::SESSIONS_DIR, xri.display()), |_, _| {}) {
            Ok(bytes) => {
                if let Err(e) = crate::storage::write_atomic(&target.with_extension("xri"), &bytes) {
                    log::warn!("couldn't save the summary beside {}: {e}", target.display());
                }
            }
            Err(e) => log::warn!("couldn't read {} from the MyChron: {e}", xri.display()),
        }
        let meta = SessionMeta { listed: listed(entry), ..meta.clone() };
        meta.save(&target).map_err(|e| e.to_string())?;
        saved += 1;
    }
    Ok(saved)
}

// ---------------------------------------------------------------------------
// WiFi: NetworkManager on Linux; on a Mac you join from the menu bar.

pub use wifi::{JOIN_HINT, paused_networks, restore_after_crash, restore_wifi_now, restore_wifi_on_quit};
use wifi::{aim_networks, current_wifi, join, leave};

#[cfg(not(target_os = "macos"))]
mod wifi {
    use super::*;

    /// What the Device page says while no MyChron answers.
    pub const JOIN_HINT: &str = "Turn on the MyChron and its WiFi. Mychronix keeps looking.";


    fn restore_file() -> Option<PathBuf> {
        crate::paths::data("wifi-restore.json")
    }

    #[derive(Serialize, Deserialize, Default)]
    struct Restore {
        /// Connection profiles whose auto-join we paused.
        paused: Vec<String>,
        /// The network to go back to.
        previous: Option<String>,
    }

    pub(super) fn nmcli(args: &[&str]) -> Result<String, String> {
        log::debug!("nmcli {}", args.join(" "));
        let out = Process::new("nmcli").args(args).output().map_err(|e| {
            log::error!("couldn't run nmcli: {e}");
            format!("nmcli: {e}")
        })?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            log::warn!("nmcli {} failed: {err}", args.join(" "));
            Err(err)
        }
    }

    /// `nmcli -t` escapes ':' inside fields as '\:'.
    pub(super) fn split_terse(line: &str) -> Vec<String> {
        let mut fields = vec![String::new()];
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => fields.last_mut().unwrap().extend(chars.next()),
                ':' => fields.push(String::new()),
                c => fields.last_mut().unwrap().push(c),
            }
        }
        fields
    }

    /// AiM networks in range (SSID, signal %), strongest first.
    pub(super) fn aim_networks() -> Vec<(String, u8)> {
        let Ok(out) = nmcli(&["-t", "-f", "SSID,SIGNAL", "device", "wifi", "list", "--rescan", "yes"]) else { return Vec::new() };
        let mut nets: Vec<(String, u8)> = out
            .lines()
            .map(split_terse)
            .filter(|f| f.len() >= 2 && f[0].starts_with("AiM-"))
            .map(|f| (f[0].clone(), f[1].parse().unwrap_or(0)))
            .collect();
        nets.sort_by(|a, b| b.1.cmp(&a.1));
        nets.dedup_by(|a, b| a.0 == b.0);
        nets
    }

    /// The WiFi connection in use (its NetworkManager name, which for a
    /// MyChron is its SSID). Read from the active connections rather than
    /// the scan list, which can be momentarily empty mid-scan.
    pub(super) fn current_wifi() -> Option<String> {
        nmcli(&["-t", "-f", "NAME,TYPE", "connection", "show", "--active"])
            .ok()?
            .lines()
            .map(split_terse)
            .find(|f| f.get(1).is_some_and(|t| t == "802-11-wireless"))
            .and_then(|f| f.first().cloned())
            .filter(|s| !s.is_empty())
    }

    fn load_restore() -> Option<Restore> {
        restore_file().and_then(|p| crate::storage::load(&p))
    }

    pub(super) fn join(ssid: &str) -> Result<(), String> {
        // Already paused by an earlier join that wasn't undone: keep its list
        // and network to go back to, which a fresh look can no longer see.
        let earlier = load_restore().unwrap_or_default();
        let previous = earlier.previous.or_else(|| current_wifi().filter(|s| !s.starts_with("AiM-")));
        let mut paused = earlier.paused;
        // Stop other WiFi profiles from pulling the computer back to the internet.
        for line in nmcli(&["-t", "-f", "NAME,TYPE,AUTOCONNECT", "connection", "show"])?.lines() {
            let f = split_terse(line);
            if f.len() >= 3 && f[1] == "802-11-wireless" && f[2] == "yes" && f[0] != ssid {
                if nmcli(&["connection", "modify", &f[0], "connection.autoconnect", "no"]).is_ok() && !paused.contains(&f[0]) {
                    paused.push(f[0].clone());
                }
            }
        }
        log::info!("paused auto-connect for {} networks; will go back to {}", paused.len(), previous.as_deref().unwrap_or("whichever reconnects"));
        if let Some(path) = restore_file() {
            if let Err(e) = crate::storage::save(&path, &Restore { paused, previous }) {
                log::error!("couldn't save {}: {e}", path.display());
            }
        }
        if let Err(e) = nmcli(&["device", "wifi", "connect", ssid]) {
            leave();
            return Err(format!("Couldn't join {ssid}: {e}"));
        }
        // The MyChron's network never auto-joins on its own.
        let _ = nmcli(&["connection", "modify", ssid, "connection.autoconnect", "no"]);
        // Wait for its DHCP address.
        for _ in 0..20 {
            if aim_link::probe(AP_ADDRESS, Duration::from_millis(500)).is_ok() {
                return Ok(());
            }
        }
        Err(format!("Joined {ssid}, but the MyChron didn't answer"))
    }

    /// Re-enables paused auto-join and goes back to the previous network.
    /// Turns the other networks' auto-connect back on right away (used when the
    /// app crashes while joined to a MyChron).
    pub fn restore_wifi_now() {
        log::warn!("restoring WiFi after a crash");
        leave();
    }

    /// Mychronix is quitting while joined to a MyChron.
    pub fn restore_wifi_on_quit() {
        leave();
    }

    pub(super) fn leave() {
        let Some(path) = restore_file() else { return };
        let Some(restore) = load_restore() else { return };
        let mut failed = 0;
        for name in &restore.paused {
            if nmcli(&["connection", "modify", name, "connection.autoconnect", "yes"]).is_err() {
                failed += 1;
            }
        }
        log::info!("turned auto-connect back on for {} of {} networks", restore.paused.len() - failed, restore.paused.len());
        // Still on the MyChron's network: go back. (Already on a network of
        // your choosing: leave it be.)
        if let Some(aim) = current_wifi().filter(|c| c.starts_with("AiM-")) {
            let back = restore.previous.as_deref().is_some_and(|ssid| nmcli(&["connection", "up", ssid]).is_ok());
            if !back {
                // Drop only the MyChron's connection; NetworkManager then joins
                // the best known network itself. (Disconnecting the WiFi device
                // would stop it auto-connecting at all until joined by hand.)
                let _ = nmcli(&["connection", "down", &aim]);
            }
        }
        // Kept if some couldn't be turned back on, so the next try covers them.
        if failed == 0 {
            if let Err(e) = std::fs::remove_file(&path) {
                log::warn!("couldn't remove {}: {e}", path.display());
            }
        }
    }

    /// True while Mychronix has other networks' auto-connect paused.
    pub fn paused_networks() -> bool {
        restore_file().is_some_and(|p| p.exists())
    }

    /// If Mychronix quit while joined to a logger, or you switched networks
    /// yourself, turn the other networks' auto-connect back on (only once the
    /// computer is off the MyChron's network).
    pub fn restore_after_crash() {
        if restore_file().is_some_and(|p| p.exists()) && !current_wifi().is_some_and(|s| s.starts_with("AiM-")) {
            leave();
        }
    }
}

/// On a Mac, apps can't list WiFi networks without Location access, so you
/// join the MyChron's network from the menu bar and Mychronix finds the
/// logger there. Joining by name still works (`networksetup`), and nothing
/// is paused, so there's nothing to restore.
#[cfg(target_os = "macos")]
mod wifi {
    use super::*;

    pub const JOIN_HINT: &str = "Turn on the MyChron and its WiFi, then choose its network (AiM-MYC…) from the WiFi menu in the menu bar. Mychronix keeps looking.";

    fn networksetup(args: &[&str]) -> Result<String, String> {
        log::debug!("networksetup {}", args.join(" "));
        let out = Process::new("networksetup").args(args).output().map_err(|e| format!("networksetup: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        // networksetup reports some failures on stdout with a zero exit.
        if out.status.success() && !text.contains("Error") && !text.contains("Could not") {
            Ok(text)
        } else {
            let err = format!("{}{}", text.trim(), String::from_utf8_lossy(&out.stderr).trim());
            log::warn!("networksetup {} failed: {err}", args.join(" "));
            Err(err)
        }
    }

    /// The WiFi interface, e.g. "en0".
    fn interface() -> Option<String> {
        let out = networksetup(&["-listallhardwareports"]).ok()?;
        let mut lines = out.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "Hardware Port: Wi-Fi" {
                return lines.next()?.strip_prefix("Device: ").map(|d| d.trim().to_string());
            }
        }
        None
    }

    /// Not available without Location access; you pick the network yourself.
    pub(super) fn aim_networks() -> Vec<(String, u8)> {
        Vec::new()
    }

    /// The current network's name, when macOS shares it (recent versions may not).
    pub(super) fn current_wifi() -> Option<String> {
        let out = networksetup(&["-getairportnetwork", &interface()?]).ok()?;
        out.trim().strip_prefix("Current Wi-Fi Network: ").map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    pub(super) fn join(ssid: &str) -> Result<(), String> {
        let iface = interface().ok_or("No WiFi on this Mac")?;
        networksetup(&["-setairportnetwork", &iface, ssid]).map_err(|e| format!("Couldn't join {ssid}: {e}"))?;
        for _ in 0..20 {
            if aim_link::probe(AP_ADDRESS, Duration::from_millis(500)).is_ok() {
                return Ok(());
            }
        }
        Err(format!("Joined {ssid}, but the MyChron didn't answer"))
    }

    /// macOS goes back to a known network by itself.
    pub(super) fn leave() {}

    pub fn restore_wifi_now() {}
    pub fn restore_wifi_on_quit() {}

    pub fn paused_networks() -> bool {
        false
    }

    pub fn restore_after_crash() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn splits_nmcli_terse_fields() {
        use wifi::split_terse;
        assert_eq!(split_terse("AiM-MYC6-345678:72"), ["AiM-MYC6-345678", "72"]);
        assert_eq!(split_terse(r"Cafe\: Guest:802-11-wireless:yes"), ["Cafe: Guest", "802-11-wireless", "yes"]);
    }

    #[test]
    fn meta_sits_beside_the_session() {
        assert_eq!(SessionMeta::path_for(Path::new("/x/a_0035.xrz")), Path::new("/x/a_0035.meta.json"));
    }

    #[test]
    fn a_reused_name_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("mychronix-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entry = |size: u64, time: &str| DirEntry { name: "a_0001.xrz".into(), size, date: "12/27/2026".into(), time: time.into() };
        // Ann's a_0001, downloaded earlier.
        let ann = entry(5, "10:00:00");
        std::fs::write(dir.join("a_0001.xrz"), b"bens!").unwrap();
        SessionMeta { listed: listed(&ann), ..Default::default() }.save(&dir.join("a_0001.xrz")).unwrap();
        assert_eq!(downloaded(&dir, &ann), Some(dir.join("a_0001.xrz")));
        // After a format, a different a_0001 (same size even): not downloaded, and saved beside it.
        let bo = entry(5, "15:30:00");
        assert_eq!(downloaded(&dir, &bo), None);
        assert_eq!(target_for(&dir, &bo), dir.join("a_0001-2.xrz"));
        std::fs::write(dir.join("a_0001-2.xrz"), b"jacks").unwrap();
        SessionMeta { listed: listed(&bo), ..Default::default() }.save(&dir.join("a_0001-2.xrz")).unwrap();
        assert_eq!(downloaded(&dir, &bo), Some(dir.join("a_0001-2.xrz")));
        assert_eq!(std::fs::read(dir.join("a_0001.xrz")).unwrap(), b"bens!");
        assert_eq!(aim_xrk::summary::session_number("a_0001-2.xrz"), Some(1));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
