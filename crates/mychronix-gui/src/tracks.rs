//! The track library behind the Tracks page: AiM's track database, custom
//! tracks, and the downloaded sessions recorded at each.
//!
//! Sources, first match wins for a given track:
//! - `~/Mychronix/tracks/**/*.tkk` and `*.ria`
//! - RaceStudio 3's copies, when installed under Wine (`~/.rs3-wine`)
//! - sessions from `~/Mychronix/sessions/<logger>/a_NNNN.xri`

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aim_track::LatLon;
use aim_xrk::SessionSummary;

/// How far a session's start/finish may be from a track's to count as that track.
const SAME_PLACE_M: f64 = 1_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Custom,
    Aim,
}

#[derive(Debug, Clone)]
pub struct Track {
    /// Short name as the logger shows it.
    pub name: String,
    /// Full venue name from the track file, once loaded.
    pub full_name: Option<String>,
    pub city: Option<String>,
    pub length_m: Option<f32>,
    pub track_kind: aim_track::TrackKind,
    pub surface: Vec<&'static str>,
    /// Eight-character AiM id (also the `.tkk` file name).
    pub id: String,
    /// The track's own file, where there is one.
    pub tkk: Option<PathBuf>,
    pub country: String,
    pub region: String,
    pub start_finish: Option<LatLon>,
    pub guid: String,
    pub kind: Kind,
    /// From the track's `.tkk`: at load for custom tracks, in the background for AiM ones.
    pub outline: Vec<LatLon>,
    /// Indices into [`Library::sessions`].
    pub sessions: Vec<usize>,
}

impl Track {
    /// "City, Region", as much as is known.
    pub fn locality(&self) -> String {
        let city = self.city.clone().unwrap_or_default();
        // Some AiM cities already end with the state ("Pine Hall, NC").
        let has_region = !self.region.is_empty() && city.split(',').any(|part| part.trim().eq_ignore_ascii_case(&self.region));
        let region = if has_region { String::new() } else { self.region.clone() };
        [city, region].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
    }

    /// "637 m Kart Track (Paved)", as RaceStudio describes a track.
    pub fn description(&self) -> String {
        let mut parts = Vec::new();
        if let Some(l) = self.length_m {
            parts.push(aim_track::length_label(l, self.track_kind));
        }
        if self.track_kind != aim_track::TrackKind::Unknown {
            parts.push(self.track_kind.label());
        }
        let mut out = parts.join(" ");
        if !self.surface.is_empty() {
            out.push_str(&format!(" ({})", self.surface.join(", ")));
        }
        out
    }

    pub fn best_lap_ms(&self, lib: &Library) -> Option<u64> {
        self.sessions.iter().filter_map(|&i| lib.sessions[i].summary.best_lap_ms).min()
    }
}

#[derive(Debug, Clone)]
pub struct LocalSession {
    pub path: PathBuf,
    /// The logger it came from (its folder under `~/Mychronix/sessions`).
    pub logger: String,
    pub summary: SessionSummary,
    /// Index into [`Library::tracks`], when the session matched a track.
    pub track: Option<usize>,
    /// Driver, kart, championship and comment entered when it was downloaded.
    pub meta: Option<crate::device::SessionMeta>,
    /// Its own setup sheet, when one was written for it.
    pub setup: Option<crate::setup::Setup>,
    /// The weather when it started, once fetched (see `crate::weather`).
    pub weather: Option<crate::weather::Weather>,
}

#[derive(Debug, Default)]
pub struct Library {
    pub tracks: Vec<Track>,
    pub sessions: Vec<LocalSession>,
    /// Where each source came from, for the page's footnote.
    pub sources: Vec<String>,
}

impl Library {
    pub fn load() -> Self {
        let started = std::time::Instant::now();
        let lib = Self::load_inner();
        log::info!("library: {} tracks, {} sessions, {} sources, in {:?}", lib.tracks.len(), lib.sessions.len(), lib.sources.len(), started.elapsed());
        log::debug!("library sources: {:?}", lib.sources);
        lib
    }

    fn load_inner() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let ours = home.join("Mychronix").join("tracks");
        let rs3 = home.join(".rs3-wine/drive_c/AIM_SPORT/RaceStudio3");

        let mut lib = Library::default();
        let mut by_guid: HashMap<String, usize> = HashMap::new();

        // Custom tracks first so they win over database entries with the same guid.
        let mut tkks = files_with_ext(&ours, "tkk");
        tkks.extend(files_with_ext(&rs3.join("user/tracks"), "tkk"));
        for path in tkks {
            let Ok(t) = aim_track::read_tkk(&path) else { continue };
            let guid = t.guid.clone().unwrap_or_else(|| format!("tkk:{}", t.id));
            if by_guid.contains_key(&guid) {
                continue;
            }
            by_guid.insert(guid.clone(), lib.tracks.len());
            lib.sources.push(path.display().to_string());
            lib.tracks.push(Track {
                name: t.name,
                full_name: (!t.full_name.is_empty()).then_some(t.full_name),
                city: t.info.city,
                length_m: t.length_m,
                track_kind: t.kind,
                surface: t.surface,
                id: t.id,
                tkk: Some(path.clone()),
                country: t.country,
                region: t.region,
                start_finish: t.start_finish,
                guid,
                kind: Kind::Custom,
                outline: t.outline,
                sessions: Vec::new(),
            });
        }

        let mut rias = files_with_ext(&ours, "ria");
        let rs3_db = rs3.join("aim_tracks/aim_tracks.ria");
        if rs3_db.exists() {
            rias.push(rs3_db);
        }
        for path in rias {
            let Ok(records) = aim_track::read_ria(&path) else { continue };
            let custom_db = path.file_name().is_some_and(|n| n == "dev.ria");
            let tkk_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let before = lib.tracks.len();
            for r in records {
                if let Some(&i) = by_guid.get(&r.guid) {
                    // Fill in what the .tkk lacks (country/region).
                    let t = &mut lib.tracks[i];
                    if t.country.is_empty() {
                        t.country = r.country;
                        t.region = r.region;
                    }
                    t.length_m = t.length_m.or(r.length_m);
                    continue;
                }
                by_guid.insert(r.guid.clone(), lib.tracks.len());
                let tkk = Some(tkk_dir.join(format!("{}.tkk", r.id))).filter(|p| p.exists());
                lib.tracks.push(Track {
                    name: r.name,
                    full_name: None,
                    city: None,
                    length_m: r.length_m,
                    track_kind: aim_track::TrackKind::Unknown,
                    surface: Vec::new(),
                    id: r.id,
                    tkk,
                    country: r.country,
                    region: r.region,
                    start_finish: r.start_finish,
                    guid: r.guid,
                    kind: if custom_db { Kind::Custom } else { Kind::Aim },
                    outline: Vec::new(),
                    sessions: Vec::new(),
                });
            }
            if lib.tracks.len() > before {
                lib.sources.push(path.display().to_string());
            }
        }

        // Custom tracks don't record a country; borrow it from the nearest AiM track.
        for i in 0..lib.tracks.len() {
            if !lib.tracks[i].country.is_empty() && (lib.tracks[i].kind != Kind::Custom || lib.tracks[i].city.is_some()) {
                continue;
            }
            let Some(here) = lib.tracks[i].start_finish else { continue };
            let nearest = lib
                .tracks
                .iter()
                .filter(|t| t.kind == Kind::Aim && !t.country.is_empty())
                .filter_map(|t| t.start_finish.map(|p| (p.distance_m(&here), t)))
                .filter(|(d, _)| *d < 20_000.0)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, t)| (t.country.clone(), t.region.clone(), t.tkk.clone()));
            if let Some((country, region, tkk)) = nearest {
                lib.tracks[i].country = country;
                lib.tracks[i].region = region;
                if lib.tracks[i].city.is_none() {
                    lib.tracks[i].city = tkk.and_then(|p| aim_track::read_tkk(p).ok()).and_then(|f| f.info.city);
                }
            }
        }
        // Custom tracks often lack a stored length; measure the outline instead.
        for t in &mut lib.tracks {
            if t.length_m.is_none() && t.outline.len() > 2 {
                let mut len: f64 = t.outline.windows(2).map(|w| w[0].distance_m(&w[1])).sum();
                len += t.outline[t.outline.len() - 1].distance_m(&t.outline[0]);
                t.length_m = Some(len as f32);
            }
        }

        // Downloaded sessions, matched to tracks by guid, then name and place.
        for path in files_with_ext(&home.join("Mychronix").join("sessions"), "xri") {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(summary) = aim_xrk::parse_xri(&bytes) else { continue };
            let session = lib.sessions.len();
            let logger = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let xrz = path.with_extension("xrz");
            let meta = crate::device::SessionMeta::load(&xrz);
            // A track chosen at download wins over the one the logger recorded.
            let chosen = meta.as_ref().map(|m| m.track.clone()).filter(|t| !t.is_empty() && Some(t) != summary.track.as_ref());
            let track = match chosen {
                Some(name) => lib.match_by_name(&name, &summary),
                None => lib.match_track(&summary, &by_guid),
            };
            if let Some(i) = track {
                lib.tracks[i].sessions.push(session);
            }
            let setup = crate::setup::Setup::load(&xrz);
            let weather = crate::weather::Weather::load(&xrz);
            lib.sessions.push(LocalSession { path: xrz, logger, summary, track, meta, setup, weather });
        }
        // RaceStudio 2 sessions (MyChron 4), summarized from the file itself.
        for path in files_with_ext(&home.join("Mychronix").join("sessions"), "drk") {
            let Ok(summary) = aim_xrk::drk::summary(&path) else { continue };
            let session = lib.sessions.len();
            let logger = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let meta = crate::device::SessionMeta::load(&path);
            let chosen = meta.as_ref().map(|m| m.track.clone()).filter(|t| !t.is_empty());
            let track = match chosen.or_else(|| summary.track.clone()) {
                Some(name) => lib.match_by_name(&name, &summary),
                None => None,
            };
            if let Some(i) = track {
                lib.tracks[i].sessions.push(session);
            }
            let setup = crate::setup::Setup::load(&path);
            let weather = crate::weather::Weather::load(&path);
            lib.sessions.push(LocalSession { path, logger, summary, track, meta, setup, weather });
        }
        for t in &mut lib.tracks {
            t.sessions.sort_by_key(|&i| std::cmp::Reverse(lib.sessions[i].summary.number));
        }

        // Visited tracks first, then custom, then by name.
        lib.sort();
        lib
    }

    /// A track by name, preferring the one nearest where the session was recorded.
    fn match_by_name(&self, name: &str, s: &SessionSummary) -> Option<usize> {
        let place = s.start_finish.map(|(lat, lon)| LatLon { lat, lon });
        let distance = |t: &Track| match (place, t.start_finish) {
            (Some(a), Some(b)) => a.distance_m(&b),
            _ => f64::MAX / 2.0,
        };
        (0..self.tracks.len())
            .filter(|&i| self.tracks[i].name.eq_ignore_ascii_case(name) || self.tracks[i].full_name.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(name)))
            .min_by(|&a, &b| distance(&self.tracks[a]).total_cmp(&distance(&self.tracks[b])))
    }

    fn match_track(&self, s: &SessionSummary, by_guid: &HashMap<String, usize>) -> Option<usize> {
        if let Some(i) = s.track_guid.as_ref().and_then(|g| by_guid.get(g)) {
            return Some(*i);
        }
        let name = s.track.as_deref()?;
        let place = s.start_finish.map(|(lat, lon)| LatLon { lat, lon });
        let near = |t: &Track| match (place, t.start_finish) {
            (Some(a), Some(b)) => a.distance_m(&b) < SAME_PLACE_M,
            _ => true,
        };
        self.tracks.iter().position(|t| t.name == name && near(t))
    }

    fn sort(&mut self) {
        // Keep session indices valid: sessions aren't reordered, only tracks.
        self.tracks.sort_by(|a, b| {
            (b.sessions.len() > 0)
                .cmp(&(a.sessions.len() > 0))
                .then((a.kind != Kind::Custom).cmp(&(b.kind != Kind::Custom)))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        // Re-point each session at its track's new position.
        for (ti, t) in self.tracks.iter().enumerate() {
            for &si in &t.sessions {
                self.sessions[si].track = Some(ti);
            }
        }
    }

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
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.tracks.iter().filter(|t| t.kind == kind).count()
    }
}

fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_with_ext(&path, ext));
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// What the list needs from each track file; the rest is read on selection.
pub struct Details {
    pub full_name: String,
    pub city: Option<String>,
    pub length_m: Option<f32>,
    pub kind: aim_track::TrackKind,
    pub surface: Vec<&'static str>,
    pub outline: Vec<LatLon>,
}

/// Reads track files on a background thread, sending details as they're read.
pub fn load_details(files: Vec<(usize, PathBuf)>, ctx: eframe::egui::Context) -> std::sync::mpsc::Receiver<(usize, Details)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for (i, path) in files {
            let Ok(t) = aim_track::read_tkk(&path) else { continue };
            let d = Details {
                full_name: t.full_name,
                city: t.info.city,
                length_m: t.length_m,
                kind: t.kind,
                surface: t.surface,
                outline: t.outline,
            };
            if tx.send((i, d)).is_err() {
                return;
            }
        }
        ctx.request_repaint();
    });
    rx
}

/// Tracks marked as favorites, by track guid, saved in `~/Mychronix/favorite-tracks.json`.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Favorites {
    pub guids: std::collections::BTreeSet<String>,
}

impl Favorites {
    fn path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("favorite-tracks.json"))
    }

    pub fn load() -> Self {
        crate::storage::load_or_default(Self::path())
    }

    pub fn contains(&self, guid: &str) -> bool {
        self.guids.contains(guid)
    }

    /// Adds or removes `guid`, then saves.
    pub fn toggle(&mut self, guid: &str) -> std::io::Result<()> {
        if !self.guids.remove(guid) {
            self.guids.insert(guid.to_string());
        }
        let Some(path) = Self::path() else { return Ok(()) };
        crate::storage::save(&path, self)
    }
}
