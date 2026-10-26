//! Preferences, in `~/Mychronix/settings.json`: changed on the Settings page,
//! or by hand. A missing file or setting means the default; settings this
//! version doesn't know are kept when it saves. For example:
//!
//! ```json
//! { "units": "metric", "clock": "24", "weather": false, "mcp": true }
//! ```

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "imperial" or "metric"; otherwise from the computer's region.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// "12" or "24"; otherwise from the computer's region.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
    /// Fetch each session's weather from Open-Meteo.
    pub weather: bool,
    /// Check GitHub once a day for a newer Mychronix.
    pub update_check: bool,
    /// Download AiM's track database from AiM's server.
    pub aim_tracks: bool,
    /// Mychronix Performance Engineer: answer an AI assistant on this
    /// computer (`mychronix --mcp`). Off until turned on.
    pub mcp: bool,
    /// The real width in meters of tracks the Map view draws, by track name
    /// (`{ "Lonato": 9 }`); other tracks get a typical width for their type.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub track_widths: std::collections::BTreeMap<String, f64>,
    /// Settings this version doesn't know, kept as they were.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { units: None, clock: None, weather: true, update_check: true, aim_tracks: true, mcp: false, track_widths: Default::default(), other: Default::default() }
    }
}

static SETTINGS: RwLock<Option<Settings>> = RwLock::new(None);
static CLOCK_24: AtomicBool = AtomicBool::new(false);

/// The current settings (read from the file the first time).
pub fn get() -> Settings {
    if let Some(s) = SETTINGS.read().ok().and_then(|g| g.clone()) {
        return s;
    }
    let s = load();
    apply(&s);
    if let Ok(mut g) = SETTINGS.write() {
        *g = Some(s.clone());
    }
    s
}

/// Changes the settings, saves them, and puts them into effect.
pub fn change(f: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
    let mut s = get();
    f(&mut s);
    apply(&s);
    if let Ok(mut g) = SETTINGS.write() {
        *g = Some(s.clone());
    }
    let path = crate::paths::data("settings.json").ok_or_else(|| std::io::Error::other("no home folder"))?;
    log::info!("settings changed: {s:?}");
    crate::storage::save(&path, &s)
}

/// Times as "14:19" rather than "2:19 PM".
pub fn clock_24() -> bool {
    CLOCK_24.load(Ordering::Relaxed)
}

/// Puts settings that are kept outside this module into effect.
fn apply(s: &Settings) {
    let h24 = match s.clock.as_deref() {
        Some("24") => true,
        Some("12") => false,
        // 12-hour where it's the everyday clock, 24-hour elsewhere.
        _ => !matches!(crate::countries::system_country().as_deref(), Some("US") | Some("CA") | Some("AU") | Some("NZ") | Some("PH") | Some("IN") | Some("EG") | Some("SA")),
    };
    CLOCK_24.store(h24, Ordering::Relaxed);
    crate::units::set(s.units.as_deref());
}

fn load() -> Settings {
    let Some(path) = crate::paths::data("settings.json") else { return Settings::default() };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(e) => {
            log::warn!("couldn't read {}: {e}; using the defaults", path.display());
            return Settings::default();
        }
    };
    // Kept in place if it doesn't parse: it's yours to fix, not damaged data.
    parse(&text).unwrap_or_else(|e| {
        log::warn!("{} has a mistake ({e}); using the defaults", path.display());
        Settings::default()
    })
}

fn parse(text: &str) -> Result<Settings, serde_json::Error> {
    let s: Settings = serde_json::from_str(text)?;
    log::info!("settings: {s:?}");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_keep_their_defaults() {
        let s = parse(r#"{ "weather": false, "something_else": 1 }"#).unwrap();
        assert!(!s.weather && s.update_check && s.aim_tracks && !s.mcp && s.units.is_none());
        let s = parse(r#"{ "units": "metric" }"#).unwrap();
        assert_eq!(s.units.as_deref(), Some("metric"));
        assert!(parse(r#"{ "weather": "no" }"#).is_err());
    }

    #[test]
    fn unknown_settings_survive_a_save() {
        let s = parse(r#"{ "weather": false, "maps": false }"#).unwrap();
        let back = serde_json::to_string(&s).unwrap();
        assert!(back.contains(r#""maps":false"#) && back.contains(r#""weather":false"#), "{back}");
    }
}
