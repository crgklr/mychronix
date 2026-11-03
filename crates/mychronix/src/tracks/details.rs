//! What the track list reads from AiM's track files in the background
//! (full names, cities, outlines, start/finish lines), after it first shows
//! the names from the database.

use std::path::PathBuf;

use aim_track::LatLon;

use super::Library;

/// What the list needs from each track file; the rest is read on selection.
pub struct Details {
    pub full_name: String,
    pub city: Option<String>,
    pub length_m: Option<f32>,
    pub kind: aim_track::TrackKind,
    pub surface: Vec<&'static str>,
    pub outline: Vec<LatLon>,
    pub start_finish: Option<LatLon>,
}

/// Reads track files on a background thread, sending details as they're read.
pub fn load_details(
    files: Vec<(usize, PathBuf)>,
    ctx: eframe::egui::Context,
) -> std::sync::mpsc::Receiver<(usize, Details)> {
    let (tx, rx) = std::sync::mpsc::channel();
    crate::jobs::spawn("track-details", move || {
        for (i, path) in files {
            let Ok(t) = aim_track::read_tkk(&path) else {
                continue;
            };
            let d = Details {
                full_name: t.full_name,
                city: t.info.city,
                length_m: t.length_m,
                kind: t.kind,
                surface: t.surface,
                outline: t.outline,
                start_finish: t.start_finish,
            };
            if tx.send((i, d)).is_err() {
                return;
            }
        }
        ctx.request_repaint();
    });
    rx
}

impl Library {
    /// Track files still to read for full names, cities and outlines, by track index.
    pub fn pending_details(&self) -> Vec<(usize, PathBuf)> {
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.full_name.is_none())
            .filter_map(|(i, t)| t.tkk.clone().map(|p| (i, p)))
            .collect()
    }

    /// Keeps the details `old` had read for the same track files, so a reload
    /// (after a download, a Garage change, a new start/finish line) doesn't
    /// send the list back to short names until they're read again. A file
    /// changed since `read_since` (when they started being read) is read again.
    pub fn keep_details(&mut self, old: &Library, read_since: std::time::SystemTime) {
        let by_file: std::collections::HashMap<&std::path::Path, &super::Track> = old
            .tracks
            .iter()
            .filter(|t| t.full_name.is_some())
            .filter_map(|t| Some((t.tkk.as_deref()?, t)))
            .collect();
        let kept: Vec<(usize, Details)> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.full_name.is_none())
            .filter_map(|(i, t)| {
                let path = t.tkk.as_deref()?;
                let o = by_file.get(path)?;
                let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
                (modified < read_since).then(|| {
                    (
                        i,
                        Details {
                            full_name: o.full_name.clone().unwrap_or_default(),
                            city: o.city.clone(),
                            length_m: o.length_m,
                            kind: o.track_kind,
                            surface: o.surface.clone(),
                            outline: o.outline.clone(),
                            start_finish: o.start_finish,
                        },
                    )
                })
            })
            .collect();
        for (i, d) in kept {
            self.apply_details(i, d);
        }
    }

    pub fn apply_details(&mut self, i: usize, d: Details) {
        let Some(t) = self.tracks.get_mut(i) else {
            return;
        };
        t.full_name = Some(d.full_name).filter(|n| !n.is_empty());
        t.city = d.city;
        t.length_m = t.length_m.or(d.length_m);
        t.track_kind = d.kind;
        t.surface = d.surface;
        if t.outline.is_empty() {
            t.outline = d.outline;
        }
        t.start_finish = t.start_finish.or(d.start_finish);
        t.index();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Track};
    use super::*;

    fn track(tkk: &std::path::Path, full_name: Option<&str>) -> Track {
        Track {
            name: "CLUB".into(),
            full_name: full_name.map(Into::into),
            city: full_name.map(|_| "Springfield".into()),
            length_m: None,
            track_kind: aim_track::TrackKind::KartTrack,
            surface: vec![],
            id: "05b5a424".into(),
            tkk: Some(tkk.to_path_buf()),
            from_mychron: None,
            country: "US".into(),
            region: String::new(),
            center: None,
            start_finish: None,
            guid: "g".into(),
            kind: Kind::Aim,
            outline: vec![LatLon { lat: 1.0, lon: 2.0 }],
            sessions: vec![],
            sort_key: String::new(),
            search: String::new(),
        }
    }

    #[test]
    fn a_reload_keeps_the_details_of_files_that_havent_changed() {
        let file =
            std::env::temp_dir().join(format!("mychronix-details-{}.tkk", std::process::id()));
        std::fs::write(&file, b"track").unwrap();
        let read_since = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
        let old = Library {
            tracks: vec![track(&file, Some("Club Ring"))],
            ..Default::default()
        };
        let fresh = || Library {
            tracks: vec![track(&file, None)],
            ..Default::default()
        };

        let mut new = fresh();
        new.keep_details(&old, read_since);
        assert_eq!(new.tracks[0].full_name.as_deref(), Some("Club Ring"));
        assert_eq!(new.tracks[0].city.as_deref(), Some("Springfield"));
        assert!(new.pending_details().is_empty());

        // Changed after it was read: read again.
        let mut new = fresh();
        new.keep_details(&old, std::time::UNIX_EPOCH);
        assert_eq!(new.tracks[0].full_name, None);
        assert_eq!(new.pending_details().len(), 1);
        let _ = std::fs::remove_file(&file);
    }
}
