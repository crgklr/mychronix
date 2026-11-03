//! Events: downloaded sessions grouped the way RaceStudio does (one event per
//! week, track and championship), plus events you name or plan ahead, saved
//! in `~/Mychronix/events.json`.

use std::path::PathBuf;

use aim_xrk::summary::{Date, SessionSummary};
use serde::{Deserialize, Serialize};

use crate::dates::{parse_date, week_start};
use crate::tracks::Library;

/// Sessions slower than this flat out never left the stand: engine warm-ups.
pub const WARM_UP_MAX_KMH: f64 = 10.0;

/// An event you named or planned.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EventDef {
    pub id: u64,
    pub name: String,
    /// Track name as the sessions carry it (e.g. "OVRP Full").
    pub track: String,
    /// "2026-10-17".
    pub start: String,
    pub end: String,
    /// Empty: any championship.
    #[serde(default)]
    pub championship: String,
    #[serde(default)]
    pub notes: String,
}

impl EventDef {
    pub fn dates(&self) -> Option<(Date, Date)> {
        let (a, b) = (parse_date(&self.start)?, parse_date(&self.end)?);
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    fn claims(&self, date: Date, track: &str, championship: &str) -> bool {
        self.dates().is_some_and(|(a, b)| a <= date && date <= b)
            && self.track.trim().eq_ignore_ascii_case(track.trim())
            && (self.championship.trim().is_empty() || self.championship.trim().eq_ignore_ascii_case(championship.trim()))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Events {
    pub events: Vec<EventDef>,
}

impl Events {
    fn path() -> Option<PathBuf> {
        crate::paths::data("events.json")
    }

    pub fn load() -> Self {
        crate::storage::load_or_default(Self::path())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| std::io::Error::other("no home folder"))?;
        crate::storage::save(&path, self)
    }


    /// Adds or replaces `def` (by id; id 0 gets a new one). Returns its id.
    pub fn upsert(&mut self, mut def: EventDef) -> u64 {
        if def.id == 0 {
            def.id = self.events.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        }
        let id = def.id;
        match self.events.iter_mut().find(|e| e.id == id) {
            Some(e) => *e = def,
            None => self.events.push(def),
        }
        id
    }
}

/// One event: a named/planned one, or an automatic week + track + championship group.
#[derive(Debug, Clone)]
pub struct Event {
    /// The saved definition, for named and planned events.
    pub def: Option<EventDef>,
    pub track: String,
    pub championship: String,
    pub start: Date,
    pub end: Date,
    /// Indices into [`Library::sessions`], oldest first.
    pub sessions: Vec<usize>,
}

impl Event {
    pub fn title(&self) -> String {
        match &self.def {
            Some(d) if !d.name.trim().is_empty() => d.name.clone(),
            _ => self.track.clone(),
        }
    }

    /// Stable key for keeping the selection across regrouping.
    pub fn key(&self) -> String {
        match &self.def {
            Some(d) => format!("def:{}", d.id),
            None => format!("auto:{}:{}:{}", self.start, self.track.to_lowercase(), self.championship.to_lowercase()),
        }
    }
}

/// What a session counts as for grouping: its date, track name and championship.
pub struct SessionKey {
    pub date: Date,
    pub track: String,
    pub championship: String,
}

/// The track and championship a session belongs to: the details entered at
/// download first, then what the logger recorded, then the logger's saved
/// championship.
pub fn session_key(lib: &Library, i: usize, profiles: &crate::profiles::Profiles) -> Option<SessionKey> {
    let s = &lib.sessions[i];
    let date = s.summary.date?;
    let meta = s.meta.clone().unwrap_or_default();
    // An engine warm-up in the pits records no track: it belongs with what
    // the same MyChron recorded at a track that day.
    let track = track_of(lib, i)
        .or_else(|| {
            lib.sessions.iter().enumerate().filter(|(_, o)| o.summary.date == Some(date) && o.logger == s.logger).find_map(|(j, _)| track_of(lib, j))
        })
        .unwrap_or_else(|| "No track".into());
    let championship = Some(meta.championship)
        .filter(|c| !c.trim().is_empty())
        .or_else(|| profiles.get(&s.logger).map(|p| p.championship.clone()))
        .unwrap_or_default();
    Some(SessionKey { date, track, championship })
}

/// A session's own track name: the one entered at download, else the logger's, else the library's.
fn track_of(lib: &Library, i: usize) -> Option<String> {
    let s = &lib.sessions[i];
    s.meta
        .as_ref()
        .map(|m| m.track.clone())
        .filter(|t| !t.trim().is_empty())
        .or_else(|| s.summary.track.clone())
        .or_else(|| s.track.map(|t| lib.tracks[t].name.clone()))
}

/// Groups every session into events. Planned events with no sessions yet are
/// included; the result is newest first.
pub fn group(lib: &Library, profiles: &crate::profiles::Profiles, defs: &Events) -> Vec<Event> {
    let mut events: Vec<Event> = defs
        .events
        .iter()
        .filter_map(|d| {
            let (start, end) = d.dates()?;
            Some(Event { def: Some(d.clone()), track: d.track.clone(), championship: d.championship.clone(), start, end, sessions: vec![] })
        })
        .collect();
    let named = events.len();
    let mut order: Vec<usize> = (0..lib.sessions.len()).collect();
    order.sort_by_key(|&i| (lib.sessions[i].summary.date, lib.sessions[i].summary.start_time.clone(), lib.sessions[i].summary.number));
    for i in order {
        let Some(k) = session_key(lib, i, profiles) else { continue };
        if let Some(e) = events[..named].iter_mut().find(|e| e.def.as_ref().is_some_and(|d| d.claims(k.date, &k.track, &k.championship))) {
            e.sessions.push(i);
            continue;
        }
        let week = week_start(k.date);
        match events[named..].iter_mut().find(|e| {
            week_start(e.start) == week && e.track.eq_ignore_ascii_case(&k.track) && e.championship.eq_ignore_ascii_case(&k.championship)
        }) {
            Some(e) => {
                e.sessions.push(i);
                e.start = e.start.min(k.date);
                e.end = e.end.max(k.date);
            }
            None => events.push(Event { def: None, track: k.track, championship: k.championship, start: k.date, end: k.date, sessions: vec![i] }),
        }
    }
    events.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)).then(a.title().cmp(&b.title())));
    events
}

/// True for a session where the kart never got going: an engine warm-up.
pub fn is_warm_up(s: &SessionSummary) -> bool {
    match s.max_speed {
        Some(v) => v < WARM_UP_MAX_KMH,
        None => s.laps.unwrap_or(0) == 0,
    }
}

/// Peak exhaust temperature of each session, read from the session files in
/// the background (the logger's summary doesn't record it) and cached in
/// `peak-egt.json` in the cache folder.
pub fn spawn_peak_egt(paths: Vec<PathBuf>, ctx: eframe::egui::Context) -> std::sync::mpsc::Receiver<(PathBuf, Option<f64>)> {
    let (tx, rx) = std::sync::mpsc::channel();
    crate::jobs::spawn("peak-egt", move || {
        let cache_path = crate::paths::cache_dir().map(|d| d.join("peak-egt.json"));
        let mut cache: std::collections::BTreeMap<String, Option<f64>> = crate::storage::load_or_default(cache_path.clone());
        let mut changed = false;
        for path in paths {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let key = format!("{}|{size}", path.display());
            let value = match cache.get(&key) {
                Some(v) => *v,
                None => {
                    let v = crate::jobs::open_session(&path).ok().and_then(|s| {
                        s.channels
                            .iter()
                            .find(|c| c.name.eq_ignore_ascii_case("Exhaust Temp") || c.name.eq_ignore_ascii_case("EGT"))
                            .and_then(|c| c.values.iter().copied().filter(|v| v.is_finite()).reduce(f64::max))
                    });
                    cache.insert(key, v);
                    changed = true;
                    v
                }
            };
            if tx.send((path, value)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
        if let (true, Some(p)) = (changed, cache_path)
            && let Err(e) = crate::storage::save(&p, &cache)
        {
            log::warn!("couldn't save {}: {e}", p.display());
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(year: u16, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    #[test]
    fn named_event_claims_matching_sessions() {
        let def = EventDef { id: 1, name: "OVRP".into(), track: "OVRP Full".into(), start: "2026-10-17".into(), end: "2026-10-18".into(), ..Default::default() };
        assert!(def.claims(d(2026, 10, 18), "ovrp full", "2026 Club"));
        assert!(!def.claims(d(2026, 10, 19), "OVRP Full", ""));
        assert!(!def.claims(d(2026, 10, 17), "OVRP Short", ""));
        let champ = EventDef { championship: "2026 Club".into(), ..def };
        assert!(!champ.claims(d(2026, 10, 17), "OVRP Full", "Other"));
    }

    #[test]
    fn a_warm_up_without_a_track_joins_that_days_event() {
        use aim_xrk::SessionSummary;
        let session = |logger: &str, date: Date, time: &str, track: Option<&str>| crate::tracks::LocalSession {
            path: format!("/x/{logger}/{time}.xrz").into(),
            logger: logger.into(),
            summary: SessionSummary { date: Some(date), start_time: Some(time.into()), track: track.map(String::from), ..Default::default() },
            track: None,
            meta: None,
            setup: None,
            weather: None,
            kart: None,
        };
        let day = d(2026, 10, 31);
        let lib = Library {
            sessions: vec![
                session("111", day, "08:55:00", None),              // warm-up in the pits
                session("111", day, "10:14:00", Some("Club Ring")), // on track
                session("222", day, "09:00:00", None),              // another MyChron with nothing on track that day
            ],
            ..Default::default()
        };
        let events = group(&lib, &crate::profiles::Profiles::default(), &Events::default());
        let ring = events.iter().find(|e| e.track == "Club Ring").unwrap();
        assert_eq!(ring.sessions, vec![0, 1]);
        assert!(events.iter().any(|e| e.track == "No track" && e.sessions == vec![2]));
    }
}
