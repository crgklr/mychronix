//! Kart setup sheets, RaceStudio's "log sheets" cut down to what a kart
//! changes: gearing, tires and pressures, weight and ballast, jetting.
//!
//! A sheet is saved beside a session as `a_NNNN.setup.json`. A session
//! without one uses the previous session's (same logger), so a sheet only
//! needs writing when something changes; the first sessions fall back to the
//! baseline of the kart the logger is on (older versions kept baselines by
//! logger in `~/Mychronix/setups.json`, still read for loggers not on a kart).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tracks::Library;

/// Values are kept as typed ("250", "~10", "10/80"), so nothing typed is lost.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, from = "SetupFile")]
pub struct Setup {
    /// Clutch (engine) sprocket teeth.
    pub drive_teeth: String,
    /// Rear axle sprocket teeth.
    pub driven_teeth: String,
    /// Brand and compound, e.g. "MG Yellow SM2".
    /// Older sheets spelled it "tyres" and put the size in it too.
    #[serde(alias = "tyres")]
    pub tires: String,
    /// Tire sizes as marked on the sidewall, e.g. "10x4.60-5".
    pub front_size: String,
    pub rear_size: String,
    pub front_psi: String,
    pub rear_psi: String,
    /// Kart with driver, lb.
    pub weight_lb: String,
    pub ballast_lb: String,
    pub jetting: String,
    /// About this session only; not carried to the next.
    pub notes: String,
}

/// A sheet as stored; older sheets have the tire size inside `tires`, which
/// loading moves into the size fields.
#[derive(Deserialize, Default)]
#[serde(default)]
struct SetupFile {
    drive_teeth: String,
    driven_teeth: String,
    #[serde(alias = "tyres")]
    tires: String,
    front_size: String,
    rear_size: String,
    front_psi: String,
    rear_psi: String,
    weight_lb: String,
    ballast_lb: String,
    jetting: String,
    notes: String,
}

impl From<SetupFile> for Setup {
    fn from(f: SetupFile) -> Self {
        let mut s = Setup {
            drive_teeth: f.drive_teeth,
            driven_teeth: f.driven_teeth,
            tires: f.tires,
            front_size: f.front_size,
            rear_size: f.rear_size,
            front_psi: f.front_psi,
            rear_psi: f.rear_psi,
            weight_lb: f.weight_lb,
            ballast_lb: f.ballast_lb,
            jetting: f.jetting,
            notes: f.notes,
        };
        if s.front_size.trim().is_empty() && s.rear_size.trim().is_empty() && let Some(size) = s.tires.split_whitespace().find(|w| is_tire_size(w)).map(str::to_string) {
            s.tires = s.tires.split_whitespace().filter(|w| *w != size).collect::<Vec<_>>().join(" ");
            s.front_size = size.clone();
            s.rear_size = size;
        }
        s
    }
}

/// Whether `w` looks like a kart tire size: "10x4.60-5", "11x7.10-5", "4.50/10.0-5".
pub fn is_tire_size(w: &str) -> bool {
    let w = w.to_lowercase();
    (w.contains('x') || w.contains('/')) && w.contains('-') && w.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Common kart tire sizes, offered in the size fields.
pub const TIRE_SIZES: [&str; 8] = ["10x4.00-5", "10x4.50-5", "10x4.60-5", "11x5.00-5", "11x6.00-5", "11x6.50-5", "11x7.10-5", "11x7.10-6"];

/// How to reach one of a setup's fields.
type Field = fn(&Setup) -> &String;

/// The fields that carry over, with their labels (notes don't).
const FIELDS: [(&str, Field); 10] = [
    ("Drive sprocket", |s| &s.drive_teeth),
    ("Rear sprocket", |s| &s.driven_teeth),
    ("Tires", |s| &s.tires),
    ("Front tire size", |s| &s.front_size),
    ("Rear tire size", |s| &s.rear_size),
    ("Front pressure", |s| &s.front_psi),
    ("Rear pressure", |s| &s.rear_psi),
    ("Weight", |s| &s.weight_lb),
    ("Ballast", |s| &s.ballast_lb),
    ("Jetting", |s| &s.jetting),
];

impl Setup {
    pub fn path_for(session: &Path) -> PathBuf {
        session.with_extension("setup.json")
    }

    pub fn load(session: &Path) -> Option<Self> {
        crate::storage::load(&Self::path_for(session))
    }

    pub fn save(&self, session: &Path) -> std::io::Result<()> {
        crate::storage::save(&Self::path_for(session), self)
    }

    pub fn remove(session: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(Self::path_for(session)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// The same apart from notes.
    pub fn same_setup(&self, other: &Setup) -> bool {
        FIELDS.iter().all(|(_, f)| f(self).trim() == f(other).trim())
    }

    /// What carries over to the next session (everything but notes).
    pub fn carried(&self) -> Setup {
        Setup { notes: String::new(), ..self.clone() }
    }

    /// "Rear sprocket 80 → 81" for each field that differs from `before`.
    pub fn changes_from(&self, before: &Setup) -> Vec<String> {
        FIELDS
            .iter()
            .filter(|(_, f)| f(self).trim() != f(before).trim())
            .map(|(label, f)| {
                let show = |v: &str| if v.trim().is_empty() { "—".to_string() } else { v.trim().to_string() };
                format!("{label} {} → {}", show(f(before)), show(f(self)))
            })
            .collect()
    }

    /// "Gearing 11/85  ·  Brand 10x4.50-5  ·  12 psi  ·  290 lb (20 lb ballast)"
    pub fn summary(&self) -> String {
        let t = |s: &String| s.trim().to_string();
        let mut parts = Vec::new();
        match (t(&self.drive_teeth), t(&self.driven_teeth)) {
            (d, r) if !d.is_empty() && !r.is_empty() => parts.push(format!("Gearing {d}/{r}")),
            (d, r) if !r.is_empty() => parts.push(format!("{r}T rear{d}")),
            _ => {}
        }
        let sizes = match (t(&self.front_size), t(&self.rear_size)) {
            (f, r) if !f.is_empty() && f == r => f,
            (f, r) if !f.is_empty() && !r.is_empty() => format!("{f} / {r}"),
            (f, r) => format!("{f}{r}"),
        };
        match (t(&self.tires), sizes) {
            (n, z) if !n.is_empty() && !z.is_empty() => parts.push(format!("{n} {z}")),
            (n, z) if !n.is_empty() || !z.is_empty() => parts.push(format!("{n}{z}")),
            _ => {}
        }
        match (t(&self.front_psi), t(&self.rear_psi)) {
            (f, r) if !f.is_empty() && f == r => parts.push(format!("{f} psi")),
            (f, r) if !f.is_empty() || !r.is_empty() => parts.push(format!("{} / {} psi front/rear", or_dash(&f), or_dash(&r))),
            _ => {}
        }
        match (t(&self.weight_lb), t(&self.ballast_lb)) {
            (w, b) if !w.is_empty() && !b.is_empty() => parts.push(format!("{w} lb ({b} lb ballast)")),
            (w, _) if !w.is_empty() => parts.push(format!("{w} lb")),
            (_, b) if !b.is_empty() => parts.push(format!("{b} lb ballast")),
            _ => {}
        }
        if !self.jetting.trim().is_empty() {
            parts.push(format!("Jetting {}", t(&self.jetting)));
        }
        parts.join("  ·  ")
    }

    /// Engine RPM per km/h the gearing and tire size should give with the clutch locked.
    pub fn expected_rpm_per_kmh(&self) -> Option<f64> {
        let ratio = number(&self.driven_teeth)? / number(&self.drive_teeth)?;
        let circumference_m = std::f64::consts::PI * tire_diameter_in(self.rear_tire())? * 0.0254;
        Some(ratio * 1000.0 / 60.0 / circumference_m)
    }

    /// The rear tire's size (older sheets: the tire text, which held it).
    pub fn rear_tire(&self) -> &str {
        if self.rear_size.trim().is_empty() { &self.tires } else { &self.rear_size }
    }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}

/// The first number in `s` ("~250 lb" → 250).
pub fn number(s: &str) -> Option<f64> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let rest = &s[start..];
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
    rest[..end].parse().ok().filter(|v: &f64| *v > 0.0)
}

/// Outer diameter in inches from a size like "10x4.60-5".
pub fn tire_diameter_in(tires: &str) -> Option<f64> {
    let lower = tires.to_lowercase();
    let x = lower.find('x')?;
    let before = lower[..x].trim_end();
    let start = before.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map(|i| i + 1).unwrap_or(0);
    before[start..].parse().ok().filter(|d: &f64| (6.0..=16.0).contains(d))
}

/// Name of the speed channel worked out from RPM, for sessions without GPS.
pub const SPEED_FROM_RPM: &str = "Speed (from RPM)";

/// Below this the clutch is out (idling or rolling), so RPM says nothing
/// about speed: about 3,500 rpm on a kart 2-stroke, lower on a 4-stroke.
pub fn typical_clutch_rpm(engine: &str) -> f64 {
    let e = engine.to_lowercase();
    let four_stroke = ["lo206", "briggs", "tillotson", "honda", "predator", "clone", "gx"].iter().any(|k| e.contains(k));
    if four_stroke { 2200.0 } else { 3500.0 }
}

/// Whether the engine has a gearbox, so one gearing can't turn RPM into speed.
pub fn is_shifter(engine: &str) -> bool {
    let e = engine.to_lowercase();
    e.contains("shifter") || e.contains("175") || e.contains("kz")
}

/// A speed channel from engine RPM, the gearing and the rear tire size, for
/// a session with no GPS speed on a single-gear kart; None without RPM, without
/// that setup, or for a shifter.
/// `clutch_rpm`: the kart's own clutch engagement, else the engine's typical one.
pub fn speed_from_rpm(session: &aim_xrk::Session, setup: &Setup, engine: &str, clutch_rpm: Option<f64>) -> Option<aim_xrk::Channel> {
    if session.channel("GPS Speed").is_some() || is_shifter(engine) {
        return None;
    }
    let clutch_in = clutch_rpm.unwrap_or_else(|| typical_clutch_rpm(engine));
    let per_kmh = setup.expected_rpm_per_kmh()?;
    let rpm = session.channels.iter().find(|c| (c.name == "RPM" || c.name == "Engine") && c.values.len() > 10)?;
    let values = rpm.values.iter().map(|&r| if r < clutch_in { 0.0 } else { r / per_kmh }).collect();
    Some(aim_xrk::Channel {
        name: SPEED_FROM_RPM.into(),
        group: None,
        units: "km/h".into(),
        decimals: 1,
        interpolate: true,
        times_ms: rpm.times_ms.clone(),
        values,
    })
}

/// Drive sprocket teeth that would give `rpm_per_kmh` with `driven` teeth on `tires`.
pub fn implied_drive_teeth(rpm_per_kmh: f64, driven: f64, tires: &str) -> Option<f64> {
    let circumference_m = std::f64::consts::PI * tire_diameter_in(tires)? * 0.0254;
    let ratio = rpm_per_kmh * 60.0 / 1000.0 * circumference_m;
    Some(driven / ratio)
}

/// Each logger's setup before its first sheet.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Baselines {
    pub loggers: BTreeMap<String, Setup>,
    /// Earlier baselines (from the karts), oldest first: each applies to
    /// sessions that started up to its `until`.
    #[serde(skip)]
    pub past: BTreeMap<String, Vec<crate::profiles::PastBaseline>>,
}

impl Baselines {
    fn path() -> Option<PathBuf> {
        crate::paths::data("setups.json")
    }

    /// What `setups.json` holds.
    pub fn saved() -> Self {
        crate::storage::load_or_default(Self::path())
    }

    /// Each logger's baseline: its kart's, else what `setups.json` holds.
    pub fn load() -> Self {
        Self::with_karts(Self::saved(), &crate::profiles::Profiles::load())
    }

    pub fn with_karts(mut saved: Self, profiles: &crate::profiles::Profiles) -> Self {
        // Each kart starts from its baseline (keyed as `kart_key`).
        for k in &profiles.karts {
            if k.baseline != Setup::default() {
                saved.loggers.insert(kart_key(k.id), k.baseline.clone());
            }
            if !k.past_baselines.is_empty() {
                saved.past.insert(kart_key(k.id), k.past_baselines.clone());
            }
        }
        saved
    }

    /// The baseline for `key` at a session that started at `stamp`, with
    /// which one it is (0 the oldest): an earlier one if the session came
    /// before that baseline was changed, else the current one.
    fn at(&self, key: &str, stamp: &str) -> (usize, Option<Setup>) {
        let past = self.past.get(key).map(Vec::as_slice).unwrap_or_default();
        let some = |s: &Setup| Some(s.clone()).filter(|s| *s != Setup::default());
        match past.iter().position(|p| !stamp.is_empty() && stamp <= p.until.as_str()) {
            Some(k) => (k, some(&past[k].setup)),
            None => (past.len(), self.loggers.get(key).and_then(some)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The session has its own sheet.
    Own,
    /// Carried over from the most recent session with a sheet (index into the library).
    From(usize),
    /// The logger's baseline.
    Baseline,
    /// Nothing recorded yet.
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub setup: Setup,
    pub source: Source,
    /// What this session's own sheet changed, against the setup before it.
    pub changes: Vec<String>,
}

/// Every session's setup, by library index.
/// What setups carry over within: a kart (whichever MyChron it had then).
fn kart_key(id: u32) -> String {
    format!("kart:{id}")
}

pub fn resolve(lib: &Library, baselines: &Baselines) -> Vec<Resolved> {
    let own: Vec<Option<Setup>> = lib.sessions.iter().map(|s| s.setup.clone()).collect();
    let order = chronological(lib);
    resolve_with(&own, &keys(lib), &stamps(lib), &order, baselines)
}

fn stamps(lib: &Library) -> Vec<String> {
    lib.sessions.iter().map(|s| s.start()).collect()
}

fn chronological(lib: &Library) -> Vec<usize> {
    let mut order: Vec<usize> = (0..lib.sessions.len()).collect();
    order.sort_by_key(|&i| {
        let s = &lib.sessions[i].summary;
        (s.date, s.start_time.clone(), s.number)
    });
    order
}

fn resolve_with(own: &[Option<Setup>], loggers: &[String], stamps: &[String], order: &[usize], baselines: &Baselines) -> Vec<Resolved> {
    let mut out = vec![Resolved { setup: Setup::default(), source: Source::Unknown, changes: vec![] }; own.len()];
    // Per logger: the setup in force, where it came from, and under which baseline.
    let mut current: BTreeMap<&str, (Setup, Source, usize)> = BTreeMap::new();
    for &i in order {
        let logger = loggers[i].as_str();
        let (era, base) = baselines.at(logger, &stamps[i]);
        // A setup carries over within one baseline; the first session after
        // the baseline was changed starts from the new one.
        let before = match current.get(logger) {
            Some((setup, source, e)) if *e == era => Some((setup.clone(), source.clone())),
            _ => base.map(|b| (b.carried(), Source::Baseline)),
        };
        out[i] = match &own[i] {
            Some(sheet) => {
                let changes = before.as_ref().map(|(b, _)| sheet.changes_from(b)).unwrap_or_default();
                current.insert(logger, (sheet.carried(), Source::From(i), era));
                Resolved { setup: sheet.clone(), source: Source::Own, changes }
            }
            None => match before {
                Some((setup, source)) => {
                    current.insert(logger, (setup.clone(), source.clone(), era));
                    Resolved { setup, source, changes: vec![] }
                }
                None => Resolved { setup: Setup::default(), source: Source::Unknown, changes: vec![] },
            },
        };
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn old_sheets_move_the_tire_size_out_of_the_tires_text() {
        let s: Setup = serde_json::from_str(r#"{"tires": "MG Yellow 10x4.60-5 SM2", "drive_teeth": "10"}"#).unwrap();
        assert_eq!((s.tires.as_str(), s.front_size.as_str(), s.rear_size.as_str()), ("MG Yellow SM2", "10x4.60-5", "10x4.60-5"));
        // New sheets keep what they say.
        let s: Setup = serde_json::from_str(r#"{"tires": "MG Red", "front_size": "10x4.50-5", "rear_size": "11x7.10-5"}"#).unwrap();
        assert_eq!((s.tires.as_str(), s.rear_tire()), ("MG Red", "11x7.10-5"));
    }

    use super::*;

    fn gearing(drive: &str, driven: &str) -> Setup {
        Setup { drive_teeth: drive.into(), driven_teeth: driven.into(), tires: "MG Yellow 10x4.60-5 SM2".into(), ..Default::default() }
    }

    #[test]
    fn sessions_inherit_the_previous_sheet() {
        let mut baselines = Baselines::default();
        baselines.loggers.insert("A".into(), gearing("10", "80"));
        let mut changed = gearing("10", "81");
        changed.notes = "Went up one for the long straight".into();
        let own = vec![None, Some(changed), None, None];
        let loggers = vec!["A".to_string(), "A".into(), "A".into(), "B".into()];
        let stamps = vec![String::new(); 4];
        let r = resolve_with(&own, &loggers, &stamps, &[0, 1, 2, 3], &baselines);
        assert_eq!(r[0].source, Source::Baseline);
        assert_eq!(r[0].setup.driven_teeth, "80");
        assert_eq!(r[1].source, Source::Own);
        assert_eq!(r[1].changes, ["Rear sprocket 80 → 81"]);
        // Carried on, without the notes.
        assert_eq!(r[2].source, Source::From(1));
        assert_eq!(r[2].setup.driven_teeth, "81");
        assert!(r[2].setup.notes.is_empty());
        // Another logger has nothing.
        assert_eq!(r[3].source, Source::Unknown);
    }

    #[test]
    fn a_changed_baseline_applies_from_then_on() {
        // Three sessions on 80, then the baseline changed to 82, then two more.
        let mut kart = crate::profiles::Kart { baseline: gearing("10", "80"), ..Default::default() };
        kart.change_baseline(gearing("10", "82"), Some("2026-10-11 02:21:31".into()));
        let mut baselines = Baselines::default();
        baselines.loggers.insert("K".into(), kart.baseline.clone());
        baselines.past.insert("K".into(), kart.past_baselines.clone());
        let stamps: Vec<String> = ["2026-10-10 14:00:00", "2026-10-11 01:00:00", "2026-10-11 02:21:31", "2026-10-18 10:00:00", "2026-10-18 11:00:00"].map(String::from).to_vec();
        let loggers = vec!["K".to_string(); 5];
        // The second session had its own sheet (81): it carries on only until the baseline change.
        let own = vec![None, Some(gearing("10", "81")), None, None, None];
        let r = resolve_with(&own, &loggers, &stamps, &[0, 1, 2, 3, 4], &baselines);
        let teeth: Vec<&str> = r.iter().map(|x| x.setup.driven_teeth.as_str()).collect();
        assert_eq!(teeth, ["80", "81", "81", "82", "82"]);
        assert_eq!(r[3].source, Source::Baseline);
        // Changing it again before any new session doesn't add a step.
        kart.change_baseline(gearing("10", "84"), Some("2026-10-11 02:21:31".into()));
        assert_eq!(kart.past_baselines.len(), 1);
        assert_eq!(kart.past_baselines[0].setup.driven_teeth, "80");
        // A kart with no sessions yet just takes the new baseline.
        let mut fresh = crate::profiles::Kart { baseline: gearing("10", "80"), ..Default::default() };
        fresh.change_baseline(gearing("10", "82"), None);
        assert!(fresh.past_baselines.is_empty());
    }

    #[test]
    fn older_sheets_still_read() {
        let s: Setup = serde_json::from_str(r#"{"tyres": "MG Yellow"}"#).unwrap();
        assert_eq!(s.tires, "MG Yellow");
    }

    #[test]
    fn gearing_matches_the_measured_ratio() {
        // 10/80 on 10-inch tires: about 167 rpm per km/h.
        let e = gearing("10", "80").expected_rpm_per_kmh().unwrap();
        assert!((e - 167.1).abs() < 0.5, "{e}");
        let teeth = implied_drive_teeth(170.0, 80.0, "MG Yellow 10x4.60-5 SM2").unwrap();
        assert!((teeth - 10.0).abs() < 0.3, "{teeth}");
        assert_eq!(tire_diameter_in("10x4.60-5"), Some(10.0));
        assert_eq!(tire_diameter_in("MG Yellow"), None);
        assert_eq!(number("~250 lb"), Some(250.0));
    }
}

/// The setup a session would have without its own sheet: what came before it.
pub fn before(lib: &Library, baselines: &Baselines, i: usize) -> Setup {
    let mut own: Vec<Option<Setup>> = lib.sessions.iter().map(|s| s.setup.clone()).collect();
    own[i] = None;
    resolve_with(&own, &keys(lib), &stamps(lib), &chronological(lib), baselines)[i].setup.carried()
}

/// What each session's setup carries over within: its kart (whichever
/// MyChron it had then), or for a session on no kart, its MyChron.
fn keys(lib: &Library) -> Vec<String> {
    lib.sessions.iter().map(|s| s.kart.map(kart_key).unwrap_or_else(|| s.logger.clone())).collect()
}

/// The setup in force on `kart` (the kart the MyChron downloading now is
/// on), else for `logger`, after its newest session: for the next download.
pub fn latest(lib: &Library, baselines: &Baselines, logger: &str, kart: Option<u32>) -> Setup {
    let resolved = resolve(lib, baselines);
    let key = kart.map(kart_key).unwrap_or_else(|| logger.to_string());
    let keys = keys(lib);
    let newest = chronological(lib).into_iter().rev().find(|&i| keys[i] == key);
    // A baseline changed since the newest session is what runs now.
    let now = baselines.past.get(&key).map_or(0, Vec::len);
    match newest {
        Some(i) if baselines.at(&key, &lib.sessions[i].start()).0 == now => Some(resolved[i].setup.carried()),
        _ => baselines.loggers.get(&key).map(|b| b.carried()),
    }
    .unwrap_or_default()
}

/// Median engine RPM per km/h above 30 km/h (clutch locked), from RPM and GPS speed.
pub fn measured_rpm_per_kmh(session: &aim_xrk::Session) -> Option<f64> {
    let rpm = session.channels.iter().find(|c| c.name == "RPM")?;
    let gps = session.gps.as_ref()?;
    let mut r: Vec<f64> = gps
        .times_ms
        .iter()
        .zip(&gps.speed_kmh)
        .filter(|(_, v)| **v > 30.0)
        .filter_map(|(&t, &v)| {
            let i = rpm.times_ms.partition_point(|&x| x < t).min(rpm.values.len().checked_sub(1)?);
            Some(rpm.values[i] / v)
        })
        .filter(|x| x.is_finite() && *x > 0.0)
        .collect();
    if r.len() < 100 {
        return None;
    }
    r.sort_by(|a, b| a.total_cmp(b));
    Some(r[r.len() / 2])
}
