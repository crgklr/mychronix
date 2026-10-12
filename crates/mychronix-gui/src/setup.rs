//! Kart setup sheets, RaceStudio's "log sheets" cut down to what a kart
//! changes: gearing, tyres and pressures, weight and ballast, jetting.
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
#[serde(default)]
pub struct Setup {
    /// Clutch (engine) sprocket teeth.
    pub drive_teeth: String,
    /// Rear axle sprocket teeth.
    pub driven_teeth: String,
    /// e.g. "MG Yellow 10x4.60-5 SM2".
    pub tyres: String,
    pub front_psi: String,
    pub rear_psi: String,
    /// Kart with driver, lb.
    pub weight_lb: String,
    pub ballast_lb: String,
    pub jetting: String,
    /// About this session only; not carried to the next.
    pub notes: String,
}

/// The fields that carry over, with their labels (notes don't).
const FIELDS: [(&str, fn(&Setup) -> &String); 8] = [
    ("Drive sprocket", |s| &s.drive_teeth),
    ("Rear sprocket", |s| &s.driven_teeth),
    ("Tyres", |s| &s.tyres),
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
        if !self.tyres.trim().is_empty() {
            parts.push(t(&self.tyres));
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

    /// Engine RPM per km/h the gearing and tyre size should give with the clutch locked.
    pub fn expected_rpm_per_kmh(&self) -> Option<f64> {
        let ratio = number(&self.driven_teeth)? / number(&self.drive_teeth)?;
        let circumference_m = std::f64::consts::PI * tyre_diameter_in(&self.tyres)? * 0.0254;
        Some(ratio * 1000.0 / 60.0 / circumference_m)
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
pub fn tyre_diameter_in(tyres: &str) -> Option<f64> {
    let lower = tyres.to_lowercase();
    let x = lower.find('x')?;
    let before = lower[..x].trim_end();
    let start = before.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map(|i| i + 1).unwrap_or(0);
    before[start..].parse().ok().filter(|d: &f64| (6.0..=16.0).contains(d))
}

/// Drive sprocket teeth that would give `rpm_per_kmh` with `driven` teeth on `tyres`.
pub fn implied_drive_teeth(rpm_per_kmh: f64, driven: f64, tyres: &str) -> Option<f64> {
    let circumference_m = std::f64::consts::PI * tyre_diameter_in(tyres)? * 0.0254;
    let ratio = rpm_per_kmh * 60.0 / 1000.0 * circumference_m;
    Some(driven / ratio)
}

/// Each logger's setup before its first sheet.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Baselines {
    pub loggers: BTreeMap<String, Setup>,
}

impl Baselines {
    fn path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("setups.json"))
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
        for k in &profiles.karts {
            if !k.logger.is_empty() && k.baseline != Setup::default() {
                saved.loggers.insert(k.logger.clone(), k.baseline.clone());
            }
        }
        saved
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
pub fn resolve(lib: &Library, baselines: &Baselines) -> Vec<Resolved> {
    let own: Vec<Option<Setup>> = lib.sessions.iter().map(|s| s.setup.clone()).collect();
    let loggers: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).collect();
    let order = chronological(lib);
    resolve_with(&own, &loggers, &order, baselines)
}

fn chronological(lib: &Library) -> Vec<usize> {
    let mut order: Vec<usize> = (0..lib.sessions.len()).collect();
    order.sort_by_key(|&i| {
        let s = &lib.sessions[i].summary;
        (s.date, s.start_time.clone(), s.number)
    });
    order
}

fn resolve_with(own: &[Option<Setup>], loggers: &[String], order: &[usize], baselines: &Baselines) -> Vec<Resolved> {
    let mut out = vec![Resolved { setup: Setup::default(), source: Source::Unknown, changes: vec![] }; own.len()];
    // Per logger: the setup in force and where it came from.
    let mut current: BTreeMap<&str, (Setup, Source)> = BTreeMap::new();
    for &i in order {
        let logger = loggers[i].as_str();
        let before = current
            .get(logger)
            .cloned()
            .or_else(|| baselines.loggers.get(logger).map(|b| (b.carried(), Source::Baseline)));
        out[i] = match &own[i] {
            Some(sheet) => {
                let changes = before.as_ref().map(|(b, _)| sheet.changes_from(b)).unwrap_or_default();
                current.insert(logger, (sheet.carried(), Source::From(i)));
                Resolved { setup: sheet.clone(), source: Source::Own, changes }
            }
            None => match before {
                Some((setup, source)) => {
                    current.insert(logger, (setup.clone(), source.clone()));
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
    use super::*;

    fn gearing(drive: &str, driven: &str) -> Setup {
        Setup { drive_teeth: drive.into(), driven_teeth: driven.into(), tyres: "MG Yellow 10x4.60-5 SM2".into(), ..Default::default() }
    }

    #[test]
    fn sessions_inherit_the_previous_sheet() {
        let mut baselines = Baselines::default();
        baselines.loggers.insert("A".into(), gearing("10", "80"));
        let mut changed = gearing("10", "81");
        changed.notes = "Went up one for the long straight".into();
        let own = vec![None, Some(changed), None, None];
        let loggers = vec!["A".to_string(), "A".into(), "A".into(), "B".into()];
        let r = resolve_with(&own, &loggers, &[0, 1, 2, 3], &baselines);
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
    fn gearing_matches_the_measured_ratio() {
        // 10/80 on 10-inch tyres: about 167 rpm per km/h.
        let e = gearing("10", "80").expected_rpm_per_kmh().unwrap();
        assert!((e - 167.1).abs() < 0.5, "{e}");
        let teeth = implied_drive_teeth(170.0, 80.0, "MG Yellow 10x4.60-5 SM2").unwrap();
        assert!((teeth - 10.0).abs() < 0.3, "{teeth}");
        assert_eq!(tyre_diameter_in("10x4.60-5"), Some(10.0));
        assert_eq!(tyre_diameter_in("MG Yellow"), None);
        assert_eq!(number("~250 lb"), Some(250.0));
    }
}

/// The setup a session would have without its own sheet: what came before it.
pub fn before(lib: &Library, baselines: &Baselines, i: usize) -> Setup {
    let mut own: Vec<Option<Setup>> = lib.sessions.iter().map(|s| s.setup.clone()).collect();
    own[i] = None;
    let loggers: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).collect();
    resolve_with(&own, &loggers, &chronological(lib), baselines)[i].setup.carried()
}

/// The setup in force for `logger` after its newest session (for the next download).
pub fn latest(lib: &Library, baselines: &Baselines, logger: &str) -> Setup {
    let resolved = resolve(lib, baselines);
    chronological(lib)
        .into_iter()
        .rev()
        .find(|&i| lib.sessions[i].logger == logger)
        .map(|i| resolved[i].setup.carried())
        .or_else(|| baselines.loggers.get(logger).map(|b| b.carried()))
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
