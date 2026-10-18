//! Braking per corner, as RaceStudio's corner reports: a corner's braking
//! phase runs from the highest speed before it to the lowest speed in it.
//! Corners are found from GPS speed alone, so any session with GPS has them.

use aim_xrk::{Lap, Session};

use crate::compare::DistanceProfile;

/// A drop (and recovery) of at least this much speed makes a corner, so GPS
/// wobble on a straight doesn't.
const MIN_DROP_KMH: f64 = 4.0;

/// One corner's braking phase on one lap.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    /// Logger time at the highest speed (braking starts) and the lowest (the apex).
    pub start_ms: i64,
    pub apex_ms: i64,
    /// Meters into the lap of each.
    pub start_m: f64,
    pub apex_m: f64,
    pub entry_kmh: f64,
    pub min_kmh: f64,
    /// Time in the zone actually on the brakes (GPS LonAcc below
    /// RaceStudio's −0.15 g), s; the rest is lifting or scrubbing speed.
    pub brake_s: f64,
    /// Hardest deceleration in the zone (Forward g from the IMU when the
    /// session has it, else GPS LonAcc), g, as a positive number.
    pub peak_g: f64,
    /// With the IMU: seconds braking while already turning (see [`trail_in`]).
    pub trail_s: Option<f64>,
    /// With a steering channel, from the start of braking to the corner's
    /// exit (the next corner's braking or the lap's end): the largest angle
    /// either way and how many times the wheel changed direction by 5°+.
    pub steering: Option<(f64, usize)>,
}

impl Zone {
    /// Braking distance, m: from the highest speed to the lowest.
    pub fn distance_m(&self) -> f64 {
        self.apex_m - self.start_m
    }

    pub fn time_s(&self) -> f64 {
        (self.apex_ms - self.start_ms) as f64 / 1000.0
    }

    /// Average deceleration over the braking phase, g.
    pub fn decel_g(&self) -> f64 {
        let t = self.time_s();
        if t <= 0.0 { 0.0 } else { (self.entry_kmh - self.min_kmh) / 3.6 / t / 9.81 }
    }
}

/// A lap's braking zones in order, with its distance profile (None without GPS).
pub fn lap_zones(session: &Session, lap: &Lap) -> Option<(Vec<Zone>, DistanceProfile)> {
    let gps = session.gps.as_ref()?;
    let profile = DistanceProfile::from_gps(session, lap)?;
    let lo = gps.times_ms.partition_point(|&t| t < lap.start_ms);
    let hi = gps.times_ms.partition_point(|&t| t <= lap.end_ms);
    let mut zones = find_zones(&gps.times_ms[lo..hi], &gps.speed_kmh[lo..hi], |t| profile.dist_at(t as f64));
    // The IMU's forward g is sharper than the GPS's (50 readings a second, unsmoothed).
    let imu = session.channel("Forward g");
    if let Some(lon) = imu.or_else(|| session.channel("GPS LonAcc")) {
        for z in &mut zones {
            (z.brake_s, z.peak_g) = braking_in(lon, z.start_ms, z.apex_ms);
        }
    }
    if let (Some(fwd), Some(lat)) = (imu, session.channel("GPS LatAcc")) {
        for z in &mut zones {
            z.trail_s = Some(trail_in(fwd, lat, z.start_ms, z.apex_ms));
        }
    }
    let steer = session.channels.iter().find(|c| c.name.to_lowercase().contains("steer") && c.values.len() > 1);
    if let Some(st) = steer {
        let ends: Vec<i64> = zones.iter().skip(1).map(|z| z.start_ms).chain(std::iter::once(lap.end_ms)).collect();
        for (z, end) in zones.iter_mut().zip(ends) {
            let lo = st.times_ms.partition_point(|&t| t < z.start_ms);
            let hi = st.times_ms.partition_point(|&t| t <= end);
            if hi > lo + 1 {
                let v = &st.values[lo..hi];
                z.steering = Some((v.iter().fold(0.0f64, |m, x| m.max(x.abs())), crate::report::reversals(v, 5.0)));
            }
        }
    }
    Some((zones, profile))
}

/// Over [start_ms, end_ms] of a longitudinal g channel: seconds below
/// −0.15 g and the hardest deceleration (positive g).
pub fn braking_in(lon: &aim_xrk::Channel, start_ms: i64, end_ms: i64) -> (f64, f64) {
    let lo = lon.times_ms.partition_point(|&t| t < start_ms);
    let hi = lon.times_ms.partition_point(|&t| t <= end_ms);
    let (mut on_ms, mut peak) = (0i64, 0.0f64);
    for i in lo..hi {
        let next = lon.times_ms.get(i + 1).copied().unwrap_or(end_ms).min(end_ms);
        let v = lon.values[i];
        if v < -0.15 {
            on_ms += (next - lon.times_ms[i]).max(0);
        }
        peak = peak.max(-v);
    }
    (on_ms as f64 / 1000.0, peak)
}

/// Trail braking over [start_ms, end_ms]: seconds braking (forward g below
/// −0.15 g) while already turning (over 0.3 g of cornering), when braking and
/// turning overlap on the way into the corner.
pub fn trail_in(forward: &aim_xrk::Channel, lat: &aim_xrk::Channel, start_ms: i64, end_ms: i64) -> f64 {
    let lo = forward.times_ms.partition_point(|&t| t < start_ms);
    let hi = forward.times_ms.partition_point(|&t| t <= end_ms);
    let mut ms = 0i64;
    for i in lo..hi {
        let t = forward.times_ms[i];
        let next = forward.times_ms.get(i + 1).copied().unwrap_or(end_ms).min(end_ms);
        let j = lat.times_ms.partition_point(|&x| x <= t).saturating_sub(1);
        let turning = lat.values.get(j).is_some_and(|g| g.abs() > 0.3);
        if forward.values[i] < -0.15 && turning {
            ms += (next - t).max(0);
        }
    }
    ms as f64 / 1000.0
}

/// Braking zones in a speed trace: each fall of at least [`MIN_DROP_KMH`] from
/// a peak to a low that speed then climbs back from by as much.
pub fn find_zones(times_ms: &[i64], speed_kmh: &[f64], dist_at: impl Fn(i64) -> f64) -> Vec<Zone> {
    let mut zones = Vec::new();
    if times_ms.len() < 3 {
        return zones;
    }
    let make = |hi: usize, lo: usize| Zone {
        start_ms: times_ms[hi],
        apex_ms: times_ms[lo],
        start_m: dist_at(times_ms[hi]),
        apex_m: dist_at(times_ms[lo]),
        entry_kmh: speed_kmh[hi],
        min_kmh: speed_kmh[lo],
        brake_s: 0.0,
        peak_g: 0.0,
        trail_s: None,
        steering: None,
    };
    let mut peak = 0;
    let mut low: Option<usize> = None;
    for i in 1..speed_kmh.len() {
        let v = speed_kmh[i];
        match low {
            None => {
                if v > speed_kmh[peak] {
                    peak = i;
                } else if v < speed_kmh[peak] - MIN_DROP_KMH {
                    low = Some(i);
                }
            }
            Some(l) => {
                if v < speed_kmh[l] {
                    low = Some(i);
                } else if v > speed_kmh[l] + MIN_DROP_KMH {
                    zones.push(make(peak, l));
                    low = None;
                    peak = i;
                }
            }
        }
    }
    // Still braking at the line: the corner ends this lap.
    if let Some(l) = low {
        zones.push(make(peak, l));
    }
    zones
}

/// For each reference corner, the matching zone of another lap: the one whose
/// apex falls nearest the same fraction of the lap, within 6%.
pub fn match_zones(reference: &[Zone], ref_len: f64, zones: &[Zone], lap_len: f64) -> Vec<Option<usize>> {
    let mut used = vec![false; zones.len()];
    reference
        .iter()
        .map(|r| {
            let f = r.apex_m / ref_len;
            let best = zones
                .iter()
                .enumerate()
                .filter(|(k, _)| !used[*k])
                .map(|(k, z)| (k, (z.apex_m / lap_len - f).abs()))
                .filter(|(_, d)| *d < 0.06)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| k);
            if let Some(k) = best {
                used[k] = true;
            }
            best
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 Hz speeds; distance = sample index × 10 m.
    fn zones_of(speeds: &[f64]) -> Vec<Zone> {
        let times: Vec<i64> = (0..speeds.len() as i64).map(|k| k * 100).collect();
        find_zones(&times, speeds, |t| t as f64 / 10.0)
    }

    #[test]
    fn finds_each_corner_from_peak_to_low() {
        // Straight at 50, brake to 30, accelerate to 52, brake to 25, back up to 40.
        let v = [48.0, 50.0, 46.0, 40.0, 34.0, 30.0, 31.0, 38.0, 45.0, 52.0, 47.0, 35.0, 25.0, 27.0, 33.0, 40.0];
        let z = zones_of(&v);
        assert_eq!(z.len(), 2);
        assert_eq!((z[0].entry_kmh, z[0].min_kmh), (50.0, 30.0));
        assert_eq!((z[0].start_m, z[0].apex_m), (10.0, 50.0));
        assert_eq!(z[0].distance_m(), 40.0);
        assert!((z[0].time_s() - 0.4).abs() < 1e-9);
        // 20 km/h in 0.4 s = 13.9 m/s² = 1.42 g.
        assert!((z[0].decel_g() - 1.416).abs() < 0.01, "{}", z[0].decel_g());
        assert_eq!((z[1].entry_kmh, z[1].min_kmh), (52.0, 25.0));
    }

    #[test]
    fn time_on_the_brakes_and_peak() {
        // 100 ms steps: lift, lift, stab, stab, stab, lift.
        let lon = aim_xrk::Channel {
            name: "GPS LonAcc".into(),
            group: None,
            units: "g".into(),
            decimals: 2,
            interpolate: true,
            times_ms: (0..6).map(|k| k * 100).collect(),
            values: vec![-0.08, -0.1, -0.3, -0.6, -0.2, -0.05],
        };
        let (on, peak) = braking_in(&lon, 0, 600);
        assert!((on - 0.3).abs() < 1e-9, "{on}");
        assert!((peak - 0.6).abs() < 1e-9);
    }

    #[test]
    fn wobble_on_a_straight_is_not_a_corner() {
        assert!(zones_of(&[50.0, 48.5, 50.5, 49.0, 51.0, 49.5, 51.5]).is_empty());
    }

    #[test]
    fn matches_corners_by_where_they_fall() {
        let at = |m: f64| Zone { start_ms: 0, apex_ms: 0, start_m: m - 20.0, apex_m: m, entry_kmh: 0.0, min_kmh: 0.0, brake_s: 0.0, peak_g: 0.0, trail_s: None, steering: None };
        let reference = [at(50.0), at(150.0), at(250.0)];
        // A slightly longer lap that missed the middle corner's dip.
        let lap = [at(52.0), at(262.0)];
        assert_eq!(match_zones(&reference, 300.0, &lap, 310.0), [Some(0), None, Some(1)]);
    }
}
