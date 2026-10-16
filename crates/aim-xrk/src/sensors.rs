//! How an AiM sensor's raw u16 samples become values: RaceStudio 2's sensor
//! table (`sensor_table.rs`), with the few sensors it calibrates per channel
//! or that read better in other units.

use crate::sensor_table::SENSORS;

/// A sensor's scaling: `a1 * raw + a0`, raw read as signed when `signed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub units: &'static str,
    pub decimals: u8,
    a1: f64,
    a0: f64,
    signed: bool,
}

impl Scale {
    pub fn apply(&self, raw: u16) -> f64 {
        let r = if self.signed { raw as i16 as f64 } else { raw as f64 };
        self.a1 * r + self.a0
    }
}

/// Decimals that show one raw step.
fn decimals(a1: f64) -> u8 {
    let a = a1.abs();
    if a == 0.0 || a >= 1.0 { 0 } else { (-a.log10()).ceil().clamp(0.0, 4.0) as u8 }
}

fn linear(units: &'static str, a1: f64, a0: f64, signed: bool) -> Scale {
    Scale { units, decimals: decimals(a1), a1, a0, signed }
}

/// The scaling for sensor `code`. `calibration` is the channel's own pair
/// of floats: (zero, gain) for accelerometers, (min, max) for imported data. None
/// when the sensor needs something this doesn't know (custom sensors, lambda
/// curves): the raw counts are then shown as they are.
pub fn scale(code: u16, calibration: Option<(f64, f64)>) -> Option<Scale> {
    const G: f64 = 9.80665;
    let deg = 180.0 / std::f64::consts::PI;
    let &(_, kind, a1, a0, units) = SENSORS.iter().find(|s| s.0 == code)?;
    Some(match code {
        // Thermocouples and on-board accelerometers can read below zero.
        96 | 405 | 406 | 413 | 414 => linear(units, a1, a0, true),
        // A GPS module's accelerations in g, angles in degrees.
        3003 | 3004 => Scale { decimals: 2, ..linear("g", a1 / G, a0 / G, false) },
        3005 | 3006 => Scale { decimals: 1, ..linear("deg", a1 * deg, a0 * deg, false) },
        3007 => Scale { decimals: 1, ..linear("deg/s", a1 * deg, a0 * deg, false) },
        // Analog accelerometers, calibrated per channel: zero in counts, and a
        // gain where 40 g spans `gain` counts (fitted against GPS g-forces).
        10 | 11 | 78 | 79 | 198 | 401..=404 => {
            let (zero, gain) = calibration?;
            // Without a gain, AiM Solo DL files read 440 counts a g (Lapline's
            // finding), signed like the gain.
            let k = if gain.abs() > 1.0 { 40.0 / gain } else if gain < 0.0 { -1.0 / 440.0 } else { 1.0 / 440.0 };
            Scale { decimals: 2, ..linear("g", k, -k * zero, false) }
        }
        // Imported data: each channel's raw range spans its own min..max
        // (the calibration slots hold them).
        999 => {
            let (lo, hi) = calibration.filter(|c| c.1 > c.0)?;
            let a1 = (hi - lo) / 65535.0;
            Scale { decimals: decimals(a1).min(3), ..linear("", a1, lo, false) }
        }
        // Linear potentiometers: calibrated per channel in a way not worked out yet.
        7 | 8 | 30 => return None,
        _ => match kind {
            1 | 2 => linear(units, a1, a0, false),
            // Fixed-scale sensors (the battery); the rest are calibrated per channel.
            0 if a1 != 0.0 && a1 < 1.0 && a0 == 0.0 => linear(units, a1, 0.0, false),
            _ => return None,
        },
    })
}

/// Samples per second for an AiM frequency code.
pub fn rate_hz(code: u8) -> Option<u32> {
    Some(match code {
        0 => 1,
        1 => 2,
        2 => 10,
        3 => 20,
        4 => 50,
        5 => 100,
        6 => 200,
        7 => 500,
        8 => 1000,
        9 => 5,
        11 => 4,
        12 => 8,
        _ => return None,
    })
}

/// Words from 0xCFF0 to 0xCFFF are a sensor's error codes ("not connected",
/// "open circuit"), not readings.
pub fn is_error(raw: u16) -> bool {
    (0xCFF0..=0xCFFF).contains(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_and_overrides() {
        let s = |c, cal| scale(c, cal).unwrap();
        assert_eq!(s(0, None).apply(8123), 8123.0);
        assert!((s(45, None).apply(2425) - 22.87).abs() < 0.01); // logger temperature
        assert!((s(68, None).apply(1495) - 8.212).abs() < 0.001); // battery
        assert!((s(96, None).apply((-25i16) as u16) + 2.5).abs() < 1e-6); // thermocouple below zero
        assert!((s(158, None).apply(150) - 84.1).abs() < 0.1); // ECU temperature
        assert!((s(3006, None).apply(30954) - 174.73).abs() < 0.05); // GPS heading
        assert!(s(3003, None).apply(16000).abs() < 1e-5); // GPS lateral g
        assert!((s(10, Some((1751.0, 19174.0))).apply(1751 + 479) - 1.0).abs() < 0.01);
        assert!(scale(10, None).is_none()); // uncalibrated accelerometer
        assert!(scale(86, None).is_none()); // custom sensor
        assert_eq!(s(1, None).units, "km/h");
        assert!((s(999, Some((0.0, 9795.0))).apply(65535) - 9795.0).abs() < 1e-6);
        assert!(scale(30, None).is_none()); // damper potentiometer
    }
}
