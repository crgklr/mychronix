//! Display units: mph and °F, or km/h and °C. The MyChron records metric;
//! values are converted when shown. Chosen on the Settings page (saved in
//! `~/Mychronix/settings.json`), else from the Mac's measurement setting or
//! the system's country.

use std::sync::atomic::{AtomicBool, Ordering};

use aim_xrk::Channel;

static IMPERIAL: AtomicBool = AtomicBool::new(false);

const MPH_PER_KMH: f64 = 0.621_371;

/// Reads the setting (or the locale) at startup.
pub fn init() {
    crate::settings::get();
}

/// Uses `units` ("imperial", "metric", or None for the computer's region).
pub fn set(units: Option<&str>) {
    let imperial = match units {
        Some("imperial") => true,
        Some("metric") => false,
        // The countries that use miles per hour and Fahrenheit day to day. The
        // measurement locale wins when set (e.g. US English with metric units).
        _ => mac_imperial().unwrap_or_else(|| {
            matches!(
                measurement_country()
                    .or_else(crate::countries::system_country)
                    .as_deref(),
                Some("US") | Some("LR") | Some("MM")
            )
        }),
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
    (region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| region.to_ascii_uppercase())
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

/// Short distances (braking zones): feet or meters.
pub fn distance(m: f64) -> f64 {
    if imperial() { m * 3.280_84 } else { m }
}

pub fn distance_unit() -> &'static str {
    if imperial() { "ft" } else { "m" }
}

pub fn temp(celsius: f64) -> f64 {
    if imperial() {
        celsius * 9.0 / 5.0 + 32.0
    } else {
        celsius
    }
}

pub fn temp_unit() -> &'static str {
    if imperial() { "°F" } else { "°C" }
}

/// How a channel's samples are shown. Sessions keep what the MyChron
/// recorded (km/h, °C, lap timing in ms) for every calculation; speeds and
/// temperatures follow Settings only when a number is drawn or printed, and
/// milliseconds show as seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown<'a> {
    scale: f64,
    offset: f64,
    unit: &'a str,
}

impl<'a> Shown<'a> {
    pub fn of(c: &'a Channel) -> Self {
        Self::in_units(c, imperial())
    }

    fn in_units(c: &'a Channel, imperial: bool) -> Self {
        let (scale, offset, unit) = match c.units.trim() {
            "km/h" | "kmh" | "kph" if imperial => (MPH_PER_KMH, 0.0, "mph"),
            "C" | "°C" | "degC" if imperial => (9.0 / 5.0, 32.0, "°F"),
            "C" | "°C" | "degC" => (1.0, 0.0, "°C"),
            "F" => (1.0, 0.0, "°F"),
            "deg" => (1.0, 0.0, "°"),
            "ms" => (0.001, 0.0, "s"),
            other => (1.0, 0.0, other),
        };
        Self {
            scale,
            offset,
            unit,
        }
    }

    pub fn value(&self, v: f64) -> f64 {
        v * self.scale + self.offset
    }

    pub fn unit(&self) -> &'a str {
        self.unit
    }

    /// Decimals to show: the channel's own, or milliseconds' as seconds.
    pub fn decimals(&self, c: &Channel) -> u8 {
        if self.scale == 0.001 { 3 } else { c.decimals }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(units: &str) -> Channel {
        Channel {
            name: String::new(),
            group: None,
            units: units.into(),
            decimals: 1,
            interpolate: true,
            times_ms: vec![],
            values: vec![],
        }
    }

    #[test]
    fn recorded_units_are_converted_only_for_showing() {
        let (kmh, c, ms, g) = (channel("km/h"), channel("C"), channel("ms"), channel("g"));
        let mph = Shown::in_units(&kmh, true);
        assert_eq!(mph.unit(), "mph");
        assert!((mph.value(100.0) - 62.1371).abs() < 1e-3);
        assert_eq!(Shown::in_units(&kmh, false).value(100.0), 100.0);
        assert_eq!(Shown::in_units(&c, true).value(100.0), 212.0);
        assert_eq!(Shown::in_units(&c, false).unit(), "°C");
        let s = Shown::in_units(&ms, false);
        assert_eq!((s.value(1500.0), s.unit(), s.decimals(&ms)), (1.5, "s", 3));
        assert_eq!(Shown::in_units(&g, true).value(-0.8), -0.8);
    }
}
