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
                    clutch_rpm: String::new(),
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

    /// The kart a logger is mounted on.
    pub fn kart_on(&self, logger: &str) -> Option<&Kart> {
        (!logger.is_empty()).then(|| self.karts.iter().find(|k| k.logger == logger)).flatten()
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
    pub fn note_download(&mut self, logger: &str, driver: &str, kart: &str, championship: &str, track: Option<&str>) {
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
        for (j, other) in self.karts.iter_mut().enumerate() {
            if j != k && other.logger == logger {
                other.logger.clear();
            }
        }
        let k = &mut self.karts[k];
        k.logger = logger.to_string();
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
        p.note_download("1", "Ann", "Cadet", "Club", Some("Club Circuit"));
        assert_eq!(p.kart_on("1").map(|k| k.name.as_str()), Some("Cadet"));
        // Moved to a new kart, driven this time by someone else: the kart keeps no logger,
        // the new kart gets it, and the usual driver of an existing kart isn't changed.
        p.note_download("1", "Bob", "Spare", "Club", None);
        assert_eq!(p.karts[0].logger, "");
        assert_eq!(p.kart_on("1").map(|k| k.name.as_str()), Some("Spare"));
        p.note_download("1", "Bob", "cadet", "Club", None);
        assert_eq!(p.kart_on("1").and_then(|k| p.driver_name(k.driver)), Some("Ann"));
        assert_eq!(p.loggers["1"].track, "Club Circuit");
        assert_eq!(p.drivers.len(), 2);
    }
}
