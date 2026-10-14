//! Session notes: patterns across a session's racing laps worth pointing out,
//! measured rather than guessed. Racing laps are the complete laps no longer
//! than 115% of the median (out/in laps and laps behind a flag left out).

use crate::{Lap, Session};
use crate::checks::{Sensor, sensor_of};

/// One note.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// Lap times rose (positive) or fell steadily, seconds per lap.
    LapTrend { per_lap_s: f64, first_lap: i32, last_lap: i32 },
    /// Standard deviation of the racing laps, seconds.
    Consistency { sd_s: f64, laps: usize },
    /// A temperature's peak per lap rose or fell steadily, in the channel's own
    /// unit (`units`: the logger records °C; an app may have converted to °F).
    TempTrend { channel: String, units: String, from: f64, to: f64, per_lap: f64 },
}

/// The racing laps of a session.
pub fn racing_laps(session: &Session) -> Vec<Lap> {
    let laps = session.complete_laps();
    let mut d: Vec<i64> = laps.iter().map(|l| l.duration_ms()).filter(|&d| d >= 10_000).collect();
    d.sort_unstable();
    let Some(&median) = d.get(d.len() / 2) else { return Vec::new() };
    laps.iter().filter(|l| l.duration_ms() >= 10_000 && l.duration_ms() as f64 <= median as f64 * 1.15).copied().collect()
}

pub fn session_notes(session: &Session) -> Vec<Note> {
    let laps = racing_laps(session);
    let mut notes = Vec::new();
    if laps.len() >= 4 {
        let t: Vec<f64> = laps.iter().map(|l| l.duration_ms() as f64 / 1000.0).collect();
        let mean = t.iter().sum::<f64>() / t.len() as f64;
        let sd = (t.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / t.len() as f64).sqrt();
        notes.push(Note::Consistency { sd_s: sd, laps: t.len() });
        if laps.len() >= 5 {
            let x: Vec<f64> = (0..t.len()).map(|i| i as f64).collect();
            let (slope, r) = fit(&x, &t);
            if slope.abs() >= 0.1 && r.abs() >= 0.5 {
                notes.push(Note::LapTrend { per_lap_s: slope, first_lap: laps[0].number, last_lap: laps[laps.len() - 1].number });
            }
        }
    }
    if laps.len() >= 5 {
        for c in &session.channels {
            if sensor_of(&c.name) != Some(Sensor::Temperature) || c.values.len() < 2 {
                continue;
            }
            let peaks: Vec<f64> = laps
                .iter()
                .filter_map(|l| {
                    let lo = c.times_ms.partition_point(|&t| t < l.start_ms);
                    let hi = c.times_ms.partition_point(|&t| t <= l.end_ms);
                    c.values[lo..hi].iter().copied().reduce(f64::max)
                })
                .collect();
            if peaks.len() != laps.len() {
                continue;
            }
            let x: Vec<f64> = (0..peaks.len()).map(|i| i as f64).collect();
            let (slope, r) = fit(&x, &peaks);
            let span = slope * (peaks.len() - 1) as f64;
            // Thresholds are 1.5 °C a lap and 8 °C overall; the same in °F.
            let k = if c.units.to_uppercase().contains('F') { 1.8 } else { 1.0 };
            if slope.abs() >= 1.5 * k && r.abs() >= 0.7 && span.abs() >= 8.0 * k {
                let mean = peaks.iter().sum::<f64>() / peaks.len() as f64;
                let mid = (peaks.len() - 1) as f64 / 2.0;
                notes.push(Note::TempTrend { channel: c.name.clone(), units: c.units.clone(), from: mean - slope * mid, to: mean + slope * mid, per_lap: slope });
            }
        }
    }
    notes
}

/// Least-squares slope and correlation of `ys` over `xs`.
fn fit(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let syy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    if sxx == 0.0 {
        return (0.0, 0.0);
    }
    (sxy / sxx, if syy > 0.0 { sxy / (sxx * syy).sqrt() } else { 0.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Channel, SessionInfo};

    /// Laps of the given lengths (s) back to back, with an out and in lap around them.
    fn session(lap_s: &[f64], channels: Vec<Channel>) -> Session {
        let mut laps = vec![Lap { number: 0, start_ms: 0, end_ms: 60_000 }];
        let mut t = 60_000;
        for (i, s) in lap_s.iter().enumerate() {
            let end = t + (s * 1000.0) as i64;
            laps.push(Lap { number: i as i32 + 1, start_ms: t, end_ms: end });
            t = end;
        }
        laps.push(Lap { number: lap_s.len() as i32 + 1, start_ms: t, end_ms: t + 60_000 });
        Session { info: SessionInfo::default(), channels, laps, gps: None }
    }

    #[test]
    fn lap_trend_and_consistency() {
        let s = session(&[31.0, 31.3, 31.6, 31.9, 32.2, 32.5], vec![]);
        let notes = session_notes(&s);
        assert!(notes.iter().any(|n| matches!(n, Note::LapTrend { per_lap_s, .. } if (per_lap_s - 0.3).abs() < 1e-6)));
        assert!(notes.iter().any(|n| matches!(n, Note::Consistency { laps: 6, .. })));
        // Scattered laps: no trend.
        let s = session(&[31.0, 32.0, 31.0, 32.0, 31.0, 32.0], vec![]);
        assert!(!session_notes(&s).iter().any(|n| matches!(n, Note::LapTrend { .. })));
    }

    #[test]
    fn rising_water_temperature() {
        // Water peaks 40, 44, 48 … one per 30 s lap.
        let times: Vec<i64> = (0..400).map(|k| 60_000 + k * 500).collect();
        let values: Vec<f64> = times.iter().map(|&t| 40.0 + ((t - 60_000) / 30_000) as f64 * 4.0).collect();
        let water = Channel { name: "Water Temp".into(), group: None, units: "C".into(), decimals: 1, interpolate: true, times_ms: times, values };
        let s = session(&[30.0; 6], vec![water]);
        let note = session_notes(&s).into_iter().find(|n| matches!(n, Note::TempTrend { .. })).unwrap();
        let Note::TempTrend { per_lap, from, to, .. } = note else { unreachable!() };
        assert!((per_lap - 4.0).abs() < 0.5, "{per_lap}");
        assert!(to > from + 15.0);
    }
}
