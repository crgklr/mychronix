//! The Device page's worker: finds the MyChron's WiFi, joins it, talks to the
//! logger and downloads sessions, all on a background thread so the window
//! never stalls. The UI sends [`Command`]s and reads [`Event`]s.
//!
//! WiFi goes through NetworkManager (`nmcli`). While joined to the logger,
//! other WiFi networks stop auto-joining so the computer doesn't wander back
//! to one with internet; they're restored when you leave, or at the next
//! start if Mychronix quit while joined (see [`restore_after_crash`]).

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use aim_link::{AP_ADDRESS, Beacon, DirEntry, Logger};
use aim_xrk::SessionSummary;
use serde::{Deserialize, Serialize};

pub enum Command {
    /// Look for a logger at 10.0.0.1 and list AiM networks in range.
    Scan,
    Join { ssid: String },
    Leave,
    /// Read the logger's session list (folder listing + summary file).
    List,
    /// Each file with the details to save beside it.
    Download { files: Vec<(DirEntry, SessionMeta)>, dir: PathBuf },
}

pub enum Event {
    /// Result of a scan: the logger if one answered, AiM networks in range, and the current WiFi network.
    Scanned { logger: Option<Beacon>, networks: Vec<(String, u8)>, current: Option<String> },
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
        std::thread::spawn(move || {
            let send = |e: Event| {
                let _ = ev_tx.send(e);
                ctx.request_repaint();
            };
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    Command::Scan => {
                        let logger = aim_link::probe(AP_ADDRESS, Duration::from_millis(700)).ok();
                        let (networks, current) = if logger.is_some() { (Vec::new(), current_wifi()) } else { (aim_networks(), current_wifi()) };
                        log::debug!(
                            "scan: logger {:?}, on WiFi {:?}, MyChron networks {:?}",
                            logger.as_ref().map(|b| (b.logger_id, b.round_trip)),
                            current,
                            networks
                        );
                        send(Event::Scanned { logger, networks, current });
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

fn list() -> Result<(Vec<DirEntry>, Vec<SessionSummary>, usize), String> {
    let mut logger = Logger::connect_ap().map_err(|e| format!("Couldn't connect to the MyChron: {e}"))?;
    let listing = logger.list(aim_link::SESSIONS_DIR).map_err(|e| format!("Couldn't list sessions: {e}"))?;
    let summaries = listing
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case("summary1.csv"))
        .and_then(|_| logger.read_file("1:/mem/summary1.csv", |_, _| {}).ok())
        .map(|csv| aim_xrk::parse_summary_csv(&String::from_utf8_lossy(&csv)))
        .unwrap_or_default();
    let new_tracks = copy_custom_tracks(&mut logger);
    Ok((listing, summaries, new_tracks))
}

/// Copies the logger's custom tracks (listed in `0:/tkk/dev.ria`, stored as
/// `0:/tkk/o/<id>.tkk`) into `~/Mychronix/tracks/custom` when they're new, so
/// sessions recorded on them have a track to attach to. RaceStudio does the
/// same while connecting. Returns how many were copied.
fn copy_custom_tracks(logger: &mut Logger) -> usize {
    let Some(dir) = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("tracks").join("custom")) else { return 0 };
    let Ok(index) = logger.read_file("0:/tkk/dev.ria", |_, _| {}) else { return 0 };
    let Ok(tracks) = aim_track::parse_ria(&index) else { return 0 };
    let _ = std::fs::create_dir_all(&dir);
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
        if let Ok(bytes) = logger.read_file(&format!("0:/tkk/o/{}.tkk", t.id), |_, _| {}) {
            if aim_track::parse_tkk(&bytes).is_ok() && std::fs::write(&target, bytes).is_ok() {
                copied += 1;
            }
        }
    }
    copied
}

/// Downloads each session (and its `.xri` summary) into `dir`, writing the
/// metadata beside it. Files land under a temporary name first, so a
/// canceled download never looks complete.
fn download(files: &[(DirEntry, SessionMeta)], dir: &Path, send: &impl Fn(Event)) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let mut logger = Logger::connect_ap().map_err(|e| format!("Couldn't connect to the MyChron: {e}"))?;
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
        let target = dir.join(&entry.name);
        let partial = target.with_extension("part");
        std::fs::write(&partial, &data).and_then(|_| std::fs::rename(&partial, &target)).map_err(|e| e.to_string())?;
        log::info!("saved {} ({} bytes)", target.display(), data.len());
        // The small summary beside it, used by the Sessions and Tracks pages.
        let xri = Path::new(&entry.name).with_extension("xri");
        if let Ok(bytes) = logger.read_file(&format!("{}/{}", aim_link::SESSIONS_DIR, xri.display()), |_, _| {}) {
            let _ = std::fs::write(dir.join(&xri), bytes);
        }
        meta.save(&target).map_err(|e| e.to_string())?;
        saved += 1;
    }
    Ok(saved)
}

// ---------------------------------------------------------------------------
// WiFi via NetworkManager

fn restore_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("wifi-restore.json"))
}

#[derive(Serialize, Deserialize, Default)]
struct Restore {
    /// Connection profiles whose auto-join we paused.
    paused: Vec<String>,
    /// The network to go back to.
    previous: Option<String>,
}

fn nmcli(args: &[&str]) -> Result<String, String> {
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
fn split_terse(line: &str) -> Vec<String> {
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
fn aim_networks() -> Vec<(String, u8)> {
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

fn current_wifi() -> Option<String> {
    nmcli(&["-t", "-f", "ACTIVE,SSID", "device", "wifi"])
        .ok()?
        .lines()
        .map(split_terse)
        .find(|f| f.first().is_some_and(|a| a == "yes"))
        .and_then(|f| f.get(1).cloned())
        .filter(|s| !s.is_empty())
}

fn join(ssid: &str) -> Result<(), String> {
    let previous = current_wifi().filter(|s| !s.starts_with("AiM-"));
    // Stop other WiFi profiles from pulling the computer back to the internet.
    let mut paused = Vec::new();
    for line in nmcli(&["-t", "-f", "NAME,TYPE,AUTOCONNECT", "connection", "show"])?.lines() {
        let f = split_terse(line);
        if f.len() >= 3 && f[1] == "802-11-wireless" && f[2] == "yes" && f[0] != ssid {
            if nmcli(&["connection", "modify", &f[0], "connection.autoconnect", "no"]).is_ok() {
                paused.push(f[0].clone());
            }
        }
    }
    if let Some(path) = restore_file() {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, serde_json::to_string(&Restore { paused: paused.clone(), previous }).unwrap_or_default());
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

fn leave() {
    let Some(path) = restore_file() else { return };
    let Some(restore) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Restore>(&s).ok()) else { return };
    for name in &restore.paused {
        let _ = nmcli(&["connection", "modify", name, "connection.autoconnect", "yes"]);
    }
    // Already back on a network of your choosing: leave it be.
    let current = current_wifi();
    if current.as_deref().is_some_and(|c| !c.starts_with("AiM-")) {
        let _ = std::fs::remove_file(path);
        return;
    }
    match &restore.previous {
        Some(ssid) => {
            let _ = nmcli(&["connection", "up", ssid]).or_else(|_| nmcli(&["device", "wifi", "connect", ssid]));
        }
        None => {
            let _ = nmcli(&["device", "disconnect", "wifi"]);
        }
    }
    let _ = std::fs::remove_file(path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_nmcli_terse_fields() {
        assert_eq!(split_terse("AiM-MYC6-345678:72"), ["AiM-MYC6-345678", "72"]);
        assert_eq!(split_terse(r"Cafe\: Guest:802-11-wireless:yes"), ["Cafe: Guest", "802-11-wireless", "yes"]);
    }

    #[test]
    fn meta_sits_beside_the_session() {
        assert_eq!(SessionMeta::path_for(Path::new("/x/a_0035.xrz")), Path::new("/x/a_0035.meta.json"));
    }
}
