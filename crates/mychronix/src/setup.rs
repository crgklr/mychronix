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
    /// Cold pressures, set before going out.
    pub front_psi: String,
    pub rear_psi: String,
    /// Hot pressures, read when the kart comes in; about this session only,
    /// like notes.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub front_hot_psi: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rear_hot_psi: String,
    /// Each tire's pressures, when they differ side to side; then the front
    /// and rear ones above are empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub each_tire: Option<EachTire>,
    /// Kart with driver, lb.
    pub weight_lb: String,
    pub ballast_lb: String,
    pub jetting: String,
    /// About this session only; not carried to the next.
    pub notes: String,
    /// Pressures in bar and weights in kg, as typed with Settings on metric;
    /// otherwise psi and lb (every sheet from before this was kept).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub metric: bool,
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
    front_hot_psi: String,
    rear_hot_psi: String,
    each_tire: Option<EachTire>,
    weight_lb: String,
    ballast_lb: String,
    jetting: String,
    notes: String,
    metric: bool,
}

/// Pressures tire by tire: left front, right front, left rear, right rear.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EachTire {
    pub cold: [String; 4],
    #[serde(skip_serializing_if = "all_empty")]
    pub hot: [String; 4],
}

fn all_empty(v: &[String; 4]) -> bool {
    v.iter().all(|x| x.trim().is_empty())
}

/// What `EachTire`'s pressures are labeled, in order.
pub const TIRE_NAMES: [&str; 4] = ["LF", "RF", "LR", "RR"];

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
            front_hot_psi: f.front_hot_psi,
            rear_hot_psi: f.rear_hot_psi,
            each_tire: f.each_tire,
            weight_lb: f.weight_lb,
            ballast_lb: f.ballast_lb,
            jetting: f.jetting,
            notes: f.notes,
            metric: f.metric,
        };
        if s.front_size.trim().is_empty()
            && s.rear_size.trim().is_empty()
            && let Some(size) = s
                .tires
                .split_whitespace()
                .find(|w| is_tire_size(w))
                .map(str::to_string)
        {
            s.tires = s
                .tires
                .split_whitespace()
                .filter(|w| *w != size)
                .collect::<Vec<_>>()
                .join(" ");
            s.front_size = size.clone();
            s.rear_size = size;
        }
        s.tidy();
        s
    }
}

/// Whether `w` looks like a kart tire size: "10x4.60-5", "11x7.10-5", "4.50/10.0-5".
pub fn is_tire_size(w: &str) -> bool {
    let w = w.to_lowercase();
    (w.contains('x') || w.contains('/'))
        && w.contains('-')
        && w.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Common kart tire sizes, offered in the size fields.
pub const TIRE_SIZES: [&str; 8] = [
    "10x4.00-5",
    "10x4.50-5",
    "10x4.60-5",
    "11x5.00-5",
    "11x6.00-5",
    "11x6.50-5",
    "11x7.10-5",
    "11x7.10-6",
];

/// How to reach one of a setup's fields.
type Field = fn(&Setup) -> &String;

/// The fields that carry over, with their labels (notes and hot pressures
/// don't), but for the cold pressures: those go after the tire sizes.
const FIELDS: [(&str, Field); 8] = [
    ("Drive sprocket", |s| &s.drive_teeth),
    ("Rear sprocket", |s| &s.driven_teeth),
    ("Tires", |s| &s.tires),
    ("Front tire size", |s| &s.front_size),
    ("Rear tire size", |s| &s.rear_size),
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

    /// The same apart from notes and hot pressures.
    pub fn same_setup(&self, other: &Setup) -> bool {
        let each = self.cold_uneven() || other.cold_uneven();
        self.fields(each)
            .iter()
            .zip(other.fields(each))
            .all(|((_, a), (_, b))| a.trim() == b.trim())
    }

    /// Each field that carries over, labeled, as typed: the cold pressures
    /// tire by tire with `each`, else front and rear.
    fn fields(&self, each: bool) -> Vec<(&'static str, &str)> {
        let cold = self.cold_each();
        let pressures = if each {
            vec![
                ("LF pressure", cold[0]),
                ("RF pressure", cold[1]),
                ("LR pressure", cold[2]),
                ("RR pressure", cold[3]),
            ]
        } else {
            vec![("Front pressure", cold[0]), ("Rear pressure", cold[2])]
        };
        let field = |(label, f): &(&'static str, Field)| (*label, f(self).as_str());
        FIELDS[..5]
            .iter()
            .map(field)
            .chain(pressures)
            .chain(FIELDS[5..].iter().map(field))
            .collect()
    }

    /// Whether it has anything for this session only (notes, hot pressures),
    /// which keeps a sheet of its own even when the setup didn't change.
    pub fn has_session_only(&self) -> bool {
        !self.notes.trim().is_empty() || self.hot_each().iter().any(|p| !p.is_empty())
    }

    /// Cold pressures tire by tire (LF, RF, LR, RR): each tire's own, else
    /// the front one twice and the rear one twice.
    pub fn cold_each(&self) -> [&str; 4] {
        match &self.each_tire {
            Some(e) => e.cold.each_ref().map(|p| p.trim()),
            None => pair(&self.front_psi, &self.rear_psi),
        }
    }

    /// Hot pressures tire by tire, as `cold_each`.
    pub fn hot_each(&self) -> [&str; 4] {
        match &self.each_tire {
            Some(e) => e.hot.each_ref().map(|p| p.trim()),
            None => pair(&self.front_hot_psi, &self.rear_hot_psi),
        }
    }

    /// Whether the cold pressures differ left to right on either axle.
    fn cold_uneven(&self) -> bool {
        uneven(self.cold_each())
    }

    /// Whether any pressure differs left to right: such a sheet is shown
    /// tire by tire, as front and rear would lose what it says.
    pub fn uneven(&self) -> bool {
        uneven(self.cold_each()) || uneven(self.hot_each())
    }

    /// Gives each tire its own pressures, starting from the front and rear ones.
    pub fn split_tires(&mut self) {
        if self.each_tire.is_some() {
            return;
        }
        self.each_tire = Some(EachTire {
            cold: self.cold_each().map(str::to_string),
            hot: self.hot_each().map(str::to_string),
        });
        for p in [
            &mut self.front_psi,
            &mut self.rear_psi,
            &mut self.front_hot_psi,
            &mut self.rear_hot_psi,
        ] {
            p.clear();
        }
    }

    /// Back to front and rear pressures, if that loses nothing: when each
    /// tire's are the same as the other side's.
    pub fn tidy(&mut self) {
        if self.each_tire.is_none() || self.uneven() {
            return;
        }
        let [front, _, rear, _] = self.cold_each().map(str::to_string);
        let [front_hot, _, rear_hot, _] = self.hot_each().map(str::to_string);
        (self.front_psi, self.rear_psi) = (front, rear);
        (self.front_hot_psi, self.rear_hot_psi) = (front_hot, rear_hot);
        self.each_tire = None;
    }

    /// Fills in the hot pressures from tire pressure sensors (`bar`, LF, RF,
    /// LR, RR; see [`tpms_hot`]) when none were typed, in the sheet's units:
    /// front and rear when each axle's sides read the same, else tire by tire.
    pub fn fill_hot(&mut self, bar: [f64; 4]) {
        if self.hot_each().iter().any(|p| !p.is_empty()) {
            return;
        }
        let hot = bar.map(|b| {
            if self.metric {
                format!("{b:.2}")
            } else {
                format!("{:.1}", b * PSI_PER_BAR)
            }
        });
        if hot[0] == hot[1] && hot[2] == hot[3] && self.each_tire.is_none() {
            self.front_hot_psi = hot[0].clone();
            self.rear_hot_psi = hot[2].clone();
        } else {
            self.split_tires();
            if let Some(e) = &mut self.each_tire {
                e.hot = hot;
            }
        }
    }

    /// The cold pressures as typed, without units: "10", "10 / 10.5" front
    /// and rear, or "10/10.2/10.5/10.5" tire by tire; empty if none.
    pub fn cold_text(&self) -> String {
        row_text(self.cold_each(), self.uneven())
    }

    /// The hot pressures, as `cold_text`.
    pub fn hot_text(&self) -> String {
        row_text(self.hot_each(), self.uneven())
    }

    /// The units its pressures were typed in: "psi", or "bar".
    pub fn pressure_unit(&self) -> &'static str {
        if self.metric { "bar" } else { "psi" }
    }

    /// The units its weights were typed in: "lb", or "kg".
    pub fn weight_unit(&self) -> &'static str {
        if self.metric { "kg" } else { "lb" }
    }

    /// What carries over to the next session (everything but notes and hot
    /// pressures).
    pub fn carried(&self) -> Setup {
        let mut s = Setup {
            notes: String::new(),
            front_hot_psi: String::new(),
            rear_hot_psi: String::new(),
            ..self.clone()
        };
        if let Some(e) = &mut s.each_tire {
            e.hot = Default::default();
        }
        s.tidy();
        s
    }

    /// "Rear sprocket 80 → 81" for each field that differs from `before`
    /// (the cold pressures tire by tire if either differs side to side).
    pub fn changes_from(&self, before: &Setup) -> Vec<String> {
        let each = self.cold_uneven() || before.cold_uneven();
        self.fields(each)
            .into_iter()
            .zip(before.fields(each))
            .filter(|((_, now), (_, was))| now.trim() != was.trim())
            .map(|((label, now), (_, was))| {
                let show = |v: &str| {
                    if v.trim().is_empty() {
                        "—".to_string()
                    } else {
                        v.trim().to_string()
                    }
                };
                format!("{label} {} → {}", show(was), show(now))
            })
            .collect()
    }

    /// "Gearing 11/85  ·  Brand 10x4.50-5  ·  12 psi  ·  290 lb (20 lb ballast)",
    /// with hot pressures when there are some: "10 / 10.5 psi cold, 12 / 12.5 hot".
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
        let (psi, lb) = (self.pressure_unit(), self.weight_unit());
        // Which pressure is which: front and rear, or each tire's (said once, at the end).
        let (which, tires) = if self.uneven() {
            ("LF/RF/LR/RR", " (LF/RF/LR/RR)")
        } else {
            ("front/rear", "")
        };
        let cold = self.cold_each();
        let one = cold.iter().all(|p| *p == cold[0]);
        match (self.cold_text(), self.hot_text()) {
            (c, h) if h.is_empty() && !c.is_empty() && one => parts.push(format!("{c} {psi}")),
            (c, h) if h.is_empty() && !c.is_empty() => parts.push(format!("{c} {psi} {which}")),
            (c, h) if c.is_empty() && !h.is_empty() => parts.push(format!("{h} {psi} hot{tires}")),
            (c, h) if !c.is_empty() => parts.push(format!("{c} {psi} cold, {h} hot{tires}")),
            _ => {}
        }
        match (t(&self.weight_lb), t(&self.ballast_lb)) {
            (w, b) if !w.is_empty() && !b.is_empty() => {
                parts.push(format!("{w} {lb} ({b} {lb} ballast)"))
            }
            (w, _) if !w.is_empty() => parts.push(format!("{w} {lb}")),
            (_, b) if !b.is_empty() => parts.push(format!("{b} {lb} ballast")),
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
        if self.rear_size.trim().is_empty() {
            &self.tires
        } else {
            &self.rear_size
        }
    }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}

/// A front and a rear pressure, tire by tire.
fn pair<'a>(front: &'a str, rear: &'a str) -> [&'a str; 4] {
    let (f, r) = (front.trim(), rear.trim());
    [f, f, r, r]
}

/// Whether a left tire's pressure differs from the right one's (LF, RF, LR, RR).
fn uneven(p: [&str; 4]) -> bool {
    p[0] != p[1] || p[2] != p[3]
}

/// Pressures as typed (LF, RF, LR, RR): all four with `each`, else front
/// and rear, or one when they're the same; empty if none.
fn row_text(p: [&str; 4], each: bool) -> String {
    if p.iter().all(|x| x.is_empty()) {
        String::new()
    } else if each {
        p.map(or_dash).join("/")
    } else if p[0] == p[2] {
        p[0].to_string()
    } else {
        format!("{} / {}", or_dash(p[0]), or_dash(p[2]))
    }
}

/// The first number in `s` ("~250 lb" → 250).
pub fn number(s: &str) -> Option<f64> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let rest = &s[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok().filter(|v: &f64| *v > 0.0)
}

/// Outer diameter in inches from a size like "10x4.60-5".
pub fn tire_diameter_in(tires: &str) -> Option<f64> {
    let lower = tires.to_lowercase();
    let x = lower.find('x')?;
    let before = lower[..x].trim_end();
    let start = before
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|i| i + 1)
        .unwrap_or(0);
    before[start..]
        .parse()
        .ok()
        .filter(|d: &f64| (6.0..=16.0).contains(d))
}

/// Name of the speed channel worked out from RPM, for sessions without GPS.
pub const SPEED_FROM_RPM: &str = "Speed (from RPM)";

/// Below this the clutch is out (idling or rolling), so RPM says nothing
/// about speed: about 3,500 rpm on a kart 2-stroke, lower on a 4-stroke.
pub fn typical_clutch_rpm(engine: &str) -> f64 {
    let e = engine.to_lowercase();
    let four_stroke = [
        "lo206",
        "briggs",
        "tillotson",
        "honda",
        "predator",
        "clone",
        "gx",
    ]
    .iter()
    .any(|k| e.contains(k));
    if four_stroke { 2200.0 } else { 3500.0 }
}

/// Whether a channel was worked out by Mychronix (speed from RPM), not recorded.
pub fn is_calculated(name: &str) -> bool {
    name == SPEED_FROM_RPM
}

/// Adds speed worked out from RPM to library session `i` when it has no GPS
/// speed: from its setup (gearing, rear tire) and its kart's engine and
/// clutch. Before units are converted, so it's converted with the rest.
pub fn add_speed_from_rpm(
    session: &mut aim_xrk::Session,
    lib: &Library,
    baselines: &Baselines,
    i: usize,
    kart: Option<&crate::profiles::Kart>,
) {
    if session.channel("GPS Speed").is_some() {
        return;
    }
    let engine = kart.map(|k| k.engine.clone()).unwrap_or_default();
    let clutch = kart.and_then(|k| number(&k.clutch_rpm));
    if let Some(ch) = resolve(lib, baselines)
        .get(i)
        .and_then(|r| speed_from_rpm(session, &r.setup, &engine, clutch))
    {
        session.channels.push(ch);
    }
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
pub fn speed_from_rpm(
    session: &aim_xrk::Session,
    setup: &Setup,
    engine: &str,
    clutch_rpm: Option<f64>,
) -> Option<aim_xrk::Channel> {
    if session.channel("GPS Speed").is_some() || is_shifter(engine) {
        return None;
    }
    let clutch_in = clutch_rpm.unwrap_or_else(|| typical_clutch_rpm(engine));
    let per_kmh = setup.expected_rpm_per_kmh()?;
    let rpm = crate::channels::find(session, crate::channels::Role::Rpm)
        .filter(|c| c.values.len() > 10)?;
    let values = rpm
        .values
        .iter()
        .map(|&r| if r < clutch_in { 0.0 } else { r / per_kmh })
        .collect();
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
    fn at(&self, key: &str, stamp: Option<crate::dates::Stamp>) -> (usize, Option<Setup>) {
        let past = self.past.get(key).map(Vec::as_slice).unwrap_or_default();
        let some = |s: &Setup| Some(s.clone()).filter(|s| *s != Setup::default());
        match past
            .iter()
            .position(|p| stamp.is_some() && stamp <= p.until)
        {
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

fn stamps(lib: &Library) -> Vec<Option<crate::dates::Stamp>> {
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

fn resolve_with(
    own: &[Option<Setup>],
    loggers: &[String],
    stamps: &[Option<crate::dates::Stamp>],
    order: &[usize],
    baselines: &Baselines,
) -> Vec<Resolved> {
    let mut out = vec![
        Resolved {
            setup: Setup::default(),
            source: Source::Unknown,
            changes: vec![]
        };
        own.len()
    ];
    // Per logger: the setup in force, where it came from, and under which baseline.
    let mut current: BTreeMap<&str, (Setup, Source, usize)> = BTreeMap::new();
    for &i in order {
        let logger = loggers[i].as_str();
        let (era, base) = baselines.at(logger, stamps[i]);
        // A setup carries over within one baseline; the first session after
        // the baseline was changed starts from the new one.
        let before = match current.get(logger) {
            Some((setup, source, e)) if *e == era => Some((setup.clone(), source.clone())),
            _ => base.map(|b| (b.carried(), Source::Baseline)),
        };
        out[i] = match &own[i] {
            Some(sheet) => {
                let changes = before
                    .as_ref()
                    .map(|(b, _)| sheet.changes_from(b))
                    .unwrap_or_default();
                current.insert(logger, (sheet.carried(), Source::From(i), era));
                Resolved {
                    setup: sheet.clone(),
                    source: Source::Own,
                    changes,
                }
            }
            None => match before {
                Some((setup, source)) => {
                    current.insert(logger, (setup.clone(), source.clone(), era));
                    Resolved {
                        setup,
                        source,
                        changes: vec![],
                    }
                }
                None => Resolved {
                    setup: Setup::default(),
                    source: Source::Unknown,
                    changes: vec![],
                },
            },
        };
    }
    out
}

/// The setup a session would have without its own sheet: what came before it.
pub fn before(lib: &Library, baselines: &Baselines, i: usize) -> Setup {
    let mut own: Vec<Option<Setup>> = lib.sessions.iter().map(|s| s.setup.clone()).collect();
    own[i] = None;
    resolve_with(
        &own,
        &keys(lib),
        &stamps(lib),
        &chronological(lib),
        baselines,
    )[i]
        .setup
        .carried()
}

/// What each session's setup carries over within: its kart (whichever
/// MyChron it had then), or for a session on no kart, its MyChron.
fn keys(lib: &Library) -> Vec<String> {
    lib.sessions
        .iter()
        .map(|s| s.kart.map(kart_key).unwrap_or_else(|| s.logger.clone()))
        .collect()
}

/// The setup in force on `kart` (the kart the MyChron downloading now is
/// on), else for `logger`, after its newest session: for the next download.
pub fn latest(lib: &Library, baselines: &Baselines, logger: &str, kart: Option<u32>) -> Setup {
    let resolved = resolve(lib, baselines);
    let key = kart.map(kart_key).unwrap_or_else(|| logger.to_string());
    let keys = keys(lib);
    let newest = chronological(lib)
        .into_iter()
        .rev()
        .find(|&i| keys[i] == key);
    // A baseline changed since the newest session is what runs now.
    let now = baselines.past.get(&key).map_or(0, Vec::len);
    match newest {
        Some(i) if baselines.at(&key, lib.sessions[i].start()).0 == now => {
            Some(resolved[i].setup.carried())
        }
        _ => baselines.loggers.get(&key).map(|b| b.carried()),
    }
    .unwrap_or_default()
}

/// Median engine RPM per km/h above 30 km/h (clutch locked), from RPM and GPS speed.
pub fn measured_rpm_per_kmh(session: &aim_xrk::Session) -> Option<f64> {
    let rpm = crate::channels::find(session, crate::channels::Role::Rpm)?;
    let gps = session.gps.as_ref()?;
    let mut r: Vec<f64> = gps
        .times_ms
        .iter()
        .zip(&gps.speed_kmh)
        .filter(|(_, v)| **v > 30.0)
        .filter_map(|(&t, &v)| {
            let i = rpm
                .times_ms
                .partition_point(|&x| x < t)
                .min(rpm.values.len().checked_sub(1)?);
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

const PSI_PER_BAR: f64 = 14.5038;

/// Hot tire pressures from a session's tire pressure sensors (AiM's TPMS:
/// "TPMS_Press_LF", "_RF", "_LR", "_RR"), in bar: each tire's median over the
/// last racing lap, warmed up as they are when the kart comes in. None
/// without all four, or a racing lap.
pub fn tpms_hot(session: &aim_xrk::Session) -> Option<[f64; 4]> {
    let racing = aim_xrk::laps::racing(session);
    let lap = session
        .laps
        .iter()
        .zip(&racing)
        .filter(|(_, r)| **r)
        .map(|(l, _)| *l)
        .next_back()?;
    let mut out = [0.0; 4];
    for (k, tire) in TIRE_NAMES.iter().enumerate() {
        let c = session.channel(&format!("TPMS_Press_{tire}"))?;
        let to_bar = match c.units.trim().to_lowercase().as_str() {
            "psi" => 1.0 / PSI_PER_BAR,
            "kpa" => 0.01,
            _ => 1.0,
        };
        let mut v: Vec<f64> = c
            .times_ms
            .iter()
            .zip(&c.values)
            .filter(|(t, v)| **t >= lap.start_ms && **t <= lap.end_ms && v.is_finite() && **v > 0.0)
            .map(|(_, v)| v * to_bar)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_by(f64::total_cmp);
        out[k] = v[v.len() / 2];
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hot_pressures_come_from_tire_sensors_when_none_were_typed() {
        use super::*;
        // Four 60 s laps (out, two racing, in); pressures climbing 1.5 → 1.9 bar,
        // the right rear a little higher.
        let lap = |n: i32| aim_xrk::Lap {
            number: n,
            start_ms: (n as i64 - 1) * 60_000,
            end_ms: n as i64 * 60_000,
        };
        let sensor = |tire: &str, extra: f64| aim_xrk::Channel {
            name: format!("TPMS_Press_{tire}"),
            group: None,
            units: "bar".into(),
            decimals: 2,
            interpolate: true,
            times_ms: (0..=240).map(|k| k * 1000).collect(),
            values: (0..=240)
                .map(|k| 1.5 + 0.4 * k as f64 / 240.0 + extra)
                .collect(),
        };
        let session = aim_xrk::Session {
            info: Default::default(),
            channels: vec![
                sensor("LF", 0.0),
                sensor("RF", 0.0),
                sensor("LR", 0.0),
                sensor("RR", 0.1),
            ],
            laps: (1..=4).map(lap).collect(),
            gps: None,
        };
        let bar = tpms_hot(&session).expect("read");
        // The last racing lap (lap 3: 120–180 s), its middle reading.
        assert!(
            (bar[0] - 1.75).abs() < 0.01 && (bar[3] - 1.85).abs() < 0.01,
            "{bar:?}"
        );

        // psi, tire by tire (the rears differ).
        let mut sheet = Setup::default();
        sheet.fill_hot(bar);
        assert_eq!(sheet.hot_each(), ["25.4", "25.4", "25.4", "26.8"]);
        // bar, front and rear when the sides match.
        let mut sheet = Setup {
            metric: true,
            ..Default::default()
        };
        sheet.fill_hot([1.8, 1.8, 1.9, 1.9]);
        assert_eq!(
            (sheet.front_hot_psi.as_str(), sheet.rear_hot_psi.as_str()),
            ("1.80", "1.90")
        );
        assert!(sheet.each_tire.is_none());
        // Typed ones stay.
        let mut sheet = Setup {
            front_hot_psi: "12".into(),
            ..Default::default()
        };
        sheet.fill_hot(bar);
        assert_eq!(sheet.front_hot_psi, "12");
        assert!(sheet.rear_hot_psi.is_empty());
    }

    #[test]
    fn old_sheets_move_the_tire_size_out_of_the_tires_text() {
        let s: Setup =
            serde_json::from_str(r#"{"tires": "MG Yellow 10x4.60-5 SM2", "drive_teeth": "10"}"#)
                .unwrap();
        assert_eq!(
            (
                s.tires.as_str(),
                s.front_size.as_str(),
                s.rear_size.as_str()
            ),
            ("MG Yellow SM2", "10x4.60-5", "10x4.60-5")
        );
        // New sheets keep what they say.
        let s: Setup = serde_json::from_str(
            r#"{"tires": "MG Red", "front_size": "10x4.50-5", "rear_size": "11x7.10-5"}"#,
        )
        .unwrap();
        assert_eq!((s.tires.as_str(), s.rear_tire()), ("MG Red", "11x7.10-5"));
    }

    use super::*;

    /// A start or a move time, as written in files.
    fn st(s: &str) -> Option<crate::dates::Stamp> {
        crate::dates::Stamp::parse(s)
    }

    fn gearing(drive: &str, driven: &str) -> Setup {
        Setup {
            drive_teeth: drive.into(),
            driven_teeth: driven.into(),
            tires: "MG Yellow 10x4.60-5 SM2".into(),
            ..Default::default()
        }
    }

    #[test]
    fn sessions_inherit_the_previous_sheet() {
        let mut baselines = Baselines::default();
        baselines.loggers.insert("A".into(), gearing("10", "80"));
        let mut changed = gearing("10", "81");
        changed.notes = "Went up one for the long straight".into();
        let own = vec![None, Some(changed), None, None];
        let loggers = vec!["A".to_string(), "A".into(), "A".into(), "B".into()];
        let stamps = vec![None; 4];
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
        let mut kart = crate::profiles::Kart {
            baseline: gearing("10", "80"),
            ..Default::default()
        };
        kart.change_baseline(gearing("10", "82"), st("2026-10-11 02:21:31"));
        let mut baselines = Baselines::default();
        baselines.loggers.insert("K".into(), kart.baseline.clone());
        baselines
            .past
            .insert("K".into(), kart.past_baselines.clone());
        let stamps: Vec<_> = [
            "2026-10-10 14:00:00",
            "2026-10-11 01:00:00",
            "2026-10-11 02:21:31",
            "2026-10-18 10:00:00",
            "2026-10-18 11:00:00",
        ]
        .map(st)
        .to_vec();
        let loggers = vec!["K".to_string(); 5];
        // The second session had its own sheet (81): it carries on only until the baseline change.
        let own = vec![None, Some(gearing("10", "81")), None, None, None];
        let r = resolve_with(&own, &loggers, &stamps, &[0, 1, 2, 3, 4], &baselines);
        let teeth: Vec<&str> = r.iter().map(|x| x.setup.driven_teeth.as_str()).collect();
        assert_eq!(teeth, ["80", "81", "81", "82", "82"]);
        assert_eq!(r[3].source, Source::Baseline);
        // Changing it again before any new session doesn't add a step.
        kart.change_baseline(gearing("10", "84"), st("2026-10-11 02:21:31"));
        assert_eq!(kart.past_baselines.len(), 1);
        assert_eq!(kart.past_baselines[0].setup.driven_teeth, "80");
        // A kart with no sessions yet just takes the new baseline.
        let mut fresh = crate::profiles::Kart {
            baseline: gearing("10", "80"),
            ..Default::default()
        };
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

    #[test]
    fn a_sheet_keeps_the_units_it_was_typed_in() {
        // Every sheet from before units were kept was typed in psi and lb.
        let old: Setup =
            serde_json::from_str(r#"{ "front_psi": "10", "weight_lb": "250" }"#).unwrap();
        assert_eq!((old.pressure_unit(), old.weight_unit()), ("psi", "lb"));
        assert!(!serde_json::to_string(&old).unwrap().contains("metric"));
        let metric = Setup {
            front_psi: "0.7".into(),
            metric: true,
            ..Default::default()
        };
        let back: Setup = serde_json::from_str(&serde_json::to_string(&metric).unwrap()).unwrap();
        assert_eq!((back.pressure_unit(), back.weight_unit()), ("bar", "kg"));
        assert!(back.carried().metric);
    }

    fn pressures(cold: (&str, &str), hot: (&str, &str)) -> Setup {
        Setup {
            front_psi: cold.0.into(),
            rear_psi: cold.1.into(),
            front_hot_psi: hot.0.into(),
            rear_hot_psi: hot.1.into(),
            ..Default::default()
        }
    }

    #[test]
    fn sheets_from_before_hot_pressures_read_and_save_as_they_were() {
        let old = r#"{"drive_teeth":"10","driven_teeth":"80","tires":"MG Yellow","front_size":"","rear_size":"","front_psi":"10","rear_psi":"10.5","weight_lb":"250","ballast_lb":"","jetting":"","notes":""}"#;
        let s: Setup = serde_json::from_str(old).unwrap();
        assert_eq!(s.cold_each(), ["10", "10", "10.5", "10.5"]);
        assert!(s.front_hot_psi.is_empty() && s.rear_hot_psi.is_empty());
        assert!(s.each_tire.is_none() && !s.uneven());
        assert_eq!(serde_json::to_string(&s).unwrap(), old);
    }

    #[test]
    fn the_summary_adds_hot_pressures_only_when_there_are_some() {
        let s = |cold, hot| pressures(cold, hot).summary();
        assert_eq!(s(("12", "12"), ("", "")), "12 psi");
        assert_eq!(s(("10", "10.5"), ("", "")), "10 / 10.5 psi front/rear");
        assert_eq!(
            s(("10", "10.5"), ("12", "12.5")),
            "10 / 10.5 psi cold, 12 / 12.5 hot"
        );
        assert_eq!(s(("10", "10"), ("12", "12")), "10 psi cold, 12 hot");
        assert_eq!(s(("", ""), ("12", "12.5")), "12 / 12.5 psi hot");
        // Tire by tire.
        let mut each = pressures(("10", "10.5"), ("12", "12.5"));
        each.split_tires();
        each.each_tire.as_mut().unwrap().hot[1] = "12.2".into();
        assert_eq!(
            each.summary(),
            "10/10/10.5/10.5 psi cold, 12/12.2/12.5/12.5 hot (LF/RF/LR/RR)"
        );
        // Hot pressures are this session's: the next starts without them.
        assert_eq!(each.carried().summary(), "10 / 10.5 psi front/rear");
        assert!(each.carried().each_tire.is_none());
    }

    #[test]
    fn each_tire_goes_back_to_front_and_rear_only_when_nothing_is_lost() {
        let mut s = pressures(("10", "10.5"), ("12", "12.5"));
        s.split_tires();
        assert_eq!(s.cold_each(), ["10", "10", "10.5", "10.5"]);
        assert!(s.front_psi.is_empty() && !s.uneven());
        // Left and right the same: back to front and rear.
        s.tidy();
        assert!(s.each_tire.is_none());
        assert_eq!(
            (s.front_psi.as_str(), s.rear_hot_psi.as_str()),
            ("10", "12.5")
        );
        // One tire different (hot or cold): it stays tire by tire.
        s.split_tires();
        s.each_tire.as_mut().unwrap().hot[3] = "12.8".into();
        s.tidy();
        assert!(s.each_tire.is_some() && s.uneven());
        // And a sheet saved so opens so; one saved tire by tire that needn't be opens front and rear.
        let back: Setup = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let even = r#"{"each_tire":{"cold":["10","10","11","11"]}}"#;
        let even: Setup = serde_json::from_str(even).unwrap();
        assert!(even.each_tire.is_none());
        assert_eq!(
            (even.front_psi.as_str(), even.rear_psi.as_str()),
            ("10", "11")
        );
    }

    #[test]
    fn changes_compare_cold_pressures_tire_by_tire_when_either_has_them() {
        let before = pressures(("10", "10.5"), ("", ""));
        let mut after = before.clone();
        after.split_tires();
        after.each_tire.as_mut().unwrap().cold[1] = "10.2".into();
        assert_eq!(after.changes_from(&before), ["RF pressure 10 → 10.2"]);
        assert!(!after.same_setup(&before));
        // Hot pressures aren't a change to the setup, but keep a sheet of its own.
        let hot = pressures(("10", "10.5"), ("12", ""));
        assert!(hot.changes_from(&before).is_empty() && hot.same_setup(&before));
        assert!(hot.has_session_only() && !before.has_session_only());
        let rear = pressures(("10", "11"), ("", ""));
        assert_eq!(rear.changes_from(&before), ["Rear pressure 10.5 → 11"]);
    }
}
