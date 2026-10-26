//! Background work. Every job runs on a named thread (the name shows in the
//! log and `crash.log`), and a crash inside one is contained: it's logged,
//! the job ends, and whoever was waiting sees its channel close instead of
//! waiting forever. `guard` does the same for one item of a longer job, so a
//! single damaged session file can't stop the rest.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `work` on a background thread called `name`.
pub fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    let label = name.to_string();
    let started = std::thread::Builder::new().name(name.to_string()).spawn(move || {
        if catch_unwind(AssertUnwindSafe(work)).is_err() {
            log::error!("background job \"{label}\" stopped unexpectedly (details in crash.log)");
        }
    });
    if let Err(e) = started {
        log::error!("couldn't start background job \"{name}\": {e}");
    }
}

/// Runs `f` for one item (`what` names it in the log); None if it crashed.
pub fn guard<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Some(v),
        Err(_) => {
            log::error!("skipped {what}: reading it crashed (details in crash.log)");
            None
        }
    }
}

/// Everything a job has sent so far, and whether it has ended (its sending
/// side is gone, normally or because it crashed). No receiver: nothing, ended.
pub fn drain<T>(rx: Option<&std::sync::mpsc::Receiver<T>>) -> (Vec<T>, bool) {
    let Some(rx) = rx else { return (Vec::new(), true) };
    let mut got = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(v) => got.push(v),
            Err(std::sync::mpsc::TryRecvError::Empty) => return (got, false),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return (got, true),
        }
    }
}

/// Opens a session file. A file that crashes the reader (damaged in a way
/// it doesn't expect) comes back as an error, like any unreadable file,
/// instead of taking the app or a background job down with it; so does one
/// with nothing recorded in it (the readers are lenient, so a file that
/// isn't a session at all can come back empty rather than failing).
pub fn open_session(path: &std::path::Path) -> Result<aim_xrk::Session, String> {
    let session = guard(&path.display().to_string(), || aim_xrk::Session::open(path))
        .ok_or_else(|| "it's damaged in a way Mychronix can't read".to_string())?
        .map_err(|e| e.to_string())?;
    if session.channels.iter().all(|c| c.values.is_empty()) {
        return Err("it has no recorded data (it may not be a session file)".into());
    }
    let mut session = session;
    relap(&mut session, path);
    Ok(session)
}

/// Laps timed from GPS when the logger's look untimed (one long lap, or laps
/// run together), against the start/finish lines of the tracks within a few
/// kilometers of where the session was recorded (see `aim_xrk::relap`).
/// The session's own track's line goes first (the one you picked, else the
/// one its MyChron recorded), so a session at one configuration of a complex
/// isn't timed from another's line; then your tracks', then AiM's.
fn relap(session: &mut aim_xrk::Session, path: &std::path::Path) {
    if !aim_xrk::relap::needs_retiming(session) {
        return;
    }
    let Some(gps) = session.gps.as_ref() else { return };
    let (Some(&lat), Some(&lon)) = (gps.latitude.first(), gps.longitude.first()) else { return };
    let (own, aim) = crate::tracks::start_finishes_near(lat, lon, 3_000.0);
    let picked = crate::device::SessionMeta::load(path).and_then(|m| m.track_guid).filter(|g| !g.is_empty());
    let recorded = || std::fs::read(path.with_extension("xri")).ok().and_then(|b| aim_xrk::parse_xri(&b).ok()).and_then(|s| s.track_guid);
    let mine = picked.or_else(recorded);
    let first: Vec<crate::tracks::Line> = own.iter().chain(&aim).filter(|l| mine.as_deref() == Some(l.guid.as_str())).cloned().collect();
    let before = session.laps.len();
    for lines in [first, own, aim] {
        let at: Vec<(f64, f64)> = lines.iter().map(|l| l.at).collect();
        if let Some((k, laps)) = aim_xrk::relap::fix(session, &at) {
            log::info!("{}: {before} lap(s) as the logger timed it; {} timed from GPS against start/finish {:?}", path.display(), laps.len(), at[k]);
            session.laps = laps;
            session.info.laps_from_gps = true;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crash_is_contained() {
        assert_eq!(guard("a test item", || 7), Some(7));
        assert_eq!(guard("a test item", || -> i32 { panic!("damaged") }), None);
        let (tx, rx) = std::sync::mpsc::channel::<i32>();
        spawn("test-job", move || {
            let _keep = tx;
            panic!("damaged");
        });
        // The job ended, so its channel closed instead of hanging.
        assert!(rx.recv().is_err());
    }
}
