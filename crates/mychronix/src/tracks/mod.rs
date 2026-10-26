//! The track library behind the Tracks page: AiM's track database, custom
//! tracks, and the downloaded sessions recorded at each.
//!
//! Sources, first match wins for a given track:
//! - `~/Mychronix/tracks/**/*.tkk` and `*.ria` (custom tracks, as copied from a MyChron)
//! - AiM's database as downloaded from AiM (`~/Mychronix/aim-tracks`, see `aim_tracks`)
//! - the list of AiM's tracks copied from a MyChron, before the first download
//! - sessions from `~/Mychronix/sessions/<logger>/`
//!
//! Matching sessions to tracks is in `linking`, start/finish lines for GPS
//! lap timing in `lines`, track files read in the background in `details`.

mod details;
mod favorites;
mod lines;
mod linking;

pub use details::{Details, load_details};
pub use favorites::Favorites;
pub use lines::{Line, stamp, start_finishes_near};
pub use linking::{TrackOffer, name_match};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aim_track::LatLon;
use aim_xrk::SessionSummary;

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
    /// For a custom track, the serial of the MyChron it was copied from, when known.
    pub from_mychron: Option<String>,
    pub country: String,
    pub region: String,
    /// Where the track is, for finding the nearest one (see `aim_track::TrackRecord::center`).
    pub center: Option<LatLon>,
    /// Where laps start and end, from the track's `.tkk`: at load for custom
    /// tracks, with the details for AiM ones.
    pub start_finish: Option<LatLon>,
    pub guid: String,
    pub kind: Kind,
    /// From the track's `.tkk`: at load for custom tracks, in the background for AiM ones.
    pub outline: Vec<LatLon>,
    /// Indices into [`Library::sessions`].
    pub sessions: Vec<usize>,
    /// Lowercase name to sort by, and everything a search matches, worked
    /// out once (see [`Track::index`]) rather than on every frame.
    pub sort_key: String,
    pub search: String,
}

impl Track {
    /// A custom track from its own file at `path`.
    fn from_file(path: &Path, t: aim_track::TrackFile, guid: String) -> Self {
        Track {
            name: t.name,
            full_name: (!t.full_name.is_empty()).then_some(t.full_name),
            city: t.info.city,
            length_m: t.length_m,
            track_kind: t.kind,
            surface: t.surface,
            id: t.id,
            tkk: Some(path.to_path_buf()),
            // Beside it, the MyChron it was copied from (see `device::copy_custom_tracks`).
            from_mychron: std::fs::read_to_string(path.with_extension("mychron")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            country: t.country,
            region: t.region,
            center: t.center.or(t.start_finish),
            start_finish: t.start_finish,
            guid,
            kind: Kind::Custom,
            outline: t.outline,
            sessions: Vec::new(),
            sort_key: String::new(),
            search: String::new(),
        }
    }

    /// A track from a list of them (`.ria`): name and place only, until its
    /// file `tkk` (when there is one) is read (see `details`).
    fn from_record(r: aim_track::TrackRecord, tkk: Option<PathBuf>, kind: Kind) -> Self {
        Track {
            name: r.name,
            full_name: None,
            city: None,
            length_m: r.length_m,
            track_kind: aim_track::TrackKind::Unknown,
            surface: Vec::new(),
            id: r.id,
            tkk,
            from_mychron: None,
            country: r.country,
            region: r.region,
            center: r.center,
            start_finish: None,
            guid: r.guid,
            kind,
            outline: Vec::new(),
            sessions: Vec::new(),
            sort_key: String::new(),
            search: String::new(),
        }
    }

    /// Works out `sort_key` and `search` from the names; again whenever they change.
    pub fn index(&mut self) {
        self.sort_key = self.full_name.as_deref().unwrap_or(&self.name).to_lowercase();
        let country = crate::countries::label(&self.country);
        let parts = [Some(self.name.as_str()), Some(self.region.as_str()), self.city.as_deref(), self.full_name.as_deref(), Some(country.as_str())];
        // One per line, so a search can't match across two of them.
        self.search = parts.into_iter().flatten().collect::<Vec<_>>().join("\n").to_lowercase();
    }

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
            parts.push(aim_track::length_label(l, self.track_kind, crate::units::imperial()));
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
    /// The kart its MyChron was on when it was recorded (see
    /// `Profiles::kart_for`); None for a MyChron that wasn't on one then.
    pub kart: Option<u32>,
}

impl LocalSession {
    /// Where it was recorded, as its MyChron placed it (the start/finish of
    /// the track it recorded on); None when it recorded no track.
    pub fn place(&self) -> Option<LatLon> {
        self.summary.start_finish.map(|(lat, lon)| LatLon { lat, lon })
    }

    /// Its track's name: the track it's assigned to, else the name you typed
    /// for it, else the one its MyChron recorded.
    pub fn track_name(&self, tracks: &[Track]) -> Option<String> {
        self.track
            .map(|t| tracks[t].name.clone())
            .or_else(|| self.meta.as_ref().map(|m| m.track.clone()).filter(|t| !t.is_empty()))
            .or_else(|| self.summary.track.clone())
    }

    /// When it started, as `Profiles::kart_for` compares it.
    pub fn start(&self) -> String {
        crate::profiles::stamp(self.summary.date, self.summary.start_time.as_deref())
    }
}

#[derive(Debug, Default)]
pub struct Library {
    pub tracks: Vec<Track>,
    pub sessions: Vec<LocalSession>,
    /// Where each source came from, for the page's footnote.
    pub sources: Vec<String>,
    /// Sessions the logger may not have timed (one lap or none, but minutes
    /// long) that haven't been looked at yet: their laps may come from GPS.
    pub relap_pending: Vec<PathBuf>,
    /// Tracks that arrived after you typed their name for sessions at the
    /// same place: offered for those sessions (see `App::track_offer_dialog`).
    pub offers: Vec<TrackOffer>,
}

impl Library {
    pub fn load() -> Self {
        let started = std::time::Instant::now();
        let ours = crate::paths::data("tracks").unwrap_or_default();
        let sessions = crate::paths::sessions_dir().unwrap_or_default();
        let mut lib = Library::default();
        // Each track's index by guid, to match sessions and to skip a track
        // already read from another source.
        let mut by_guid: HashMap<String, usize> = HashMap::new();
        lib.add_custom_tracks(&ours, &mut by_guid);
        lib.add_track_lists(&ours, &mut by_guid);
        lib.fill_in_custom_tracks();
        lib.add_sessions(&sessions, &by_guid);
        lib.add_mychron4_sessions(&sessions);
        lib.apply_digests();
        lib.rank_offers();
        lib.sort();
        for t in &mut lib.tracks {
            t.index();
        }
        lib.assign_karts(&crate::profiles::Profiles::load());
        log::info!("library: {} tracks, {} sessions, {} sources, in {:?}", lib.tracks.len(), lib.sessions.len(), lib.sources.len(), started.elapsed());
        log::debug!("library sources: {:?}", lib.sources);
        lib
    }

    /// Custom tracks first, so they win over list entries with the same guid.
    fn add_custom_tracks(&mut self, ours: &Path, by_guid: &mut HashMap<String, usize>) {
        for path in files_with_ext(ours, "tkk") {
            let Ok(t) = aim_track::read_tkk(&path) else { continue };
            let guid = t.guid.clone().unwrap_or_else(|| format!("tkk:{}", t.id));
            if by_guid.contains_key(&guid) {
                continue;
            }
            by_guid.insert(guid.clone(), self.tracks.len());
            self.sources.push(path.display().to_string());
            self.tracks.push(Track::from_file(&path, t, guid));
        }
    }

    /// Lists of tracks (`.ria`): your custom ones' (`dev.ria`), AiM's
    /// database, and before the first download, a MyChron's copy of it.
    fn add_track_lists(&mut self, ours: &Path, by_guid: &mut HashMap<String, usize>) {
        let mut rias = files_with_ext(ours, "ria");
        rias.extend(crate::aim_tracks::list_path());
        rias.extend(crate::aim_tracks::mychron_list_path());
        for path in rias {
            let Ok(records) = aim_track::read_ria(&path) else { continue };
            let kind = if path.file_name().is_some_and(|n| n == "dev.ria") { Kind::Custom } else { Kind::Aim };
            let tkk_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let before = self.tracks.len();
            for r in records {
                if let Some(&i) = by_guid.get(&r.guid) {
                    // Fill in what the .tkk lacks (country/region).
                    let t = &mut self.tracks[i];
                    if t.country.is_empty() {
                        t.country = r.country;
                        t.region = r.region;
                    }
                    t.length_m = t.length_m.or(r.length_m);
                    continue;
                }
                by_guid.insert(r.guid.clone(), self.tracks.len());
                let tkk = Some(tkk_dir.join(format!("{}.tkk", r.id))).filter(|p| p.exists());
                self.tracks.push(Track::from_record(r, tkk, kind));
            }
            if self.tracks.len() > before {
                self.sources.push(path.display().to_string());
            }
        }
    }

    /// Custom tracks don't record a country, and often not a length: the
    /// country (and city) of the nearest AiM track, and the outline measured.
    fn fill_in_custom_tracks(&mut self) {
        for i in 0..self.tracks.len() {
            if !self.tracks[i].country.is_empty() && (self.tracks[i].kind != Kind::Custom || self.tracks[i].city.is_some()) {
                continue;
            }
            let Some(here) = self.tracks[i].center else { continue };
            let nearest = self
                .tracks
                .iter()
                .filter(|t| t.kind == Kind::Aim && !t.country.is_empty())
                .filter_map(|t| t.center.map(|p| (p.distance_m(&here), t)))
                .filter(|(d, _)| *d < 20_000.0)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, t)| (t.country.clone(), t.region.clone(), t.tkk.clone()));
            if let Some((country, region, tkk)) = nearest {
                self.tracks[i].country = country;
                self.tracks[i].region = region;
                if self.tracks[i].city.is_none() {
                    self.tracks[i].city = tkk.and_then(|p| aim_track::read_tkk(p).ok()).and_then(|f| f.info.city);
                }
            }
        }
        for t in &mut self.tracks {
            if t.length_m.is_none() && t.outline.len() > 2 {
                let mut len: f64 = t.outline.windows(2).map(|w| w[0].distance_m(&w[1])).sum();
                len += t.outline[t.outline.len() - 1].distance_m(&t.outline[0]);
                t.length_m = Some(len as f32);
            }
        }
    }

    /// Sessions downloaded from a MyChron 5 or 6 (their `.xri` summaries),
    /// each on the track you named for it (see `link_named`), else the one
    /// its MyChron recorded.
    fn add_sessions(&mut self, dir: &Path, by_guid: &HashMap<String, usize>) {
        for path in files_with_ext(dir, "xri") {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(summary) = aim_xrk::parse_xri(&bytes) else { continue };
            let session = self.sessions.len();
            let xrz = path.with_extension("xrz");
            let mut meta = crate::device::SessionMeta::load(&xrz);
            let named = meta.as_ref().map(|m| m.track.clone()).filter(|t| !t.is_empty() && Some(t) != summary.track.as_ref());
            let track = match (named, meta.as_mut()) {
                (Some(name), Some(m)) => self.link_named(session, &name, &summary, m, &xrz, by_guid),
                _ => self.match_track(&summary, by_guid),
            };
            self.push_session(xrz, logger_of(&path), summary, track, meta);
        }
    }

    /// MyChron 4 sessions: RaceStudio 2's `.drk` and runs downloaded from a
    /// data key (`.mc4`, or `.imd` without its `.drk`), summarized from the
    /// files themselves, on the track by name.
    fn add_mychron4_sessions(&mut self, dir: &Path) {
        let rs2 = files_with_ext(dir, "drk").into_iter().map(|p| (p, true));
        let mc4 = files_with_ext(dir, "mc4").into_iter().map(|p| (p, false));
        let imd = files_with_ext(dir, "imd").into_iter().filter(|p| !p.with_extension("drk").exists()).map(|p| (p, false));
        for (path, is_drk) in rs2.chain(mc4).chain(imd) {
            let summary = if is_drk { aim_xrk::drk::summary(&path) } else { aim_xrk::mc4::summary(&path) };
            let Ok(summary) = summary else { continue };
            let meta = crate::device::SessionMeta::load(&path);
            let named = meta.as_ref().map(|m| m.track.clone()).filter(|t| !t.is_empty());
            let track = named.or_else(|| summary.track.clone()).and_then(|name| self.match_by_name(&name, &summary));
            self.push_session(path.clone(), logger_of(&path), summary, track, meta);
        }
    }

    /// Adds a session (with its setup and weather beside it) to the library and its track.
    fn push_session(&mut self, path: PathBuf, logger: String, summary: SessionSummary, track: Option<usize>, meta: Option<crate::device::SessionMeta>) {
        if let Some(i) = track {
            self.tracks[i].sessions.push(self.sessions.len());
        }
        let setup = crate::setup::Setup::load(&path);
        let weather = crate::weather::Weather::load(&path);
        self.sessions.push(LocalSession { path, logger, summary, track, meta, setup, weather, kart: None });
    }

    /// Laps timed from GPS (see `jobs::relap`) stand in for the logger's
    /// summary; sessions that may need them (untimed, or at a track you
    /// picked) and haven't been looked at yet are noted (see `relap_pending`).
    fn apply_digests(&mut self) {
        let digests = crate::digest::Digests::load();
        for s in &mut self.sessions {
            match digests.get(&s.path) {
                Some(d) if d.laps_from_gps => {
                    s.summary.laps = Some(d.lap_count);
                    s.summary.best_lap_number = d.best.map(|b| b.0);
                    s.summary.best_lap_ms = d.best.map(|b| b.1);
                }
                Some(_) => {}
                None if s.summary.laps.unwrap_or(0) <= 1 && s.summary.duration_ms.unwrap_or(0) > 120_000 => self.relap_pending.push(s.path.clone()),
                // A track picked over the MyChron's: its laps may start at another line.
                None if s.meta.as_ref().and_then(|m| m.track_guid.as_deref()).is_some_and(|g| !g.is_empty() && Some(g) != s.summary.track_guid.as_deref()) => self.relap_pending.push(s.path.clone()),
                None => {}
            }
        }
    }

    /// Visited tracks first, then custom, then by name; each track's sessions
    /// newest first.
    fn sort(&mut self) {
        for t in &mut self.tracks {
            // By when they started (sessions from several MyChrons mix here,
            // and each numbers its own), then by number.
            t.sessions.sort_by_key(|&i| std::cmp::Reverse((self.sessions[i].start(), self.sessions[i].summary.number)));
        }
        // Keep session indices valid: sessions aren't reordered, only tracks.
        self.tracks.sort_by(|a, b| {
            (!b.sessions.is_empty())
                .cmp(&!a.sessions.is_empty())
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

    /// Gives each session the kart its MyChron was on when it was recorded.
    pub fn assign_karts(&mut self, profiles: &crate::profiles::Profiles) {
        for s in &mut self.sessions {
            s.kart = profiles.kart_for(&s.logger, &s.start()).map(|k| k.id);
        }
    }

    /// The start of a MyChron's newest session here, or "" (see `Profiles::mount`).
    pub fn newest_start(&self, logger: &str) -> String {
        self.sessions.iter().filter(|s| s.logger == logger).map(|s| s.start()).max().unwrap_or_default()
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.tracks.iter().filter(|t| t.kind == kind).count()
    }
}

/// The logger a session file came from: its folder's name.
fn logger_of(path: &Path) -> String {
    path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Every file with extension `ext` under `dir`, in order.
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
