//! Reading and writing Mychronix's own JSON files safely.
//!
//! Saves go to a temporary file that's then renamed over the old one, so a
//! crash mid-save never leaves half a file. A file that can't be read back
//! (damaged, or from a newer version) is moved aside as
//! `<name>.damaged-<time>.json` rather than quietly treated as empty, where
//! the next save would overwrite it; the app shows a message about it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Serialize, de::DeserializeOwned};

static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Messages about damaged files, for the app to show once.
pub fn take_warnings() -> Vec<String> {
    WARNINGS.lock().map(|mut w| std::mem::take(&mut *w)).unwrap_or_default()
}

fn warn(message: String) {
    log::warn!("{message}");
    if let Ok(mut w) = WARNINGS.lock() {
        w.push(message);
    }
}

/// Reads a JSON file. Missing → None. Damaged → moved aside, a warning
/// recorded, and None.
pub fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn(format!("Couldn't read {}: {e}", path.display()));
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            let aside = damaged_name(path);
            let moved = std::fs::rename(path, &aside).is_ok();
            warn(if moved {
                format!("{} was damaged ({e}). It's been kept as {} and Mychronix started that part afresh.", file_label(path), file_label(&aside))
            } else {
                format!("{} is damaged ({e}) and couldn't be moved aside.", file_label(path))
            });
            None
        }
    }
}

/// Reads a JSON file, or the default when it's missing or damaged.
pub fn load_or_default<T: DeserializeOwned + Default>(path: Option<PathBuf>) -> T {
    path.and_then(|p| load(&p)).unwrap_or_default()
}

/// Writes `value` as JSON: to a temporary file, then renamed into place.
pub fn save<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    log::debug!("saving {}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn damaged_name(path: &Path) -> PathBuf {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let stem = path.file_name().map(|n| n.to_string_lossy().trim_end_matches(".json").to_string()).unwrap_or_default();
    path.with_file_name(format!("{stem}.damaged-{secs}.json"))
}

/// "events.json" or "a_0035.setup.json" for messages.
fn file_label(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq, Serialize, serde::Deserialize)]
    struct Thing {
        n: u32,
    }

    #[test]
    fn round_trip_and_damaged_files() {
        let dir = std::env::temp_dir().join(format!("mychronix-storage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("things.json");
        assert_eq!(load::<Thing>(&path), None);
        save(&path, &Thing { n: 7 }).unwrap();
        assert_eq!(load::<Thing>(&path), Some(Thing { n: 7 }));
        assert!(!dir.join("things.json.tmp").exists());

        // Damaged: moved aside, a warning recorded, and the default used.
        std::fs::write(&path, "{ not json").unwrap();
        take_warnings();
        assert_eq!(load_or_default::<Thing>(Some(path.clone())), Thing::default());
        assert!(!path.exists());
        let kept: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("damaged")).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(take_warnings().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
