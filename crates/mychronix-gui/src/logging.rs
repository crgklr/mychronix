//! The app's log: `~/.cache/mychronix/mychronix.log`, rolled over to
//! `mychronix.log.old` past 1 MB. Mychronix's own messages at info and above
//! by default (`MYCHRONIX_LOG=debug` or `trace` for more, including every
//! frame exchanged with the MyChron); other libraries' only at warn and above.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_BYTES: u64 = 1_000_000;

struct FileLog {
    path: PathBuf,
    file: Mutex<Option<std::fs::File>>,
    ours: LevelFilter,
}

/// Mychronix's own crates: the app, the CLI and the aim-* libraries.
fn is_ours(target: &str) -> bool {
    target.starts_with("mychronix") || target.starts_with("aim_")
}

impl Log for FileLog {
    fn enabled(&self, m: &Metadata) -> bool {
        if is_ours(m.target()) {
            m.level() <= self.ours
        } else if m.target().starts_with("wgpu") || m.target().starts_with("naga") {
            // The graphics stack warns about every Vulkan driver it skips at startup.
            m.level() <= Level::Error
        } else {
            m.level() <= Level::Warn
        }
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("{} {:<5} {}: {}\n", timestamp(), r.level(), r.target(), r.args());
        let Ok(mut guard) = self.file.lock() else { return };
        // Roll over once the file passes the limit.
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() > MAX_BYTES) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("log.old"));
            *guard = open(&self.path);
        }
        if let Some(f) = guard.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut g) = self.file.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

fn open(path: &PathBuf) -> Option<std::fs::File> {
    std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
}

/// "2026-10-12 17:45:02.123Z" (UTC).
fn timestamp() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() as i64;
    let d = crate::events::from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}Z", d.year, d.month, d.day, s / 3600, s % 3600 / 60, s % 60, now.subsec_millis())
}

/// Where the log is.
pub fn path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/mychronix/mychronix.log"))
}

/// Starts logging to the file. Safe to call once; without a home folder it
/// does nothing.
pub fn init() {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let ours = match std::env::var("MYCHRONIX_LOG").unwrap_or_default().to_lowercase().as_str() {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Info,
    };
    let file = Mutex::new(open(&path));
    if log::set_boxed_logger(Box::new(FileLog { path, file, ours })).is_ok() {
        log::set_max_level(ours.max(LevelFilter::Warn));
    }
}
