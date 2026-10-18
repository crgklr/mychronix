//! Where Mychronix keeps its caches and logs: `~/.cache/mychronix` on Linux;
//! `~/Library/Caches/Mychronix` and `~/Library/Logs/Mychronix` on a Mac.
//! Your data stays in `~/Mychronix` everywhere.

use std::path::PathBuf;

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Your data, the same everywhere: `~/Mychronix`.
pub fn data_dir() -> Option<PathBuf> {
    home().map(|h| h.join("Mychronix"))
}

/// A file or folder in the data folder, e.g. `data("profiles.json")`.
pub fn data(name: &str) -> Option<PathBuf> {
    data_dir().map(|d| d.join(name))
}

/// Where downloads and imports go: `~/Mychronix/sessions`, a folder per MyChron.
pub fn sessions_dir() -> Option<PathBuf> {
    data("sessions")
}

/// Map tiles and other things that can be thrown away and rebuilt.
pub fn cache_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Caches/Mychronix"))
    } else {
        home().map(|h| h.join(".cache/mychronix"))
    }
}

/// The log and crash log.
pub fn log_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Logs/Mychronix"))
    } else {
        cache_dir()
    }
}
