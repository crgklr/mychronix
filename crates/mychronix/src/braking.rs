//! Finding corners, as RaceStudio's corner reports do: a corner's braking
//! phase runs from the highest speed before it to the lowest speed in it.
//! Corners are found from GPS speed alone, so any session with GPS has them;
//! what happens in each is worked out in `corner_model`.

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
}

/// A lap's braking zones in order, with its distance profile (None without GPS).
pub fn lap_zones(session: &Session, lap: &Lap) -> Option<(Vec<Zone>, DistanceProfile)> {
    let gps = session.gps.as_ref()?;
    let profile = DistanceProfile::from_gps(session, lap)?;
    let lo = gps.times_ms.partition_point(|&t| t < lap.start_ms);
    let hi = gps.times_ms.partition_point(|&t| t <= lap.end_ms);
    let zones = find_zones(&gps.times_ms[lo..hi], &gps.speed_kmh[lo..hi], |t| profile.dist_at(t as f64));
    Some((zones, profile))
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
        assert_eq!((z[1].entry_kmh, z[1].min_kmh), (52.0, 25.0));
    }

    #[test]
    fn wobble_on_a_straight_is_not_a_corner() {
        assert!(zones_of(&[50.0, 48.5, 50.5, 49.0, 51.0, 49.5, 51.5]).is_empty());
    }

}
