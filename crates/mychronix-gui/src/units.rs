//! Display units: mph and °F, or km/h and °C. The MyChron records metric;
//! values are converted when shown. Chosen in `~/Mychronix/settings.json`
//! (`"units": "imperial"` or `"metric"`), else from the Mac's measurement
//! setting or the system's country.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use aim_xrk::Channel;

static IMPERIAL: AtomicBool = AtomicBool::new(false);

const MPH_PER_KMH: f64 = 0.621_371;

/// Reads the setting (or the locale) once at startup.
pub fn init() {
    let setting = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Mychronix").join("settings.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("units").and_then(|u| u.as_str()).map(str::to_owned));
    let imperial = match setting.as_deref() {
        Some("imperial") => true,
        Some("metric") => false,
        // The countries that use miles per hour and Fahrenheit day to day. The
        // measurement locale wins when set (e.g. US English with metric units).
        _ => mac_imperial().unwrap_or_else(|| matches!(measurement_country().or_else(crate::countries::system_country).as_deref(), Some("US") | Some("LR") | Some("MM"))),
    };
    IMPERIAL.store(imperial, Ordering::Relaxed);
}

/// A Mac's own measurement setting (System Settings → General → Language &
/// Region), when it has one: metric unless set to US units.
fn mac_imperial() -> Option<bool> {
    crate::countries::mac_default("AppleMetricUnits").map(|v| v == "0")
}

/// The region of `LC_MEASUREMENT`, e.g. `en_DK.UTF-8` → `DK`.
fn measurement_country() -> Option<String> {
    let value = std::env::var("LC_MEASUREMENT").ok()?;
    let region = value.split(['.', '@']).next()?.split('_').nth(1)?;
    (region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic())).then(|| region.to_ascii_uppercase())
}

pub fn imperial() -> bool {
    IMPERIAL.load(Ordering::Relaxed)
}

pub fn speed(kmh: f64) -> f64 {
    if imperial() { kmh * MPH_PER_KMH } else { kmh }
}

pub fn speed_unit() -> &'static str {
    if imperial() { "mph" } else { "km/h" }
}

pub fn temp(celsius: f64) -> f64 {
    if imperial() { celsius * 9.0 / 5.0 + 32.0 } else { celsius }
}

pub fn temp_unit() -> &'static str {
    if imperial() { "°F" } else { "°C" }
}

/// Converts a channel's samples and units for display (speeds and temperatures).
pub fn convert_channel(c: &mut Channel) {
    if !imperial() {
        return;
    }
    match c.units.trim() {
        "km/h" | "kmh" | "kph" => {
            for v in &mut c.values {
                *v *= MPH_PER_KMH;
            }
            c.units = "mph".into();
        }
        "C" | "°C" | "degC" => {
            for v in &mut c.values {
                *v = *v * 9.0 / 5.0 + 32.0;
            }
            c.units = "F".into();
        }
        _ => {}
    }
}
