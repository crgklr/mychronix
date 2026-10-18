//! Events: downloaded sessions grouped the way RaceStudio does (one event per
//! week, track and championship), plus events you name or plan ahead, saved
//! in `~/Mychronix/events.json`.

use std::path::PathBuf;

use aim_xrk::summary::{Date, SessionSummary};
use serde::{Deserialize, Serialize};

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
    let track = Some(meta.track)
        .filter(|t| !t.trim().is_empty())
        .or_else(|| s.summary.track.clone())
        .or_else(|| s.track.map(|t| lib.tracks[t].name.clone()))
        .unwrap_or_else(|| "No track".into());
    let championship = Some(meta.championship)
        .filter(|c| !c.trim().is_empty())
        .or_else(|| profiles.get(&s.logger).map(|p| p.championship.clone()))
        .unwrap_or_default();
    Some(SessionKey { date, track, championship })
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

/// Days since 1970-01-01 (Howard Hinnant's days_from_civil).
pub fn days(d: Date) -> i64 {
    let (m, dd) = (d.month as i64, d.day as i64);
    let y = d.year as i64 - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + dd - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `n` days after 1970-01-01.
pub fn from_days(n: i64) -> Date {
    let z = n + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u16;
    Date { year, month, day }
}

/// The Monday of `d`'s week, so a Saturday–Sunday race weekend stays together.
pub fn week_start(d: Date) -> Date {
    let n = days(d);
    // 1970-01-01 was a Thursday (3 days after Monday).
    from_days(n - (n + 3).rem_euclid(7))
}

pub fn today() -> Date {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    from_days((secs / 86_400) as i64)
}

/// "10/17/2026" as written in the US, or "2026-10-17" (also "2026-10-7").
pub fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    let (sep, us) = if s.contains('/') { ('/', true) } else { ('-', false) };
    let mut p = s.split(sep).map(|x| x.trim().parse::<u16>().ok());
    let (a, b, c) = (p.next()??, p.next()??, p.next()??);
    let (year, month, day) = if us { (c, a, b) } else { (a, b, c) };
    if p.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1990 {
        return None;
    }
    let d = Date { year, month: month as u8, day: day as u8 };
    // Reject days the month doesn't have (e.g. 31 September).
    (from_days(days(d)) == d).then_some(d)
}

/// "10/17/2026", for date fields.
pub fn us_numeric(d: Date) -> String {
    format!("{}/{}/{}", d.month, d.day, d.year)
}

/// "Oct 17, 2026".
pub fn us_short(d: Date) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}, {}", M[(d.month.clamp(1, 12) - 1) as usize], d.day, d.year)
}

/// "2:19 PM" (or "14:19" on a 24-hour clock, see Settings) from the logger's "14:19:05".
pub fn clock_time(t: &str) -> String {
    format_time(t, crate::settings::clock_24())
}

fn format_time(t: &str, h24: bool) -> String {
    let mut p = t.split(':');
    let (Some(Ok(h)), Some(m)) = (p.next().map(|h| h.trim().parse::<u32>()), p.next()) else { return t.to_string() };
    let m = m.get(..2).unwrap_or(m);
    if h24 {
        return format!("{h:02}:{m}");
    }
    let (h12, ampm) = match h { 0 => (12, "AM"), 1..=11 => (h, "AM"), 12 => (12, "PM"), _ => (h - 12, "PM") };
    format!("{h12}:{m} {ampm}")
}

pub fn iso(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(year: u16, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    #[test]
    fn days_round_trip() {
        for date in [d(1970, 1, 1), d(2000, 2, 29), d(2026, 10, 10), d(2026, 12, 31)] {
            assert_eq!(from_days(days(date)), date);
        }
        assert_eq!(days(d(1970, 1, 2)), 1);
    }

    #[test]
    fn weeks_start_on_monday() {
        // Saturday, October 10 and Sunday, October 11, 2026 share Monday, October 5.
        assert_eq!(week_start(d(2026, 10, 10)), d(2026, 10, 5));
        assert_eq!(week_start(d(2026, 10, 11)), d(2026, 10, 5));
        assert_eq!(week_start(d(2026, 10, 5)), d(2026, 10, 5));
        assert_eq!(week_start(d(2026, 10, 12)), d(2026, 10, 12));
    }

    #[test]
    fn parses_dates() {
        assert_eq!(parse_date("2026-10-17"), Some(d(2026, 10, 17)));
        assert_eq!(parse_date(" 2026-1-7 "), Some(d(2026, 1, 7)));
        assert_eq!(parse_date("2026-09-31"), None);
        assert_eq!(parse_date("17/10/2026"), None);
        assert_eq!(parse_date("10/17/2026"), Some(d(2026, 10, 17)));
        assert_eq!(us_numeric(d(2026, 1, 7)), "1/7/2026");
        assert_eq!(us_short(d(2026, 10, 17)), "Oct 17, 2026");
        assert_eq!(format_time("14:19:05", false), "2:19 PM");
        assert_eq!(format_time("14:19:05", true), "14:19");
        assert_eq!(format_time("09:05", true), "09:05");
        assert_eq!(format_time("00:05:00", false), "12:05 AM");
        assert_eq!(format_time("12:30", false), "12:30 PM");
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
