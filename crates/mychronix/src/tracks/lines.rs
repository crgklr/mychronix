//! Start/finish lines for GPS lap timing (see `jobs::relap`), read from the
//! track files near a session rather than from the whole library.

use std::path::{Path, PathBuf};

use aim_track::LatLon;

use super::files_with_ext;

/// What GPS lap timing had to go on: your custom tracks and AiM's lists, by
/// name and size. Changes when a track is added or a list is updated.
pub fn stamp() -> String {
    let mut parts = Vec::new();
    let ours = crate::paths::data("tracks").unwrap_or_default();
    let mut files: Vec<PathBuf> = files_with_ext(&ours, "tkk");
    files.extend(files_with_ext(&ours, "ria"));
    files.extend(crate::aim_tracks::list_path());
    files.extend(crate::aim_tracks::mychron_list_path());
    for f in files {
        let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        parts.push(format!("{}:{size}", f.file_name().unwrap_or_default().to_string_lossy()));
    }
    parts.join(",")
}

/// A track's start/finish line, as GPS lap timing uses it.
#[derive(Debug, Clone)]
pub struct Line {
    pub at: (f64, f64),
    /// The track's guid, to prefer a session's own track's line.
    pub guid: String,
}

/// Start/finish lines within `within_m` of a spot, nearest first: your own
/// tracks' (custom ones, as on your MyChron), then AiM's database's. The
/// lines a session recorded there may have been meant to cross (see
/// `aim_xrk::relap`). A few milliseconds: no need to keep them between calls.
pub fn start_finishes_near(lat: f64, lon: f64, within_m: f64) -> (Vec<Line>, Vec<Line>) {
    let ours = crate::paths::data("tracks").unwrap_or_default();
    let here = LatLon { lat, lon };
    let line = |t: aim_track::TrackFile, guid: Option<String>| t.start_finish.map(|p| (p, guid.or(t.guid).unwrap_or_default()));
    let mut own: Vec<(LatLon, String)> = files_with_ext(&ours, "tkk").iter().filter_map(|p| line(aim_track::read_tkk(p).ok()?, None)).collect();
    let mut aim: Vec<(LatLon, String)> = Vec::new();
    let mut rias = files_with_ext(&ours, "ria");
    rias.extend(crate::aim_tracks::list_path());
    rias.extend(crate::aim_tracks::mychron_list_path());
    for path in rias {
        let custom = path.file_name().is_some_and(|n| n == "dev.ria");
        let Ok(records) = aim_track::read_ria(&path) else { continue };
        // A list records each track's center; its start/finish is in its own file.
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let points = records
            .into_iter()
            .filter(|r| r.center.is_some_and(|c| c.distance_m(&here) <= within_m))
            .filter_map(|r| line(aim_track::read_tkk(dir.join(format!("{}.tkk", r.id))).ok()?, Some(r.guid)));
        if custom { own.extend(points) } else { aim.extend(points) }
    }
    let near = |points: Vec<(LatLon, String)>| {
        let mut v: Vec<(f64, LatLon, String)> = points.into_iter().map(|(p, g)| (p.distance_m(&here), p, g)).filter(|(d, _, _)| *d <= within_m).collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        // The same line from several sources, once.
        v.dedup_by(|a, b| a.1.distance_m(&b.1) < 1.0 && a.2 == b.2);
        v.into_iter().map(|(_, p, guid)| Line { at: (p.lat, p.lon), guid }).collect::<Vec<_>>()
    };
    (near(own), near(aim))
}
