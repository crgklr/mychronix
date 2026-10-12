//! Who drives what: a driver and kart name for each logger, saved in
//! `~/Mychronix/profiles.json`. The logger's own files leave these blank.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LoggerProfile {
    pub driver: String,
    pub kart: String,
    /// Last championship entered when downloading from this logger.
    #[serde(default)]
    pub championship: String,
    /// Last track entered when downloading from this logger.
    #[serde(default)]
    pub track: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profiles {
    /// By logger id, e.g. "12345678".
    pub loggers: BTreeMap<String, LoggerProfile>,
}

impl Profiles {
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("profiles.json"))
    }

    /// Loads saved profiles; on first run, fills in what RaceStudio 3 already knows.
    pub fn load() -> Self {
        let saved = Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Profiles>(&s).ok());
        match saved {
            Some(p) => p,
            None => {
                let p = Self::from_racestudio();
                if !p.loggers.is_empty() {
                    let _ = p.save();
                }
                p
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    pub fn get(&self, logger: &str) -> Option<&LoggerProfile> {
        self.loggers.get(logger).filter(|p| !p.driver.is_empty() || !p.kart.is_empty())
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
