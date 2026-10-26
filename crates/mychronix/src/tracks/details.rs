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
pub fn load_details(files: Vec<(usize, PathBuf)>, ctx: eframe::egui::Context) -> std::sync::mpsc::Receiver<(usize, Details)> {
    let (tx, rx) = std::sync::mpsc::channel();
    crate::jobs::spawn("track-details", move || {
        for (i, path) in files {
            let Ok(t) = aim_track::read_tkk(&path) else { continue };
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

    pub fn apply_details(&mut self, i: usize, d: Details) {
        let Some(t) = self.tracks.get_mut(i) else { return };
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
