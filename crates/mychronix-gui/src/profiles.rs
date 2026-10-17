//! Who drives what, saved in `~/Mychronix/profiles.json`: the drivers, and
//! the karts with their engine, usual driver, baseline setup and the MyChron
//! mounted on them. The logger's own files leave these blank, so a session's
//! driver and kart come from the kart its MyChron is on.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A logger's driver, kart and engine (from the kart it's on), and the last
/// championship and track entered when downloading from it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LoggerProfile {
    /// Older files kept names here, one set per logger; they move to
    /// `drivers` and `karts` on load.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub driver: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kart: String,
    /// Last championship entered when downloading from this logger.
    #[serde(default)]
    pub championship: String,
    /// Last track entered when downloading from this logger.
    #[serde(default)]
    pub track: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub engine: String,
    /// The logger's model, e.g. "MyChron 6", learned from its WiFi name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Driver {
    pub id: u32,
    pub name: String,
    /// Racing class, e.g. "Mini Swift".
    pub class: String,
    /// Birthday as typed ("2017-03-14" or "3/14/2017"), so the age stays right.
    pub born: String,
    /// In race gear, lb, as typed.
    pub weight_lb: String,
}

impl Driver {
    /// Age in whole years on `today`, if the birthday reads as a date.
    pub fn age(&self, today: aim_xrk::summary::Date) -> Option<u32> {
        let b = parse_birthday(&self.born)?;
        let mut age = today.year as i32 - b.year as i32;
        if (today.month, today.day) < (b.month, b.day) {
            age -= 1;
        }
        u32::try_from(age).ok()
    }
}

/// "2017-03-14", or "3/14/2017" as written in the US.
pub fn parse_birthday(s: &str) -> Option<aim_xrk::summary::Date> {
    let s = s.trim();
    if let Some(d) = crate::events::parse_date(s) {
        return Some(d);
    }
    let mut parts = s.split('/').map(|p| p.trim().parse::<u32>().ok());
    let (Some(Some(m)), Some(Some(d)), Some(Some(y)), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return None };
    ((1..=12).contains(&m) && (1..=31).contains(&d) && (1900..=2100).contains(&y)).then_some(aim_xrk::summary::Date { year: y as u16, month: m as u8, day: d as u8 })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Kart {
    pub id: u32,
    pub name: String,
    /// The engine package, e.g. "IAME Mini Swift". A picture of it in
    /// `~/Mychronix/engines/<name>.png` stands in for the driver's initial.
    pub engine: String,
    /// Its usual driver.
    pub driver: Option<u32>,
    /// The MyChron mounted on it (logger id), or empty.
    pub logger: String,
    /// Its setup before its first setup sheet.
    pub baseline: crate::setup::Setup,
    /// RPM at which the clutch engages, if measured; else the engine's typical one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub clutch_rpm: String,
    /// MyChrons that were on it before, oldest first: their sessions up to
    /// when they came off stay this kart's (and its driver's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub past: Vec<PastMyChron>,
    /// Taken out of the Garage. Its sessions still show it and its setup.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
    /// Baselines it had before, oldest first. Changing a kart's baseline
    /// means "what it runs from now on": the old one stays in force for
    /// the sessions it had then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub past_baselines: Vec<PastBaseline>,
}

/// A baseline a kart had: it applies to the kart's sessions up to `until`
/// (the start of its newest session when the baseline was changed, as
/// `stamp` writes it). Later sessions use the next one.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PastBaseline {
    pub setup: crate::setup::Setup,
    pub until: String,
}

impl Kart {
    /// Sets its baseline to `new`. If it had sessions under the old one
    /// (`newest`: the start of its newest session), they keep it.
    pub fn change_baseline(&mut self, new: crate::setup::Setup, newest: Option<String>) {
        if new == self.baseline {
            return;
        }
        if let Some(until) = newest.filter(|n| !n.is_empty()) {
            // Two changes with no session between them: the first never applied to anything.
            match self.past_baselines.last_mut() {
                Some(p) if p.until == until => {}
                _ => self.past_baselines.push(PastBaseline { setup: self.baseline.clone(), until }),
            }
        }
        self.baseline = new;
    }
}

/// A MyChron a kart had: its sessions recorded up to `until` (the start of
/// its newest session when it came off, "2026-10-11 02:21:31") are the kart's.
/// Later ones belong to wherever it went next.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PastMyChron {
    pub logger: String,
    pub until: String,
}

/// A session's start as compared with `PastMyChron::until`: "2026-10-11 02:21:31".
pub fn stamp(date: Option<aim_xrk::summary::Date>, time: Option<&str>) -> String {
    match date {
        Some(d) => format!("{:04}-{:02}-{:02} {}", d.year, d.month, d.day, time.unwrap_or("00:00:00")),
        None => String::new(),
    }
}

impl Kart {
    /// Every MyChron that has been on it, the current one first.
    pub fn all_loggers(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.logger).chain(self.past.iter().rev().map(|p| &p.logger)).filter(|l| !l.is_empty())
    }

    /// Takes its MyChron off; its sessions up to `until` stay this kart's.
    fn unmount(&mut self, until: String) {
        let old = std::mem::take(&mut self.logger);
        if !old.is_empty() {
            self.past.push(PastMyChron { logger: old, until });
        }
    }
}

/// Racing engines offered when adding a kart, by maker: the ones with a
/// picture. Anything else is entered as "Other engine".
pub const ENGINES: [(&str, &[&str]); 8] = [
    ("BRP-Rotax", &["Rotax 125 Micro MAX", "Rotax 125 Mini MAX", "Rotax 125 Junior MAX", "Rotax 125 Senior MAX"]),
    ("IAME / Parilla", &["IAME Micro Swift", "IAME Mini Swift", "IAME X30 Mini", "IAME X30 Junior", "IAME X30 Senior", "IAME KA100", "IAME X30 Super Shifter 175"]),
    ("Vortex", &["Vortex Mini ROK", "Vortex ROK VLR", "Vortex ROK GP", "Vortex Shifter ROK"]),
    ("Comer", &["Comer C50", "Comer C51", "Comer C52"]),
    ("Briggs & Stratton", &["Briggs & Stratton LO206"]),
    ("Tillotson", &["Tillotson T225RS"]),
    ("Honda", &["Honda GX200"]),
    ("Predator", &["Predator 79", "Predator Ghost 212"]),
];

/// Whether `engine` is one of [`ENGINES`].
pub fn is_known_engine(engine: &str) -> bool {
    ENGINES.iter().flat_map(|(_, e)| e.iter()).any(|e| e.eq_ignore_ascii_case(engine.trim()))
}

/// A picture of `engine` as (uri, PNG bytes): `~/Mychronix/engines/<name>.png`
/// if you've added one, else one bundled with the app.
/// "IAME Mini Swift" → `iame-mini-swift.png`.
pub fn engine_picture(engine: &str) -> Option<(String, Vec<u8>)> {
    let slug: String = engine
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        return None;
    }
    // The Micro and Mini Swift are the same engine.
    const IAME_SWIFT: &[u8] = include_bytes!("../assets/engines/iame-mini-swift.png");
    // The Micro and Mini MAX are the same engine, so they share a picture.
    const ROTAX_MINI: &[u8] = include_bytes!("../assets/engines/rotax-mini-max.png");
    // As are the Junior and Senior MAX.
    const ROTAX_MAX: &[u8] = include_bytes!("../assets/engines/rotax-max.png");
    // The X30 Mini, Junior and Senior are one engine.
    const IAME_X30: &[u8] = include_bytes!("../assets/engines/iame-x30.png");
    const IAME_KA100: &[u8] = include_bytes!("../assets/engines/iame-ka100.png");
    const IAME_SHIFTER: &[u8] = include_bytes!("../assets/engines/iame-super-shifter-175.png");
    const VORTEX_MINI_ROK: &[u8] = include_bytes!("../assets/engines/vortex-mini-rok.png");
    const VORTEX_ROK_VLR: &[u8] = include_bytes!("../assets/engines/vortex-rok-vlr.png");
    const VORTEX_SHIFTER: &[u8] = include_bytes!("../assets/engines/vortex-shifter-rok.png");
    const TILLOTSON_225: &[u8] = include_bytes!("../assets/engines/tillotson-t225rs.png");
    const PREDATOR_GHOST: &[u8] = include_bytes!("../assets/engines/predator-ghost-212.png");
    const BRIGGS_LO206: &[u8] = include_bytes!("../assets/engines/briggs-lo206.png");
    const PREDATOR_79: &[u8] = include_bytes!("../assets/engines/predator-79.png");
    const VORTEX_ROK_GP: &[u8] = include_bytes!("../assets/engines/vortex-rok-gp.png");
    const COMER_C50: &[u8] = include_bytes!("../assets/engines/comer-c50.png");
    const COMER_C51: &[u8] = include_bytes!("../assets/engines/comer-c51.png");
    const COMER_C52: &[u8] = include_bytes!("../assets/engines/comer-c52.png");
    const HONDA_GX200: &[u8] = include_bytes!("../assets/engines/honda-gx200.png");
    const BUNDLED: [(&str, &str, &[u8]); 23] = [
        ("iame-micro-swift", "iame-mini-swift", IAME_SWIFT),
        ("iame-mini-swift", "iame-mini-swift", IAME_SWIFT),
        ("iame-ka100", "iame-ka100", IAME_KA100),
        ("iame-x30-mini", "iame-x30", IAME_X30),
        ("iame-x30-junior", "iame-x30", IAME_X30),
        ("iame-x30-senior", "iame-x30", IAME_X30),
        ("iame-x30-super-shifter-175", "iame-super-shifter-175", IAME_SHIFTER),
        ("rotax-125-micro-max", "rotax-mini-max", ROTAX_MINI),
        ("rotax-125-mini-max", "rotax-mini-max", ROTAX_MINI),
        ("rotax-125-junior-max", "rotax-max", ROTAX_MAX),
        ("rotax-125-senior-max", "rotax-max", ROTAX_MAX),
        ("vortex-mini-rok", "vortex-mini-rok", VORTEX_MINI_ROK),
        ("vortex-rok-vlr", "vortex-rok-vlr", VORTEX_ROK_VLR),
        ("vortex-rok-gp", "vortex-rok-gp", VORTEX_ROK_GP),
        ("vortex-shifter-rok", "vortex-shifter-rok", VORTEX_SHIFTER),
        ("comer-c50", "comer-c50", COMER_C50),
        ("comer-c51", "comer-c51", COMER_C51),
        ("comer-c52", "comer-c52", COMER_C52),
        ("tillotson-t225rs", "tillotson-t225rs", TILLOTSON_225),
        ("predator-ghost-212", "predator-ghost-212", PREDATOR_GHOST),
        ("predator-79", "predator-79", PREDATOR_79),
        ("briggs-stratton-lo206", "briggs-lo206", BRIGGS_LO206),
        ("honda-gx200", "honda-gx200", HONDA_GX200),
    ];
    let bundled = BUNDLED.iter().find(|(s, ..)| *s == slug);
    // Your own picture, under this engine's name or the one it shares a picture with.
    let shared = bundled.map(|(_, file, _)| *file);
    for name in std::iter::once(slug.as_str()).chain(shared) {
        let own = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("engines").join(format!("{name}.png")));
        if let Some(bytes) = own.as_ref().and_then(|p| std::fs::read(p).ok()) {
            return Some((format!("bytes://engine/{name}.png"), bytes));
        }
    }
    bundled.map(|(_, file, b)| (format!("bytes://engine/bundled/{file}.png"), b.to_vec()))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profiles {
    #[serde(default)]
    pub drivers: Vec<Driver>,
    #[serde(default)]
    pub karts: Vec<Kart>,
    /// Download defaults by logger id, e.g. "12345678".
    #[serde(default)]
    pub loggers: BTreeMap<String, LoggerProfile>,
    /// MyChrons put off with "Not now": how many of their sessions had no
    /// kart then (they're asked about again once there are more).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub snoozed: BTreeMap<String, usize>,
}

impl Profiles {
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("profiles.json"))
    }

    /// Loads saved profiles; on first run, fills in what RaceStudio 3 already knows.
    pub fn load() -> Self {
        let saved = Self::path().and_then(|p| crate::storage::load::<Profiles>(&p));
        let fresh = saved.is_none();
        let mut p = saved.unwrap_or_else(Self::from_racestudio);
        let migrated = p.migrate(&crate::setup::Baselines::saved().loggers);
        if migrated || (fresh && !p.loggers.is_empty()) {
            let _ = p.save();
        }
        p
    }

    /// Moves names kept per logger (older files) into drivers and karts, with
    /// each logger's baseline setup. True if anything moved.
    fn migrate(&mut self, baselines: &BTreeMap<String, crate::setup::Setup>) -> bool {
        let mut moved = false;
        let loggers: Vec<String> = self.loggers.keys().cloned().collect();
        for logger in loggers {
            let old = self.loggers[&logger].clone();
            if old.driver.trim().is_empty() && old.kart.trim().is_empty() && old.engine.trim().is_empty() {
                continue;
            }
            let driver = (!old.driver.trim().is_empty()).then(|| self.driver_named(&old.driver));
            if self.kart_on(&logger).is_none() {
                let id = self.next_kart_id();
                self.karts.push(Kart {
                    id,
                    name: if old.kart.trim().is_empty() { "Kart".into() } else { old.kart.trim().into() },
                    engine: old.engine.trim().into(),
                    driver,
                    logger: logger.clone(),
                    baseline: baselines.get(&logger).cloned().unwrap_or_default(),
                    ..Default::default()
                });
            }
            let entry = self.loggers.get_mut(&logger).unwrap();
            entry.driver.clear();
            entry.kart.clear();
            entry.engine.clear();
            moved = true;
        }
        moved
    }

    /// The driver called `name` (any case), added if there isn't one.
    pub fn driver_named(&mut self, name: &str) -> u32 {
        let name = name.trim();
        if let Some(d) = self.drivers.iter().find(|d| d.name.eq_ignore_ascii_case(name)) {
            return d.id;
        }
        let id = self.drivers.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        self.drivers.push(Driver { id, name: name.into(), ..Default::default() });
        id
    }

    pub fn next_kart_id(&self) -> u32 {
        self.karts.iter().map(|k| k.id).max().unwrap_or(0) + 1
    }

    pub fn driver_name(&self, id: Option<u32>) -> Option<&str> {
        let id = id?;
        self.drivers.iter().find(|d| d.id == id).map(|d| d.name.as_str())
    }

    /// The kart a logger is on, else the last one it was on (removed karts
    /// included): for when there's no session time to go by.
    pub fn kart_on(&self, logger: &str) -> Option<&Kart> {
        if logger.is_empty() {
            return None;
        }
        self.karts.iter().find(|k| k.logger == logger).or_else(|| self.last_kart_of(logger).map(|(k, _)| k))
    }

    /// The kart `logger` was last on before, and when it came off.
    pub fn last_kart_of(&self, logger: &str) -> Option<(&Kart, &str)> {
        self.karts.iter().flat_map(|k| k.past.iter().filter(|p| p.logger == logger).map(move |p| (k, p.until.as_str()))).max_by(|a, b| a.1.cmp(b.1))
    }

    /// The kart a session from `logger` that started at `when` (see [`stamp`])
    /// belongs to: the kart the MyChron was on then. None when it was on no
    /// kart then (a MyChron that left a kart and came back without one).
    pub fn kart_for(&self, logger: &str, when: &str) -> Option<&Kart> {
        if logger.is_empty() {
            return None;
        }
        let current = self.karts.iter().find(|k| k.logger == logger);
        if when.is_empty() {
            return current.or_else(|| self.kart_on(logger));
        }
        // The earliest time it came off a kart that's not before the session.
        let mut spans: Vec<(&str, &Kart)> = self.karts.iter().flat_map(|k| k.past.iter().filter(|p| p.logger == logger).map(move |p| (p.until.as_str(), k))).collect();
        spans.sort_by(|a, b| a.0.cmp(b.0));
        spans.into_iter().find(|(until, _)| when <= *until).map(|(_, k)| k).or(current)
    }

    /// The Garage's karts (not removed).
    pub fn garage_karts(&self) -> impl Iterator<Item = &Kart> {
        self.karts.iter().filter(|k| !k.removed)
    }

    /// The kart in the Garage that `logger` is on now.
    pub fn mounted_on(&self, logger: &str) -> Option<&Kart> {
        (!logger.is_empty()).then(|| self.garage_karts().find(|k| k.logger == logger)).flatten()
    }

    /// Whether `logger` has been on one of the karts (now or before).
    pub fn knows_logger(&self, logger: &str) -> bool {
        self.karts.iter().any(|k| k.all_loggers().any(|l| l == logger))
    }

    /// Puts `logger` on kart `kart`. The MyChron it replaces, and the kart
    /// this MyChron leaves, keep their sessions up to now: `newest(logger)` is
    /// the start of a MyChron's newest session (see [`stamp`]).
    pub fn mount(&mut self, logger: &str, kart: u32, newest: &dyn Fn(&str) -> String) {
        if self.karts.iter().any(|k| k.id == kart && k.logger == logger) {
            return;
        }
        let until = newest(logger);
        for k in self.karts.iter_mut().filter(|k| k.id != kart && k.logger == logger) {
            k.unmount(until.clone());
        }
        if let Some(k) = self.karts.iter_mut().find(|k| k.id == kart) {
            let old = k.logger.clone();
            k.unmount(newest(&old));
            k.logger = logger.to_string();
        }
        self.snoozed.remove(logger);
    }

    /// Takes kart `kart`'s MyChron off it (its sessions so far stay the kart's).
    pub fn unmount(&mut self, kart: u32, newest: &dyn Fn(&str) -> String) {
        if let Some(k) = self.karts.iter_mut().find(|k| k.id == kart) {
            let until = newest(&k.logger);
            k.unmount(until);
        }
    }

    /// Takes a MyChron out of the Garage: off its kart, whose sessions so
    /// far it keeps. If it records more, it's asked about again.
    pub fn remove_logger(&mut self, logger: &str, newest: &dyn Fn(&str) -> String) {
        let until = newest(logger);
        for k in self.karts.iter_mut().filter(|k| k.logger == logger) {
            k.unmount(until.clone());
        }
    }

    /// Takes a kart out of the Garage; its sessions keep it.
    pub fn remove_kart(&mut self, kart: u32) {
        if let Some(k) = self.karts.iter_mut().find(|k| k.id == kart) {
            k.removed = true;
        }
    }

    /// Deletes a driver and their karts (their sessions are deleted by the caller).
    pub fn delete_driver(&mut self, driver: u32) {
        self.drivers.retain(|d| d.id != driver);
        self.karts.retain(|k| k.driver != Some(driver));
        let known: Vec<String> = self.karts.iter().flat_map(|k| k.all_loggers().cloned()).collect();
        self.loggers.retain(|l, _| known.contains(l));
        self.snoozed.retain(|l, _| known.contains(l));
    }

    /// A logger's names and download defaults, blank where unknown.
    pub fn for_logger(&self, logger: &str) -> LoggerProfile {
        let defaults = self.loggers.get(logger).cloned().unwrap_or_default();
        let kart = self.kart_on(logger);
        LoggerProfile {
            driver: kart.and_then(|k| self.driver_name(k.driver)).unwrap_or_default().to_string(),
            kart: kart.map(|k| k.name.clone()).unwrap_or_default(),
            engine: kart.map(|k| k.engine.clone()).unwrap_or_default(),
            championship: defaults.championship,
            track: defaults.track,
            model: defaults.model,
        }
    }

    /// Remembers what a download was entered as. The logger is now on the kart
    /// named (added if new, with the driver as its usual driver); a kart that
    /// had this logger before no longer does.
    pub fn note_download(&mut self, logger: &str, driver: &str, kart: &str, championship: &str, track: Option<&str>, newest: &dyn Fn(&str) -> String) {
        let entry = self.loggers.entry(logger.to_string()).or_default();
        entry.championship = championship.to_string();
        if let Some(t) = track {
            entry.track = t.to_string();
        }
        let driver = (!driver.trim().is_empty()).then(|| self.driver_named(driver));
        let kart = kart.trim();
        if kart.is_empty() {
            return;
        }
        let k = match self.karts.iter().position(|k| k.name.eq_ignore_ascii_case(kart)) {
            Some(k) => k,
            None => {
                let id = self.next_kart_id();
                self.karts.push(Kart { id, name: kart.into(), driver, ..Default::default() });
                self.karts.len() - 1
            }
        };
        let id = self.karts[k].id;
        self.mount(logger, id, newest);
        let k = &mut self.karts[k];
        if k.driver.is_none() {
            k.driver = driver;
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else { return Ok(()) };
        crate::storage::save(&path, self)
    }

    /// A logger's names, if it's on a kart.
    pub fn get(&self, logger: &str) -> Option<LoggerProfile> {
        Some(self.for_logger(logger)).filter(|p| !p.driver.is_empty() || !p.kart.is_empty())
    }

    /// RaceStudio 3 files sessions as `data/<date>/<driver>/<track>/<driver>_<kart>_<track>_a_NNNN.xrk`
    /// and writes the names into each `.xrk`; read one per driver to learn their logger.
    fn from_racestudio() -> Self {
        let mut out = Profiles::default();
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return out };
        let data = home.join(".rs3-wine/drive_c/AIM_SPORT/RaceStudio3/user/data");
        let Ok(days) = std::fs::read_dir(data) else { return out };
        for day in days.flatten() {
            let Ok(drivers) = std::fs::read_dir(day.path()) else { continue };
            for driver in drivers.flatten() {
                let Some(xrk) = find_xrk(&driver.path()) else { continue };
                let Ok(session) = aim_xrk::Session::open(&xrk) else { continue };
                let (Some(id), Some(name)) = (session.info.logger_id, session.info.driver.clone()) else { continue };
                out.loggers.entry(id.to_string()).or_insert(LoggerProfile {
                    driver: name,
                    kart: session.info.vehicle.clone().unwrap_or_default(),
                    ..Default::default()
                });
            }
        }
        out
    }
}

fn find_xrk(dir: &std::path::Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(found) = find_xrk(&p) {
                return Some(found);
            }
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xrk")) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_files_move_to_drivers_and_karts() {
        let mut p = Profiles::default();
        p.loggers.insert(
            "1".into(),
            LoggerProfile { driver: "Ann".into(), kart: "Cadet".into(), engine: "IAME Mini Swift".into(), championship: "Club".into(), track: "Club Circuit".into(), ..Default::default() },
        );
        let mut baselines = BTreeMap::new();
        baselines.insert("1".to_string(), crate::setup::Setup { driven_teeth: "80".into(), ..Default::default() });
        assert!(p.migrate(&baselines));
        assert!(!p.migrate(&baselines));
        assert_eq!(p.drivers, [Driver { id: 1, name: "Ann".into(), ..Default::default() }]);
        assert_eq!(p.karts.len(), 1);
        assert_eq!(p.karts[0].baseline.driven_teeth, "80");
        let lp = p.get("1").unwrap();
        assert_eq!((lp.driver.as_str(), lp.kart.as_str(), lp.engine.as_str(), lp.championship.as_str()), ("Ann", "Cadet", "IAME Mini Swift", "Club"));
    }

    #[test]
    fn every_listed_engine_has_a_picture() {
        for (_, engines) in ENGINES {
            for e in engines {
                assert!(engine_picture(e).is_some(), "{e}");
            }
        }
    }

    #[test]
    fn bundled_engine_pictures() {
        let uri = |e: &str| engine_picture(e).map(|(u, _)| u);
        assert_eq!(uri("Rotax 125 Micro MAX"), uri("Rotax 125 Mini MAX"));
        assert!(uri("Rotax 125 Mini MAX").is_some_and(|u| u.ends_with("rotax-mini-max.png")));
        assert_eq!(uri("Rotax 125 Junior MAX"), uri("Rotax 125 Senior MAX"));
        assert!(uri("Rotax 125 Senior MAX").is_some_and(|u| u.ends_with("rotax-max.png")));
        assert_eq!(uri("IAME X30 Mini"), uri("IAME X30 Senior"));
        assert!(uri("IAME X30 Junior").is_some_and(|u| u.ends_with("iame-x30.png")));
        assert!(uri("IAME Mini Swift").is_some());
        assert_eq!(uri("IAME Micro Swift"), uri("IAME Mini Swift"));
        assert!(uri("IAME KA100").is_some_and(|u| u.ends_with("iame-ka100.png")));
        assert!(uri("Vortex Mini ROK").is_some_and(|u| u.ends_with("vortex-mini-rok.png")));
        assert!(uri("Vortex ROK VLR").is_some_and(|u| u.ends_with("vortex-rok-vlr.png")));
        for (engine, file) in [("Vortex Shifter ROK", "vortex-shifter-rok.png"), ("Tillotson T225RS", "tillotson-t225rs.png"), ("Predator Ghost 212", "predator-ghost-212.png"), ("Predator 79", "predator-79.png"), ("Briggs & Stratton LO206", "briggs-lo206.png"), ("Vortex ROK GP", "vortex-rok-gp.png"), ("Comer C50", "comer-c50.png"), ("Comer C51", "comer-c51.png"), ("Comer C52", "comer-c52.png"), ("Honda GX200", "honda-gx200.png")] {
            assert!(uri(engine).is_some_and(|u| u.ends_with(file)), "{engine}");
        }
        assert!(uri("IAME X30 Super Shifter 175").is_some_and(|u| u.ends_with("iame-super-shifter-175.png")));
    }

    #[test]
    fn ages_from_birthdays() {
        let d = |born: &str| Driver { born: born.into(), ..Default::default() };
        let today = aim_xrk::summary::Date { year: 2026, month: 10, day: 12 };
        assert_eq!(d("2017-03-14").age(today), Some(9));
        assert_eq!(d("10/13/2016").age(today), Some(9));
        assert_eq!(d("10/12/2016").age(today), Some(10));
        assert_eq!(d("sometime").age(today), None);
        assert_eq!(d("13/1/2016").age(today), None);
    }

    #[test]
    fn a_download_moves_the_mychron_to_the_kart_named() {
        let mut p = Profiles::default();
        p.note_download("1", "Ann", "Cadet", "Club", Some("Club Circuit"), &|_| String::new());
        assert_eq!(p.kart_on("1").map(|k| k.name.as_str()), Some("Cadet"));
        // Moved to a new kart, driven this time by someone else: the kart keeps no logger,
        // the new kart gets it, and the usual driver of an existing kart isn't changed.
        p.note_download("1", "Bob", "Spare", "Club", None, &|_| String::new());
        assert_eq!(p.karts[0].logger, "");
        assert_eq!(p.kart_on("1").map(|k| k.name.as_str()), Some("Spare"));
        p.note_download("1", "Bob", "cadet", "Club", None, &|_| String::new());
        assert_eq!(p.kart_on("1").and_then(|k| p.driver_name(k.driver)), Some("Ann"));
        assert_eq!(p.loggers["1"].track, "Club Circuit");
        assert_eq!(p.drivers.len(), 2);
    }

    fn two_drivers() -> Profiles {
        let mut p = Profiles::default();
        let (a, b) = (p.driver_named("Ann"), p.driver_named("Bo"));
        p.karts.push(Kart { id: 1, name: "Ann's kart".into(), driver: Some(a), logger: "5555".into(), ..Default::default() });
        p.karts.push(Kart { id: 2, name: "Bo's kart".into(), driver: Some(b), logger: "4444".into(), ..Default::default() });
        p
    }

    /// The newest session start each MyChron had downloaded "so far".
    fn newest(at: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> String {
        move |l: &str| at.iter().find(|x| x.0 == l).map(|x| x.1.to_string()).unwrap_or_default()
    }

    #[test]
    fn a_handed_down_mychron_splits_its_sessions_by_time() {
        // Ann's MyChron 5 (5555) goes to Bo; Ann gets a 6 (6666), Bo's 4 (4444) retires.
        let mut p = two_drivers();
        let so_far = newest(&[("5555", "2026-12-20 10:00:00"), ("4444", "2026-12-21 09:00:00")]);
        p.mount("6666", 1, &so_far);
        assert_eq!(p.karts[0].past, [PastMyChron { logger: "5555".into(), until: "2026-12-20 10:00:00".into() }]);
        // Bo's race with the formatted 5 (before anything was set up) has no kart yet.
        assert!(p.kart_for("5555", "2026-12-27 11:00:00").is_none());
        p.mount("5555", 2, &so_far);
        // Ann's old sessions stay Ann's; the new ones are Bo's; Bo's MyChron 4 stays Bo's.
        assert_eq!(p.kart_for("5555", "2026-12-20 10:00:00").map(|k| k.id), Some(1));
        assert_eq!(p.kart_for("5555", "2026-12-27 11:00:00").map(|k| k.id), Some(2));
        assert_eq!(p.kart_for("4444", "2026-12-21 09:00:00").map(|k| k.id), Some(2));
        assert_eq!(p.kart_for("6666", "2026-12-28 09:00:00").map(|k| k.id), Some(1));
    }

    #[test]
    fn an_old_mychron_on_a_new_kart_of_the_same_driver() {
        let mut p = two_drivers();
        let so_far = newest(&[("5555", "2026-12-20 10:00:00")]);
        p.mount("6666", 1, &so_far);
        let ann = p.drivers[0].id;
        p.karts.push(Kart { id: 3, name: "Ann's new kart".into(), driver: Some(ann), ..Default::default() });
        p.mount("5555", 3, &so_far);
        assert_eq!(p.kart_for("5555", "2026-12-01 10:00:00").map(|k| k.id), Some(1));
        assert_eq!(p.kart_for("5555", "2027-01-05 10:00:00").map(|k| k.id), Some(3));
    }

    #[test]
    fn removing_hides_but_keeps_the_links() {
        let mut p = two_drivers();
        p.remove_kart(1);
        assert_eq!(p.garage_karts().count(), 1);
        assert_eq!(p.kart_for("5555", "2026-10-01 10:00:00").map(|k| k.id), Some(1));
        p.remove_logger("4444", &newest(&[("4444", "2026-10-11 02:21:31")]));
        assert!(p.mounted_on("4444").is_none());
        assert_eq!(p.kart_for("4444", "2026-10-11 02:21:31").map(|k| k.id), Some(2));
        // Anything it records later is asked about.
        assert!(p.kart_for("4444", "2026-11-01 10:00:00").is_none());
        assert!(p.knows_logger("4444"));
    }

    #[test]
    fn deleting_a_driver_takes_their_karts() {
        let mut p = two_drivers();
        let ann = p.drivers[0].id;
        p.delete_driver(ann);
        assert_eq!(p.drivers.len(), 1);
        assert_eq!(p.karts.len(), 1);
        assert_eq!(p.for_logger("4444").driver, "Bo");
    }
}
