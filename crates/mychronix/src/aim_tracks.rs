//! AiM's track database, kept current from AiM's own server: the same files
//! RaceStudio 3 downloads, so nobody needs RaceStudio for the track list.
//!
//! - `web_tracks.ria`: every track (128 bytes each, ~5,600 tracks, ~700 KB).
//! - `Tracks/<id>.tkk`: one track with its outline (~12 KB each, ~70 MB in all).
//!
//! Kept in `~/Mychronix/aim-tracks/`. The first run downloads every track;
//! after that, each start asks for the list with the ETag of the last one (an
//! unchanged list costs a single "not modified" reply) and downloads only
//! tracks that are new or whose record changed, and deletes removed ones. An
//! interrupted run picks up where it stopped on the next start.
//!
//! Offline is normal: without a connection the app keeps using what it has
//! (the saved list, else the list copied from a MyChron 5/6, see
//! `mychron_list_path`) and tries again on the next start.
//!
//! Only AiM's files are requested; nothing about the user is sent.
//! `"aim_tracks": false` in `~/Mychronix/settings.json` turns it off.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use aim_track::RIA_RECORD;

const BASE: &str = "https://aimsa.blob.core.windows.net/tracks/";
/// Downloads at a time. The files are small, so time goes to round trips;
/// 16 at once fetches the whole set in about a minute. Still modest for a
/// file server built for this (it's what serves RaceStudio 3 too).
const WORKERS: usize = 16;

pub enum Event {
    /// The track list was saved for the first time (outlines still coming).
    List,
    Progress { done: usize, total: usize },
    /// Finished; `changed` tracks were new or updated.
    Finished { changed: usize },
    /// Couldn't finish (no connection, or the server said no); tried again next start.
    Failed { offline: bool },
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct State {
    /// The ETag of the list saved last.
    etag: Option<String>,
}

/// `~/Mychronix/aim-tracks`.
pub fn dir() -> Option<PathBuf> {
    crate::paths::data("aim-tracks")
}

/// The saved track list, once there is one.
pub fn list_path() -> Option<PathBuf> {
    dir().map(|d| d.join("aim_tracks.ria")).filter(|p| p.exists())
}

/// Where a MyChron 5/6's own copy of AiM's list (`0:/tkk/al.ria`: names and
/// start/finish lines, no outlines) is kept: the fallback for a computer that
/// hasn't been online yet.
pub fn mychron_list_target() -> Option<PathBuf> {
    dir().map(|d| d.join("from-mychron.ria"))
}

pub fn mychron_list_path() -> Option<PathBuf> {
    mychron_list_target().filter(|p| p.exists())
}

fn enabled() -> bool {
    crate::settings::get().aim_tracks
}

/// A track id as AiM writes them: eight lowercase letters and digits. Anything
/// else is skipped, so a list can't name a file outside the folder.
fn is_id(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Each record of a `.ria`, by track id.
fn records(buf: &[u8]) -> HashMap<String, &[u8]> {
    buf.as_chunks::<RIA_RECORD>()
        .0
        .iter()
        .filter_map(|r| {
            let id = String::from_utf8_lossy(&r[56..64]).trim_end_matches('\0').to_string();
            is_id(&id).then_some((id, r.as_slice()))
        })
        .collect()
}

/// The tracks to download: new ones, changed ones, and any whose file is missing.
fn wanted(old: &[u8], new: &[u8], has_file: impl Fn(&str) -> bool) -> Vec<String> {
    let old = records(old);
    let mut ids: Vec<String> = records(new).into_iter().filter(|(id, r)| old.get(id) != Some(r) || !has_file(id)).map(|(id, _)| id).collect();
    ids.sort();
    ids
}

/// Writes `data` to `path` by way of a temporary file, so a half-written
/// file is never left under the real name.
/// Syncs in the background; events report progress.
pub fn spawn(ctx: eframe::egui::Context) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    if !enabled() {
        return rx;
    }
    crate::jobs::spawn("aim-tracks", move || match sync(&tx, &ctx) {
        Ok(changed) => {
            let _ = tx.send(Event::Finished { changed });
            ctx.request_repaint();
        }
        Err(e) => {
            log::info!("AiM tracks: {}; will try again next start", e.0);
            let _ = tx.send(Event::Failed { offline: e.1 });
            ctx.request_repaint();
        }
    });
    rx
}

/// Why a sync stopped, and whether it looks like there's no connection.
struct Stopped(String, bool);

impl From<String> for Stopped {
    fn from(s: String) -> Self {
        Stopped(s, false)
    }
}

impl From<&str> for Stopped {
    fn from(s: &str) -> Self {
        Stopped(s.to_string(), false)
    }
}

fn offline(e: reqwest::Error) -> Stopped {
    Stopped(e.to_string(), e.is_connect() || e.is_timeout())
}

fn sync(tx: &mpsc::Sender<Event>, ctx: &eframe::egui::Context) -> Result<usize, Stopped> {
    let dir = dir().ok_or("no home folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let list = dir.join("aim_tracks.ria");
    let state_path = dir.join("sync.json");
    let mut state: State = crate::storage::load_or_default(Some(state_path.clone()));
    let old = std::fs::read(&list).unwrap_or_default();

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Mychronix/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.get(format!("{BASE}web_tracks.ria"));
    if let (Some(etag), false) = (&state.etag, old.is_empty()) {
        req = req.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let r = req.send().map_err(offline)?;
    if r.status() == reqwest::StatusCode::NOT_MODIFIED {
        log::info!("AiM tracks: list unchanged");
        return Ok(0);
    }
    let r = r.error_for_status().map_err(|e| e.to_string())?;
    let etag = r.headers().get(reqwest::header::ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
    let new = r.bytes().map_err(offline)?.to_vec();
    if new.is_empty() || new.len() % RIA_RECORD != 0 || aim_track::parse_ria(&new).is_err() {
        return Err(format!("unexpected track list ({} bytes)", new.len()).into());
    }
    let need = wanted(&old, &new, |id| dir.join(format!("{id}.tkk")).exists());
    let total = need.len();
    log::info!("AiM tracks: {} in the list, {total} to download", new.len() / RIA_RECORD);

    // On the first run, save the list now so the tracks show while their outlines arrive.
    if old.is_empty() {
        crate::storage::write_atomic(&list, &new).map_err(|e| e.to_string())?;
        let _ = tx.send(Event::List);
        ctx.request_repaint();
    }

    let queue = Arc::new(Mutex::new(need.into_iter()));
    let (done, failed) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let stop = Arc::new(AtomicBool::new(false));
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let (queue, done, failed, stop) = (queue.clone(), done.clone(), failed.clone(), stop.clone());
            let (client, dir, tx, ctx) = (client.clone(), dir.clone(), tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let Some(id) = queue.lock().unwrap().next() else { break };
                    match client.get(format!("{BASE}Tracks/{id}.tkk")).send() {
                        Ok(r) if r.status().is_success() => match r.bytes() {
                            Ok(b) if !b.is_empty() => {
                                if let Err(e) = crate::storage::write_atomic(&dir.join(format!("{id}.tkk")), &b) {
                                    log::warn!("AiM tracks: couldn't save {id}.tkk: {e}");
                                    failed.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            _ => {
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                        },
                        // Listed but not on the server: nothing to fetch, not an error.
                        Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => log::debug!("AiM tracks: {id}.tkk not on the server"),
                        // Busy or refusing: stop for now rather than keep asking.
                        Ok(r) => {
                            log::info!("AiM tracks: server answered {} for {id}.tkk; stopping", r.status());
                            stop.store(true, Ordering::Relaxed);
                        }
                        Err(e) => {
                            log::debug!("AiM tracks: {id}.tkk: {e}");
                            if failed.fetch_add(1, Ordering::Relaxed) > 20 {
                                stop.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 25 == 0 || n == total {
                        let _ = tx.send(Event::Progress { done: n, total });
                        ctx.request_repaint();
                    }
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    let failed = failed.load(Ordering::Relaxed);
    if stop.load(Ordering::Relaxed) || failed > 0 {
        let left = total - done.load(Ordering::Relaxed) + failed;
        return Err(Stopped(format!("{left} tracks still to download"), failed > 0));
    }

    // Tracks AiM removed.
    let current = records(&new);
    for id in records(&old).keys().filter(|id| !current.contains_key(*id)) {
        let _ = std::fs::remove_file(dir.join(format!("{id}.tkk")));
    }
    crate::storage::write_atomic(&list, &new).map_err(|e| e.to_string())?;
    state.etag = etag;
    crate::storage::save(&state_path, &state).map_err(|e| e.to_string())?;
    log::info!("AiM tracks: up to date ({total} downloaded)");
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, name: &str) -> Vec<u8> {
        let mut r = vec![0u8; RIA_RECORD];
        r[..name.len()].copy_from_slice(name.as_bytes());
        r[56..56 + id.len()].copy_from_slice(id.as_bytes());
        r
    }

    #[test]
    fn only_new_changed_and_missing_tracks_are_wanted() {
        let old = [record("aaaa0001", "One"), record("aaaa0002", "Two"), record("aaaa0003", "Three")].concat();
        let new = [record("aaaa0002", "Two"), record("aaaa0001", "One, renamed"), record("aaaa0004", "Four"), record("aaaa0003", "Three")].concat();
        // Three's file is missing; order in the list doesn't matter.
        let want = wanted(&old, &new, |id| id != "aaaa0003");
        assert_eq!(want, ["aaaa0001", "aaaa0003", "aaaa0004"]);
        // Nothing saved yet: everything.
        assert_eq!(wanted(&[], &new, |_| false).len(), 4);
    }

    #[test]
    fn odd_ids_are_skipped() {
        let new = [record("../../x", "Bad"), record("abcd1234", "Good")].concat();
        assert_eq!(wanted(&[], &new, |_| false), ["abcd1234"]);
    }
}
