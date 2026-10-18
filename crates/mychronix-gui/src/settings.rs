//! Optional switches in `~/Mychronix/settings.json`, edited by hand
//! (Mychronix never writes it). Read once, at first use; a missing file or
//! setting means the default. For example:
//!
//! ```json
//! { "units": "metric", "weather": false, "update_check": false, "aim_tracks": false }
//! ```

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "imperial" or "metric"; otherwise from the computer's region.
    pub units: Option<String>,
    /// Fetch each session's weather from Open-Meteo.
    pub weather: bool,
    /// Check GitHub once a day for a newer Mychronix.
    pub update_check: bool,
    /// Download AiM's track database from AiM's server.
    pub aim_tracks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { units: None, weather: true, update_check: true, aim_tracks: true }
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

pub fn get() -> &'static Settings {
    SETTINGS.get_or_init(load)
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
        assert!(!s.weather && s.update_check && s.aim_tracks && s.units.is_none());
        let s = parse(r#"{ "units": "metric" }"#).unwrap();
        assert_eq!(s.units.as_deref(), Some("metric"));
        assert!(parse(r#"{ "weather": "no" }"#).is_err());
    }
}
