//! Where a session downloaded from a logger is saved, and whether it already
//! is: shared by the app and `mychronix-cli` so neither ever saves over a
//! different recording that reuses a name (a formatted MyChron numbers its
//! sessions from `a_0001` again).

use std::path::{Path, PathBuf};

use crate::client::DirEntry;

/// A listed file's date and time ("10/10/2026 14:39:36"), noted beside a
/// download so a different recording under the same name is told apart.
pub fn listed(entry: &DirEntry) -> String {
    format!("{} {}", entry.date.trim(), entry.time.trim()).trim().to_string()
}

/// `a_0001.xrz`, then `a_0001-2.xrz`, `a_0001-3.xrz`…: where a recording
/// named `name` may be saved in `dir`.
fn candidates(dir: &Path, name: &str) -> impl Iterator<Item = PathBuf> {
    let p = Path::new(name);
    let (stem, ext) = (p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default());
    let dir = dir.to_path_buf();
    let name = name.to_string();
    (1..).map(move |k| dir.join(if k == 1 { name.clone() } else { format!("{stem}-{k}.{ext}") }))
}

/// `a_0001.xrz` for `a_0001.hrz` and back. A MyChron 6 can list a session
/// under both names, byte for byte the same, and the two would share their
/// details and setup sheet (`a_0001.meta.json`, `a_0001.setup.json`), so
/// they're one recording.
pub fn twin(path: &Path) -> Option<PathBuf> {
    let other = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "xrz" => "hrz",
        "hrz" => "xrz",
        _ => return None,
    };
    Some(path.with_extension(other))
}

/// The local copy of `entry`, if it's been downloaded into `dir` (under its
/// own name or its twin's): the same size, and the same listed date and time
/// when the download noted one (`saved_listed` reads it for a saved file).
pub fn downloaded(dir: &Path, entry: &DirEntry, saved_listed: impl Fn(&Path) -> Option<String>) -> Option<PathBuf> {
    let names = std::iter::once(PathBuf::from(&entry.name)).chain(twin(Path::new(&entry.name)));
    names.into_iter().find_map(|name| {
        candidates(dir, &name.to_string_lossy()).take_while(|p| p.exists()).find(|p| {
            let same_size = std::fs::metadata(p).is_ok_and(|m| m.len() == entry.size);
            let same_time = saved_listed(p).filter(|l| !l.is_empty()).is_none_or(|l| l == listed(entry));
            same_size && same_time
        })
    })
}

/// Whether `entry` is the `.hrz` twin of an `.xrz` the logger also lists,
/// the same size: one recording, downloaded once.
pub fn listed_twice(files: &[DirEntry], entry: &DirEntry) -> bool {
    let name = Path::new(&entry.name);
    name.extension().is_some_and(|e| e.eq_ignore_ascii_case("hrz"))
        && twin(name).is_some_and(|t| files.iter().any(|f| Path::new(&f.name) == t && f.size == entry.size))
}

/// Where a new download of `entry` goes: its own name, or the next free
/// numbered one when that (or its twin, whose details it would share) is
/// taken. Nothing already downloaded is replaced.
pub fn target_for(dir: &Path, entry: &DirEntry) -> PathBuf {
    candidates(dir, &entry.name).find(|p| !p.exists() && !twin(p).is_some_and(|t| t.exists())).expect("an unused name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_reusing_a_name_is_saved_beside_the_old_one() {
        let dir = std::env::temp_dir().join(format!("aim-link-saved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entry = |time: &str| DirEntry { name: "a_0001.xrz".into(), size: 5, date: "10/31/2026".into(), time: time.into() };
        let old = entry("12:58:46");
        std::fs::write(dir.join("a_0001.xrz"), b"12345").unwrap();
        let noted = |p: &Path| (p == dir.join("a_0001.xrz")).then(|| listed(&old));
        assert_eq!(downloaded(&dir, &old, noted), Some(dir.join("a_0001.xrz")));
        // Same name and size after a format, recorded later: not downloaded, and never saved over the old one.
        let new = entry("15:30:00");
        assert_eq!(downloaded(&dir, &new, noted), None);
        assert_eq!(target_for(&dir, &new), dir.join("a_0001-2.xrz"));
        // Its .hrz twin counts as the same recording.
        let hrz = DirEntry { name: "a_0001.hrz".into(), ..old.clone() };
        assert_eq!(downloaded(&dir, &hrz, noted), Some(dir.join("a_0001.xrz")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
