//! Lap comparison along distance.
//!
//! Each lap's distance comes from integrating its GPS speed, then every lap is
//! scaled to the reference lap's length so the same corner falls at the same
//! distance even when the lines differ slightly. Channels and time are then
//! read at matching distances.

use std::path::PathBuf;
use std::sync::Arc;

use aim_xrk::{Channel, Lap, Session};

/// One lap in the comparison.
#[derive(Clone)]
pub struct CompareLap {
    pub path: PathBuf,
    pub session: Arc<Session>,
    pub lap: Lap,
    /// e.g. "S35 L10 · 0:30.480 · Alex".
    pub label: String,
    /// Color slot, kept for as long as the lap is in the comparison.
    pub slot: usize,
    pub profile: Option<DistanceProfile>,
}

impl CompareLap {
    pub fn new(path: PathBuf, session: Arc<Session>, lap: Lap, label: String, slot: usize) -> Self {
        let profile = DistanceProfile::from_gps(&session, &lap);
        Self { path, session, lap, label, slot, profile }
    }
}

/// Time against distance through one lap (both strictly increasing).
#[derive(Clone, Debug)]
pub struct DistanceProfile {
    /// Logger time, ms.
    pub times_ms: Vec<i64>,
    /// Meters from the lap's start.
    pub dist_m: Vec<f64>,
}

impl DistanceProfile {
    /// Integrates GPS speed over the lap. None without enough GPS.
    pub fn from_gps(session: &Session, lap: &Lap) -> Option<Self> {
        let gps = session.gps.as_ref()?;
        let lo = gps.times_ms.partition_point(|&t| t < lap.start_ms);
        let hi = gps.times_ms.partition_point(|&t| t <= lap.end_ms);
        Self::integrate(&gps.times_ms[lo..hi], &gps.speed_kmh[lo..hi], lap.start_ms, lap.end_ms)
    }

    /// Trapezoid integration of speed (km/h) sampled at `times_ms`, from
    /// `start_ms` to `end_ms`.
    pub fn integrate(times_ms: &[i64], speed_kmh: &[f64], start_ms: i64, end_ms: i64) -> Option<Self> {
        if times_ms.len() < 4 {
            return None;
        }
        let mut times = vec![start_ms];
        let mut dist = vec![0.0];
        let mut prev = (start_ms, speed_kmh[0]);
        for (&t, &v) in times_ms.iter().zip(speed_kmh).chain(std::iter::once((&end_ms, speed_kmh.last().unwrap()))) {
            if t <= prev.0 {
                continue;
            }
            let d = (prev.1 + v) / 2.0 / 3.6 * (t - prev.0) as f64 / 1000.0;
            let last = *dist.last().unwrap();
            times.push(t);
            // Keep distance strictly increasing so it can be inverted (a stopped kart still "moves" a hair).
            dist.push(last + d.max(1e-6));
            prev = (t, v);
        }
        (dist.last().copied().unwrap_or(0.0) > 1.0).then_some(Self { times_ms: times, dist_m: dist })
    }

    pub fn length(&self) -> f64 {
        *self.dist_m.last().unwrap()
    }

    /// Distance at logger time `t_ms`.
    pub fn dist_at(&self, t_ms: f64) -> f64 {
        interpolate(&self.times_ms.iter().map(|&t| t as f64).collect::<Vec<_>>(), &self.dist_m, t_ms)
    }

    /// Logger time when the kart had covered `d` meters.
    pub fn time_at(&self, d: f64) -> f64 {
        interpolate(&self.dist_m, &self.times_ms.iter().map(|&t| t as f64).collect::<Vec<_>>(), d)
    }
}

/// Linear interpolation of `ys` over increasing `xs`, clamped at the ends.
fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let i = xs.partition_point(|&v| v < x);
    if i == 0 {
        return ys[0];
    }
    if i >= xs.len() {
        return ys[ys.len() - 1];
    }
    let (x0, x1) = (xs[i - 1], xs[i]);
    let f = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
    ys[i - 1] + f * (ys[i] - ys[i - 1])
}

/// A lap's samples of `channel` as (distance on the reference lap's scale, value).
pub fn channel_on_distance(lap: &CompareLap, channel: &Channel, reference_len: f64) -> Vec<[f64; 2]> {
    let Some(p) = &lap.profile else { return Vec::new() };
    let scale = reference_len / p.length();
    let lo = channel.times_ms.partition_point(|&t| t < lap.lap.start_ms);
    let hi = channel.times_ms.partition_point(|&t| t <= lap.lap.end_ms);
    (lo..hi).map(|i| [p.dist_at(channel.times_ms[i] as f64) * scale, channel.values[i]]).collect()
}

/// Time gap to the reference in seconds at `n` evenly spaced distances
/// (positive: `lap` is behind there).
pub fn time_gap(lap: &DistanceProfile, reference: &DistanceProfile, n: usize) -> Vec<[f64; 2]> {
    let (ref_len, lap_len) = (reference.length(), lap.length());
    let (ref_t0, lap_t0) = (reference.times_ms[0] as f64, lap.times_ms[0] as f64);
    (0..=n)
        .map(|k| {
            let f = k as f64 / n as f64;
            let gap = (lap.time_at(f * lap_len) - lap_t0) - (reference.time_at(f * ref_len) - ref_t0);
            [f * ref_len, gap / 1000.0]
        })
        .collect()
}

/// Along `lap`, about `n` points of (logger time ms, meters into the lap,
/// seconds behind `reference` there). The two laps are matched by fraction of
/// their length, so a slightly longer line doesn't count as lost time.
pub fn gap_along(lap: &DistanceProfile, reference: &DistanceProfile, n: usize) -> Vec<(f64, f64, f64)> {
    let (ref_len, lap_len) = (reference.length(), lap.length());
    let (ref_t0, lap_t0) = (reference.times_ms[0] as f64, lap.times_ms[0] as f64);
    let step = (lap.times_ms.len() / n.max(1)).max(1);
    let mut out: Vec<(f64, f64, f64)> = (0..lap.times_ms.len())
        .step_by(step)
        .map(|i| {
            let (t, d) = (lap.times_ms[i] as f64, lap.dist_m[i]);
            let ref_t = reference.time_at(d / lap_len * ref_len) - ref_t0;
            (t, d, ((t - lap_t0) - ref_t) / 1000.0)
        })
        .collect();
    // Always end on the lap's last point: the gap there is the lap time difference.
    let last = lap.times_ms.len() - 1;
    if out.last().is_none_or(|p| p.0 != lap.times_ms[last] as f64) {
        let (t, d) = (lap.times_ms[last] as f64, lap.dist_m[last]);
        out.push((t, d, ((t - lap_t0) - (reference.time_at(ref_len) - ref_t0)) / 1000.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 300 m lap at constant `kmh`, 25 Hz.
    fn constant(kmh: f64) -> DistanceProfile {
        let secs = 300.0 / (kmh / 3.6);
        let n = (secs * 25.0) as i64;
        let times: Vec<i64> = (0..=n).map(|k| k * 40).collect();
        let speeds = vec![kmh; times.len()];
        DistanceProfile::integrate(&times, &speeds, 0, *times.last().unwrap()).unwrap()
    }

    #[test]
    fn gap_along_ends_at_the_lap_time_difference() {
        // 300 m at 36 km/h (30 s) against 300 m at 40 km/h (27 s): 3 s behind at the end, growing evenly.
        let (slow, fast) = (constant(36.0), constant(40.0));
        let g = gap_along(&slow, &fast, 100);
        let end = g.last().unwrap().2;
        assert!((end - 3.0).abs() < 0.05, "{end}");
        let mid = g[g.len() / 2].2;
        assert!((mid - 1.5).abs() < 0.1, "{mid}");
        assert!(gap_along(&fast, &fast, 100).iter().all(|p| p.2.abs() < 0.01));
    }

    #[test]
    fn integrates_speed_to_distance() {
        let p = constant(36.0); // 10 m/s for 30 s
        assert!((p.length() - 300.0).abs() < 0.5, "{}", p.length());
        assert!((p.time_at(150.0) - 15_000.0).abs() < 50.0);
        assert!((p.dist_at(10_000.0) - 100.0).abs() < 0.5);
    }

    #[test]
    fn slower_lap_falls_steadily_behind() {
        let reference = constant(36.0); // 30 s
        let slower = constant(30.0); // 36 s
        let gap = time_gap(&slower, &reference, 10);
        assert!(gap[0][1].abs() < 0.05);
        assert!((gap[5][1] - 3.0).abs() < 0.1, "{:?}", gap[5]);
        assert!((gap[10][1] - 6.0).abs() < 0.1, "{:?}", gap[10]);
        // Gap positions are on the reference's distance scale.
        assert!((gap[10][0] - reference.length()).abs() < 1e-9);
    }
}
