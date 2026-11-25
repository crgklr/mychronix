//! A track's corners, saved once so that every session there numbers them the
//! same way: "corner 3" is the same piece of track in every session, for every
//! driver. Kept in `~/Mychronix/corners/<track id>.json`, beside rather than
//! inside the track's own file, so nothing a MyChron or RaceStudio reads
//! changes.
//!
//! The first session at a track with corners sets them (from its fastest lap
//! recorded in full); "Use this session's corners" on the Corners page sets
//! them again. Each session then finds the saved corners on its own fastest
//! lap (see `corner_model`); one whose path doesn't fit (another layout)
//! finds its own, as before.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The file's format version; bump it when a field changes meaning. 2: GPS
/// speed is moved onto its positions' time (`aim_xrk::gps_lag`), so braking
/// points, and the corners found from them, sit 1.5–2 m earlier than in 1.
const VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackCorners {
    pub version: u32,
    /// The session they were found in (its file name) and its lap.
    pub session: String,
    pub lap: i32,
    /// That lap's path, (lat, lon) every `corner_model::STEP_M` from the line.
    pub line: Vec<(f64, f64)>,
    pub corners: Vec<SavedCorner>,
}

/// Where one corner starts (its braking point; the line for the first) and
/// its apex, as positions and as meters along `line`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedCorner {
    pub from: (f64, f64),
    pub apex: (f64, f64),
    pub from_m: f64,
    pub apex_m: f64,
}

impl TrackCorners {
    pub fn new(
        session: String,
        lap: i32,
        line: Vec<(f64, f64)>,
        corners: Vec<SavedCorner>,
    ) -> Self {
        Self {
            version: VERSION,
            session,
            lap,
            line,
            corners,
        }
    }

    /// Whether its lap started at the start/finish line `line` (lat, lon).
    pub fn starts_at(&self, line: (f64, f64)) -> bool {
        self.line
            .first()
            .is_some_and(|&p| aim_xrk::relap::distance_m(p, line) <= aim_xrk::relap::SAME_LINE_M)
    }

    pub fn length_m(&self) -> f64 {
        self.line.len().saturating_sub(1) as f64 * crate::corner_model::STEP_M
    }
}

fn path(track_id: &str) -> Option<PathBuf> {
    // Track ids are AiM's eight characters; anything else isn't a file name.
    if track_id.is_empty() || !track_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    crate::paths::data("corners").map(|d| d.join(format!("{track_id}.json")))
}

/// The corners saved for a track, if any (and readable by this version).
pub fn load(track_id: &str) -> Option<TrackCorners> {
    let saved: TrackCorners = crate::storage::load(&path(track_id)?)?;
    (saved.version == VERSION && saved.line.len() > 1).then_some(saved)
}

/// The session an older version's corners were found in (its file name), to
/// find them there again: it may be one picked with "Use this session's
/// corners".
pub fn outdated_session(track_id: &str) -> Option<String> {
    let saved: TrackCorners = crate::storage::load(&path(track_id)?)?;
    (saved.version != VERSION).then_some(saved.session)
}

pub fn save(track_id: &str, corners: &TrackCorners) -> std::io::Result<()> {
    let path = path(track_id).ok_or_else(|| std::io::Error::other("not a track id"))?;
    log::info!(
        "corners for track {track_id}: {} from {} lap {}",
        corners.corners.len(),
        corners.session,
        corners.lap
    );
    crate::storage::save(&path, corners)
}

/// The corners to save for a track, from its sessions (`paths`): sessions
/// don't all find the same corners (a lift that's a corner one day, two
/// close corners that merge another), so the count most of them find
/// wins, from the fastest lap that finds it. Opens every session: run it
/// in the background.
pub fn choose(paths: &[PathBuf]) -> Option<TrackCorners> {
    use std::collections::BTreeMap;
    // Per corner count: how many sessions found it, and the fastest lap that did.
    let mut found: BTreeMap<usize, (usize, i64, TrackCorners)> = BTreeMap::new();
    // In a fixed order, so equal laps always pick the same session.
    let mut paths = paths.to_vec();
    paths.sort();
    for path in &paths {
        let Ok(session) = crate::jobs::open_session(path) else {
            continue;
        };
        let gaps = aim_xrk::checks::recording_gaps(&session);
        let Some(best) = session
            .reference_lap(aim_xrk::laps::MIN_LAP_MS, &gaps)
            .copied()
        else {
            continue;
        };
        let counts: Vec<bool> = aim_xrk::laps::racing(&session)
            .into_iter()
            .zip(&session.laps)
            .map(|(racing, l)| racing && !aim_xrk::has_gap(l, &gaps))
            .collect();
        let Some(model) = crate::corner_model::CornerModel::build(&session, &best, &counts, None)
        else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ms = best.duration_ms();
        let e = found
            .entry(model.segments.len())
            .or_insert_with(|| (0, i64::MAX, model.to_saved(name.clone())));
        e.0 += 1;
        if ms < e.1 {
            e.1 = ms;
            e.2 = model.to_saved(name);
        }
    }
    log::info!(
        "corners found per session: {:?}",
        found.iter().map(|(n, e)| (*n, e.0)).collect::<Vec<_>>()
    );
    // Most sessions; between equals, the faster lap.
    found
        .into_values()
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|e| e.2)
}
