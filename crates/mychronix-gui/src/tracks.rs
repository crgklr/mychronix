//! The track library behind the Tracks page: AiM's track database, custom
//! tracks, and the downloaded sessions recorded at each.
//!
//! Sources, first match wins for a given track:
//! - `~/Mychronix/tracks/**/*.tkk` and `*.ria`
//! - AiM's database as downloaded from AiM (`~/Mychronix/aim-tracks`, see `aim_tracks`)
//! - RaceStudio 3's copies, when installed under Wine (`~/.rs3-wine`)
//! - sessions from `~/Mychronix/sessions/<logger>/a_NNNN.xri`

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aim_track::LatLon;
use aim_xrk::SessionSummary;

/// How far a session's start/finish may be from a track's to count as that track.
const SAME_PLACE_M: f64 = 1_000.0;
/// How far a track you named may be from where the session was recorded (a
/// track's point is its center; a big circuit's start/finish can be a
/// kilometer or two from it).
const NAMED_NEAR_M: f64 = 5_000.0;

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

/// Tracks that arrived where you'd named sessions after a track that wasn't
/// there yet, offered for them.
#[derive(Debug, Clone)]
pub struct TrackOffer {
    /// The name as you typed it.
    pub typed: String,
    /// Indices into [`Library::sessions`].
    pub sessions: Vec<usize>,
    /// Indices into [`Library::tracks`], the best guess first (see [`name_match`]).
    pub tracks: Vec<usize>,
}

/// How well `typed` names track `t`: 0 its very name (capitals aside), 1 the
/// same but for spaces and punctuation, 2 one inside the other ("kank" and
/// "Kankakee Sh"); None when they're unrelated.
pub fn name_match(typed: &str, t: &Track) -> Option<u8> {
    let squash = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let names = std::iter::once(t.name.as_str()).chain(t.full_name.as_deref());
    let typed_sq = squash(typed);
    names
        .filter_map(|n| {
            let n_sq = squash(n);
            if n.trim().eq_ignore_ascii_case(typed.trim()) {
                Some(0)
            } else if !typed_sq.is_empty() && n_sq == typed_sq {
                Some(1)
            } else if typed_sq.chars().count() >= 2 && (n_sq.contains(&typed_sq) || typed_sq.contains(&n_sq) && n_sq.chars().count() >= 2) {
                Some(2)
            } else {
                None
            }
        })
        .min()
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

impl Library {
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

    pub fn load() -> Self {
        let started = std::time::Instant::now();
        let mut lib = Self::load_inner();
        lib.assign_karts(&crate::profiles::Profiles::load());
        log::info!("library: {} tracks, {} sessions, {} sources, in {:?}", lib.tracks.len(), lib.sessions.len(), lib.sources.len(), started.elapsed());
        log::debug!("library sources: {:?}", lib.sources);
        lib
    }

    fn load_inner() -> Self {
        let ours = crate::paths::data("tracks").unwrap_or_default();

        let mut lib = Library::default();
        let mut by_guid: HashMap<String, usize> = HashMap::new();

        // Custom tracks first so they win over database entries with the same guid.
        for path in files_with_ext(&ours, "tkk") {
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
            });
        }

        let mut rias = files_with_ext(&ours, "ria");
        rias.extend(crate::aim_tracks::list_path());
        // Before the first download: the list copied from a MyChron (no outlines).
        rias.extend(crate::aim_tracks::mychron_list_path());
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
                    from_mychron: None,
                    country: r.country,
                    region: r.region,
                    center: r.center,
                    start_finish: None,
                    guid: r.guid,
                    kind: if custom_db { Kind::Custom } else { Kind::Aim },
                    outline: Vec::new(),
                    sessions: Vec::new(),
                    sort_key: String::new(),
                    search: String::new(),
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
            let Some(here) = lib.tracks[i].center else { continue };
            let nearest = lib
                .tracks
                .iter()
                .filter(|t| t.kind == Kind::Aim && !t.country.is_empty())
                .filter_map(|t| t.center.map(|p| (p.distance_m(&here), t)))
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
        let sessions_dir = crate::paths::sessions_dir().unwrap_or_default();
        for path in files_with_ext(&sessions_dir, "xri") {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(summary) = aim_xrk::parse_xri(&bytes) else { continue };
            let session = lib.sessions.len();
            let logger = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let xrz = path.with_extension("xrz");
            let mut meta = crate::device::SessionMeta::load(&xrz);
            // A track chosen at download, or changed since, wins over the one the logger recorded.
            let chosen = meta.as_ref().map(|m| m.track.clone()).filter(|t| !t.is_empty() && Some(t) != summary.track.as_ref());
            let track = match (chosen, meta.as_mut()) {
                (Some(name), Some(m)) => {
                    let near = lib.match_by_name(&name, &summary);
                    let place = summary.start_finish.map(|(lat, lon)| LatLon { lat, lon });
                    let around: Vec<usize> = place.map(|p| lib.near(p, NAMED_NEAR_M).into_iter().map(|(i, _)| i).collect()).unwrap_or_default();
                    match m.track_guid.clone() {
                        // Linked to a track: that one, while it's still here.
                        Some(g) if !g.is_empty() => by_guid.get(&g).copied().or(near),
                        // A name with no track to link to: tracks that arrived
                        // here since are offered, not used (see `App::track_offer_dialog`).
                        Some(_) => {
                            let fresh: Vec<usize> = around.into_iter().filter(|&i| !m.track_declined.contains(&lib.tracks[i].guid)).collect();
                            if !fresh.is_empty() {
                                match lib.offers.iter_mut().find(|o| o.typed.eq_ignore_ascii_case(&name)) {
                                    Some(o) => {
                                        o.sessions.push(session);
                                        for i in fresh {
                                            if !o.tracks.contains(&i) {
                                                o.tracks.push(i);
                                            }
                                        }
                                    }
                                    None => lib.offers.push(TrackOffer { typed: name.clone(), sessions: vec![session], tracks: fresh }),
                                }
                            }
                            None
                        }
                        // From before links were kept: a track with the very
                        // name here is linked, as it already was; otherwise
                        // the tracks here now don't count as new, except ones
                        // whose name is close to it (offered).
                        None => {
                            m.track_guid = Some(near.map(|i| lib.tracks[i].guid.clone()).unwrap_or_default());
                            if near.is_none() {
                                m.track_declined = around.iter().filter(|&&i| name_match(&name, &lib.tracks[i]).is_none()).map(|&i| lib.tracks[i].guid.clone()).collect();
                            }
                            if let Err(e) = m.save(&xrz) {
                                log::warn!("couldn't save the track link for {}: {e}", xrz.display());
                            }
                            near
                        }
                    }
                }
                _ => lib.match_track(&summary, &by_guid),
            };
            if let Some(i) = track {
                lib.tracks[i].sessions.push(session);
            }
            let setup = crate::setup::Setup::load(&xrz);
            let weather = crate::weather::Weather::load(&xrz);
            lib.sessions.push(LocalSession { path: xrz, logger, summary, track, meta, setup, weather, kart: None });
        }
        // MyChron 4 sessions: RaceStudio 2's .drk and runs downloaded from a
        // data key (.mc4), summarized from the file itself.
        let rs2 = files_with_ext(&sessions_dir, "drk").into_iter().map(|p| (p, true));
        let mc4 = files_with_ext(&sessions_dir, "mc4").into_iter().map(|p| (p, false));
        // A RaceStudio 2 raw download (.imd) counts only without its .drk.
        let imd = files_with_ext(&sessions_dir, "imd").into_iter().filter(|p| !p.with_extension("drk").exists()).map(|p| (p, false));
        for (path, is_drk) in rs2.chain(mc4).chain(imd) {
            let summary = if is_drk { aim_xrk::drk::summary(&path) } else { aim_xrk::mc4::summary(&path) };
            let Ok(summary) = summary else { continue };
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
            lib.sessions.push(LocalSession { path, logger, summary, track, meta, setup, weather, kart: None });
        }
        for t in &mut lib.tracks {
            // Newest first: by when they started (sessions from several MyChrons
            // mix here, and each numbers its own), then by number.
            t.sessions.sort_by_key(|&i| std::cmp::Reverse((lib.sessions[i].start(), lib.sessions[i].summary.number)));
        }

        // Laps timed from GPS (see `jobs::relap`) stand in for the logger's summary.
        let digests = crate::digest::Digests::load();
        for s in &mut lib.sessions {
            match digests.get(&s.path) {
                Some(d) if d.laps_from_gps => {
                    s.summary.laps = Some(d.lap_count);
                    s.summary.best_lap_number = d.best.map(|b| b.0);
                    s.summary.best_lap_ms = d.best.map(|b| b.1);
                }
                Some(_) => {}
                None if s.summary.laps.unwrap_or(0) <= 1 && s.summary.duration_ms.unwrap_or(0) > 120_000 => lib.relap_pending.push(s.path.clone()),
                None => {}
            }
        }

        // Each offer's best guess first: the closest name, then the nearest.
        for o in &mut lib.offers {
            let place = o.sessions.first().and_then(|&i| lib.sessions[i].place());
            let tracks = &lib.tracks;
            let key = |&i: &usize| (name_match(&o.typed, &tracks[i]).unwrap_or(9), place.zip(tracks[i].center).map(|(a, b)| a.distance_m(&b) as i64).unwrap_or(i64::MAX));
            o.tracks.sort_by_key(key);
        }

        // Visited tracks first, then custom, then by name.
        lib.sort();
        for t in &mut lib.tracks {
            t.index();
        }
        lib
    }

    /// The tracks where a session was recorded (every configuration there),
    /// nearest first, with how far; none when its MyChron didn't place it.
    pub fn around(&self, s: &LocalSession) -> Vec<(usize, f64)> {
        s.place().map(|p| self.near(p, NAMED_NEAR_M)).unwrap_or_default()
    }

    /// Tracks within `within_m` of `place`, nearest first, with how far.
    pub fn near(&self, place: LatLon, within_m: f64) -> Vec<(usize, f64)> {
        let mut v: Vec<(usize, f64)> = self.tracks.iter().enumerate().filter_map(|(i, t)| Some((i, t.center?.distance_m(&place)))).filter(|(_, d)| *d <= within_m).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    /// A track by name, the one nearest where the session was recorded, and
    /// only one within a few kilometers of it (names repeat: an "ABC" in
    /// Wisconsin isn't yours). Without a place to compare, the name alone.
    pub fn match_by_name(&self, name: &str, s: &SessionSummary) -> Option<usize> {
        let place = s.start_finish.map(|(lat, lon)| LatLon { lat, lon });
        let distance = |t: &Track| match (place, t.center) {
            (Some(a), Some(b)) => a.distance_m(&b),
            _ => f64::MAX / 2.0,
        };
        (0..self.tracks.len())
            .filter(|&i| self.tracks[i].name.eq_ignore_ascii_case(name) || self.tracks[i].full_name.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(name)))
            .filter(|&i| distance(&self.tracks[i]) <= NAMED_NEAR_M || distance(&self.tracks[i]) == f64::MAX / 2.0)
            .min_by(|&a, &b| distance(&self.tracks[a]).total_cmp(&distance(&self.tracks[b])))
    }

    fn match_track(&self, s: &SessionSummary, by_guid: &HashMap<String, usize>) -> Option<usize> {
        if let Some(i) = s.track_guid.as_ref().and_then(|g| by_guid.get(g)) {
            return Some(*i);
        }
        let name = s.track.as_deref()?;
        let place = s.start_finish.map(|(lat, lon)| LatLon { lat, lon });
        let near = |t: &Track| match (place, t.center) {
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
        t.start_finish = t.start_finish.or(d.start_finish);
        t.index();
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

/// Tracks marked as favorites, by track guid, saved in `~/Mychronix/favorite-tracks.json`.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Favorites {
    pub guids: std::collections::BTreeSet<String>,
}

impl Favorites {
    fn path() -> Option<PathBuf> {
        crate::paths::data("favorite-tracks.json")
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
/// `aim_xrk::relap`). Reads the track files themselves, not the whole library.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str, full: Option<&str>) -> Track {
        Track {
            name: name.into(),
            full_name: full.map(str::to_string),
            city: None,
            length_m: None,
            track_kind: aim_track::TrackKind::Unknown,
            surface: Vec::new(),
            id: String::new(),
            tkk: None,
            from_mychron: None,
            country: String::new(),
            region: String::new(),
            center: None,
            start_finish: None,
            guid: String::new(),
            kind: Kind::Aim,
            outline: Vec::new(),
            sessions: Vec::new(),
            sort_key: String::new(),
            search: String::new(),
        }
    }

    #[test]
    fn names_match_closest_first() {
        let short = track("Kankakee Sh", Some("Kankakee Short Course"));
        let full = track("Kankakee", None);
        assert_eq!(name_match("kankakee", &full), Some(0));
        assert_eq!(name_match("KANKAKEE SH", &short), Some(0));
        assert_eq!(name_match("kankakee-sh", &short), Some(1));
        assert_eq!(name_match("kank", &short), Some(2));
        assert_eq!(name_match("short", &short), Some(2));
        assert_eq!(name_match("Kankakee", &short), Some(2));
        assert_eq!(name_match("OVRP", &track("Kankakee", None)), None);
        // One letter matches too much to mean anything.
        assert_eq!(name_match("k", &short), None);
    }
}
