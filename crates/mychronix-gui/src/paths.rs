//! Where Mychronix keeps its caches and logs: `~/.cache/mychronix` on Linux;
//! `~/Library/Caches/Mychronix` and `~/Library/Logs/Mychronix` on a Mac.
//! Your data stays in `~/Mychronix` everywhere.

use std::path::PathBuf;

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
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
