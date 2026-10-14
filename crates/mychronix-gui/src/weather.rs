//! The weather at each session, for setup sheets and jetting: air temperature,
//! humidity, pressure and dew point at the track for the hour the session
//! started, from Open-Meteo (free, no account; its forecast service covers
//! the last three months, its archive anything older). Saved beside the
//! session as `a_NNNN.weather.json` so it's fetched once.
//!
//! Only the track's position and the date are sent. Turn it off with
//! `"weather": false` in `~/Mychronix/settings.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub air_c: f64,
    pub humidity_pct: f64,
    /// At the surface (what the engine breathes), hPa.
    pub pressure_hpa: f64,
    pub dew_point_c: f64,
}

impl Weather {
    pub fn path_for(session: &Path) -> PathBuf {
        session.with_extension("weather.json")
    }

    pub fn load(session: &Path) -> Option<Self> {
        let p = Self::path_for(session);
        p.exists().then(|| crate::storage::load(&p)).flatten()
    }

    /// "64°F · 55% humidity · 1015 hPa".
    pub fn summary(&self) -> String {
        format!("{:.0}{} · {:.0}% humidity · {:.0} hPa", crate::units::temp(self.air_c), crate::units::temp_unit(), self.humidity_pct, self.pressure_hpa)
    }
}

/// Whether fetching is allowed (`"weather": false` in settings.json turns it off).
pub fn enabled() -> bool {
    let setting = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Mychronix").join("settings.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("weather").and_then(|w| w.as_bool()));
    setting != Some(false)
}

/// What a fetch needs from a session.
#[derive(Debug, Clone)]
pub struct Want {
    pub session: PathBuf,
    pub lat: f64,
    pub lon: f64,
    /// "2026-10-10".
    pub date: String,
    /// Local hour the session started, 0–23.
    pub hour: usize,
}

/// Fetches the weather for `wants` in the background (one request per track
/// and day), saving each beside its session; sends (session, weather) as each
/// arrives. Failures (offline) are logged and skipped.
pub fn spawn(wants: Vec<Want>, ctx: eframe::egui::Context) -> std::sync::mpsc::Receiver<(PathBuf, Weather)> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut by_day: BTreeMap<(String, i64, i64), Vec<Want>> = BTreeMap::new();
        for w in wants {
            // Same track (to about 1 km) and day: one request.
            by_day.entry((w.date.clone(), (w.lat * 100.0).round() as i64, (w.lon * 100.0).round() as i64)).or_default().push(w);
        }
        for ((date, ..), group) in by_day {
            let (lat, lon) = (group[0].lat, group[0].lon);
            match fetch_day(lat, lon, &date) {
                Ok(hours) => {
                    for w in group {
                        let Some(weather) = hours.get(w.hour).copied().flatten() else { continue };
                        if let Err(e) = crate::storage::save(&Weather::path_for(&w.session), &weather) {
                            log::warn!("couldn't save weather for {}: {e}", w.session.display());
                            continue;
                        }
                        if tx.send((w.session, weather)).is_err() {
                            return;
                        }
                    }
                    ctx.request_repaint();
                }
                Err(e) => log::warn!("weather for {date} at {lat:.3},{lon:.3}: {e}"),
            }
        }
    });
    rx
}

/// The 24 hourly readings (local time) for a day at a place.
fn fetch_day(lat: f64, lon: f64, date: &str) -> Result<Vec<Option<Weather>>, String> {
    let vars = "temperature_2m,relative_humidity_2m,surface_pressure,dew_point_2m";
    let query = format!("latitude={lat:.4}&longitude={lon:.4}&start_date={date}&end_date={date}&hourly={vars}&timezone=auto");
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mychronix (AiM MyChron data analysis)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    // The forecast service covers about the last three months; the archive the rest.
    let mut last_err = String::new();
    for base in ["https://api.open-meteo.com/v1/forecast", "https://archive-api.open-meteo.com/v1/archive"] {
        let url = format!("{base}?{query}");
        log::debug!("weather: {url}");
        match client.get(&url).send().and_then(|r| r.error_for_status()).and_then(|r| r.text()) {
            Ok(text) => {
                if let Some(hours) = parse(&text).filter(|h| h.iter().any(Option::is_some)) {
                    return Ok(hours);
                }
                last_err = "no readings for that day".into();
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

/// Open-Meteo's hourly JSON → one reading per hour.
fn parse(text: &str) -> Option<Vec<Option<Weather>>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let h = v.get("hourly")?;
    let series = |k: &str| -> Option<Vec<Option<f64>>> { Some(h.get(k)?.as_array()?.iter().map(|x| x.as_f64()).collect()) };
    let (t, rh, p, dp) = (series("temperature_2m")?, series("relative_humidity_2m")?, series("surface_pressure")?, series("dew_point_2m")?);
    Some(
        (0..t.len())
            .map(|i| Some(Weather { air_c: t[i]?, humidity_pct: rh.get(i).copied()??, pressure_hpa: p.get(i).copied()??, dew_point_c: dp.get(i).copied()?? }))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hourly_readings() {
        let text = r#"{"hourly":{"time":["2026-10-10T00:00","2026-10-10T01:00"],"temperature_2m":[12.5,null],"relative_humidity_2m":[80,81],"surface_pressure":[1015.2,1015.0],"dew_point_2m":[9.1,9.0]}}"#;
        let h = parse(text).unwrap();
        assert_eq!(h[0], Some(Weather { air_c: 12.5, humidity_pct: 80.0, pressure_hpa: 1015.2, dew_point_c: 9.1 }));
        assert_eq!(h[1], None);
    }
}
