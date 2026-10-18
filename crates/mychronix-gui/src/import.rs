//! Importing session files from elsewhere (RaceStudio's folders, a USB
//! stick, a friend): `.xrk`/`.xrz` from a MyChron 5/6 and RaceStudio 2's
//! `.drk` (with its GPS and raw-download files) or a MyChron 4 run.
//!
//! Each session lands in `~/Mychronix/sessions/<logger serial>/` exactly as a
//! download would: a MyChron 5/6 recording as the logger's compressed `.xrz`
//! with an `.xri` summary beside it (written by Mychronix, see
//! `aim_xrk::summary::to_xri`), a RaceStudio 2 session with its companions.
//! Sessions already in the library (same MyChron, date and start time) are
//! skipped, and nothing is ever overwritten.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;


pub enum Event {
    Progress { done: usize, total: usize },
    Finished(Report),
}

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub added: usize,
    /// Already in the library.
    pub already: usize,
    /// Files that couldn't be read as sessions.
    pub unreadable: Vec<String>,
    /// The MyChrons the added sessions came from.
    pub loggers: Vec<String>,
    /// Folders searched (their names), whose other files were left alone.
    pub searched: Vec<String>,
    /// Files chosen or dropped directly that aren't session files (their names).
    pub not_sessions: Vec<String>,
}

fn name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

const MYCHRON_5_6: [&str; 2] = ["xrk", "xrz"];
const RS2: [&str; 3] = ["drk", "imd", "mc4"];
const MYCHRONIX_FILE: [&str; 5] = ["xrk", "xrz", "drk", "imd", "mc4"];

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// The session files under `paths` (files, or folders searched through),
/// one per session: RaceStudio saves a MyChron 5/6 session as `.xrk` and
/// `.drk` side by side, so a `.drk` with an `.xrk`/`.xrz` beside it is left
/// out, and so is an `.imd`/`.mc4` beside its `.drk`. Files already inside
/// `library` are left out too.
pub fn collect(paths: &[PathBuf], library: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for p in paths {
        walk(p, 0, &mut files);
    }
    files.retain(|f| !f.starts_with(library));
    let has = |f: &Path, exts: &[&str]| exts.iter().any(|e| f.with_extension(e).exists() || f.with_extension(e.to_ascii_uppercase()).exists());
    files.retain(|f| match ext(f).as_str() {
        "drk" => !has(f, &MYCHRON_5_6),
        "imd" | "mc4" => !has(f, &["drk"]),
        _ => true,
    });
    files.sort();
    files.dedup();
    files
}

fn walk(p: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        // Hidden folders (and very deep trees) aren't searched.
        if depth > 8 || (depth > 0 && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))) {
            return;
        }
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            walk(&e.path(), depth + 1, out);
        }
    } else if MYCHRON_5_6.contains(&ext(p).as_str()) || RS2.contains(&ext(p).as_str()) {
        out.push(p.to_path_buf());
    }
}

/// A session's identity for spotting one already in the library: its
/// MyChron, date and start time in seconds.
type Key = (String, (u16, u8, u8), i64);

fn key(logger: &str, s: &aim_xrk::SessionSummary) -> Option<Key> {
    let d = s.date?;
    let p: Vec<i64> = s.start_time.as_deref()?.split(':').filter_map(|x| x.trim().parse().ok()).collect();
    (p.len() == 3).then(|| (logger.to_string(), (d.year, d.month, d.day), p[0] * 3600 + p[1] * 60 + p[2]))
}

/// Whether `k` is already known, allowing two seconds either way: a summary
/// worked out from the file can differ from the logger's by a second. Adds it if not.
fn seen(known: &mut HashSet<Key>, k: Key) -> bool {
    let hit = (-2..=2).any(|d| known.contains(&(k.0.clone(), k.1, k.2 + d)));
    if !hit {
        known.insert(k);
    }
    hit
}

/// Imports in the background; events report progress.
pub fn spawn(paths: Vec<PathBuf>, lib: &crate::tracks::Library, ctx: eframe::egui::Context) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    let Some(root) = crate::paths::sessions_dir() else { return rx };
    let mut known: HashSet<_> = lib.sessions.iter().filter_map(|s| key(&s.logger, &s.summary)).collect();
    crate::jobs::spawn("import", move || {
        let files = collect(&paths, &root);
        let total = files.len();
        log::info!("import: {total} session files found under {paths:?}");
        let _ = tx.send(Event::Progress { done: 0, total });
        ctx.request_repaint();
        let mut report = Report {
            searched: paths.iter().filter(|p| p.is_dir()).map(|p| name(p)).collect(),
            not_sessions: paths.iter().filter(|p| p.is_file() && !MYCHRONIX_FILE.iter().any(|e| *e == ext(p))).map(|p| name(p)).collect(),
            ..Default::default()
        };
        for (i, f) in files.iter().enumerate() {
            // A file that trips up a reader counts as unreadable; the import carries on.
            let outcome = crate::jobs::guard(&f.display().to_string(), || import_one(f, &root, &mut known)).unwrap_or_else(|| Err("couldn't be read".into()));
            match outcome {
                Ok(Some(logger)) => {
                    report.added += 1;
                    if !report.loggers.contains(&logger) {
                        report.loggers.push(logger);
                    }
                }
                Ok(None) => report.already += 1,
                Err(e) => {
                    log::info!("import: {}: {e}", f.display());
                    report.unreadable.push(f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                }
            }
            let _ = tx.send(Event::Progress { done: i + 1, total });
            ctx.request_repaint();
        }
        log::info!("import: {} added, {} already here, {} unreadable", report.added, report.already, report.unreadable.len());
        let _ = tx.send(Event::Finished(report));
        ctx.request_repaint();
    });
    rx
}

/// A free name in `dir` for `stem.ext`: `stem.ext`, else `stem-2.ext`, …
fn free_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") };
        let p = dir.join(name);
        if !p.exists() && !p.with_extension("xri").exists() && !p.with_extension("drk").exists() {
            return p;
        }
        n += 1;
    }
}

/// Imports one session. Some(logger) when added, None when already there.
fn import_one(path: &Path, root: &Path, known: &mut HashSet<Key>) -> Result<Option<String>, String> {
    let session = crate::jobs::open_session(path)?;
    let logger = session.info.logger_id.map(|id| id.to_string()).unwrap_or_else(|| "imported".into());
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "session".into());
    let dir = root.join(&logger);
    let e = ext(path);
    if MYCHRON_5_6.contains(&e.as_str()) {
        let target = free_name(&dir, &stem, "xrz");
        let mut summary = aim_xrk::summary::summarize(&session, target.file_name().and_then(|n| n.to_str()));
        if key(&logger, &summary).is_some_and(|k| seen(known, k)) {
            return Ok(None);
        }
        summary.file = target.file_name().map(|n| n.to_string_lossy().into_owned());
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // Stored as the logger stores it: the .xrk compressed (an .xrz already is).
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let xrz = if e == "xrz" {
            bytes
        } else {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(&bytes).and_then(|_| z.finish()).map_err(|e| e.to_string())?
        };
        write_new(&target, &xrz)?;
        write_new(&target.with_extension("xri"), &aim_xrk::summary::to_xri(&summary))?;
    } else {
        let summary = if e == "drk" { aim_xrk::drk::summary(path) } else { aim_xrk::mc4::summary(path) }.map_err(|e| e.to_string())?;
        if key(&logger, &summary).is_some_and(|k| seen(known, k)) {
            return Ok(None);
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // The session and the files RaceStudio 2 keeps beside it, under one free name.
        let target = free_name(&dir, &stem, &e);
        let new_stem = target.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let src_dir = path.parent().unwrap_or(Path::new("."));
        for entry in std::fs::read_dir(src_dir).map_err(|e| e.to_string())?.flatten() {
            let p = entry.path();
            let companion = ["drk", "imd", "gpk", "rrk", "mc4"].contains(&ext(&p).as_str());
            if companion && p.file_stem().map(|s| s.to_string_lossy()) == Some(stem.as_str().into()) {
                write_new(&dir.join(format!("{new_stem}.{}", ext(&p))), &std::fs::read(&p).map_err(|e| e.to_string())?)?;
            }
        }
    }
    Ok(Some(logger))
}

/// Writes a file that mustn't exist yet, by way of a temporary name.
fn write_new(path: &Path, data: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    crate::storage::write_atomic(path, data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn racestudio_twins_count_once() {
        let d = std::env::temp_dir().join(format!("mychronix-collect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub/.hidden")).unwrap();
        for f in ["a.xrk", "a.drk", "a.rrk", "b.drk", "b.imd", "c.mc4", "sub/d.XRZ", "sub/.hidden/e.xrk", "notes.txt"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let names: Vec<String> = collect(&[d.clone()], &d.join("library")).iter().map(|p| p.strip_prefix(&d).unwrap().display().to_string()).collect();
        assert_eq!(names, ["a.xrk", "b.drk", "c.mc4", "sub/d.XRZ"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names_never_collide() {
        let d = std::env::temp_dir().join(format!("mychronix-names-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a_0001.xrz"), b"x").unwrap();
        std::fs::write(d.join("a_0001-2.xri"), b"x").unwrap();
        assert_eq!(free_name(&d, "a_0001", "xrz"), d.join("a_0001-3.xrz"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
