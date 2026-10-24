//! Laps timed from GPS, for sessions whose logger didn't time them: one long
//! lap for the whole session, or two or more laps run together.
//!
//! A MyChron draws its start/finish as a point and a width; when the kart's
//! GPS track misses that area (track learning off, an outdated layout, weak
//! GPS, several layouts of a track loaded) it logs no lap but records
//! everything else. RaceStudio fixes that on download by timing laps from GPS
//! against the track's line. This does the same, against each candidate line
//! given (the tracks near where the session was recorded), and keeps the one
//! the kart really crosses lap after lap. The file is never changed.

use crate::{GpsTrack, Lap, Session};

/// How close the kart must pass to a start/finish point to cross it, as the
/// logger's own start/finish width does (GPS wanders a few meters).
const RADIUS_M: f64 = 15.0;
/// Below this the kart is in the pits, not crossing the line.
const MOVING_KMH: f64 = 10.0;
/// No real lap is quicker than this.
const MIN_LAP_MS: i64 = 15_000;

/// Meters east and north of `(lat0, lon0)`.
fn local(lat0: f64, lon0: f64, lat: f64, lon: f64) -> (f64, f64) {
    ((lon - lon0) * 111_320.0 * lat0.to_radians().cos(), (lat - lat0) * 110_540.0)
}

/// A pass across the line through a start/finish point.
#[derive(Debug, Clone, Copy)]
pub struct Crossing {
    /// When the path crossed the line, in the session's timebase (ms).
    pub time_ms: i64,
    /// The kart's direction then, radians from east, counterclockwise.
    pub heading: f64,
    /// How far from the point it passed (m).
    pub distance_m: f64,
}

/// Each pass across the line through `start_finish`, at right angles to the
/// kart's direction of travel there: within [`RADIUS_M`] of the point, at
/// speed, timed where the path crosses the line (interpolated between fixes).
pub fn crossings(gps: &GpsTrack, start_finish: (f64, f64)) -> Vec<Crossing> {
    let (lat0, lon0) = start_finish;
    let xy: Vec<(f64, f64)> = gps.latitude.iter().zip(&gps.longitude).map(|(&la, &lo)| local(lat0, lon0, la, lo)).collect();
    let n = xy.len();
    let d = |k: usize| (xy[k].0 * xy[k].0 + xy[k].1 * xy[k].1).sqrt();
    let mut out = Vec::new();
    let mut i = 1;
    while i + 1 < n {
        // A closest approach: nearer than both neighbors, inside the radius, moving.
        if d(i) <= RADIUS_M && d(i) <= d(i - 1) && d(i) < d(i + 1) && gps.speed_kmh[i] >= MOVING_KMH {
            // The line at right angles to the kart's heading through the point:
            // find where the along-track coordinate changes sign.
            let (a, b) = (xy[i - 1], xy[i + 1]);
            let (hx, hy) = (b.0 - a.0, b.1 - a.1);
            let len = (hx * hx + hy * hy).sqrt();
            if len > 0.0 {
                let along = |k: usize| (xy[k].0 * hx + xy[k].1 * hy) / len;
                let lo = i.saturating_sub(3);
                let hi = (i + 3).min(n - 1);
                if let Some(k) = (lo..hi).find(|&k| along(k) <= 0.0 && along(k + 1) > 0.0) {
                    let (s0, s1) = (along(k), along(k + 1));
                    let f = -s0 / (s1 - s0);
                    let t = gps.times_ms[k] as f64 + f * (gps.times_ms[k + 1] - gps.times_ms[k]) as f64;
                    out.push(Crossing { time_ms: t.round() as i64, heading: hy.atan2(hx), distance_m: d(i) });
                }
            }
            i += 3;
            continue;
        }
        i += 1;
    }
    out
}

/// The crossings that make laps: those in the direction most of them go (a
/// start/finish is always crossed the same way; a spot the track passes twice
/// is crossed two ways), at least a lap's minimum apart.
fn lap_crossings(all: &[Crossing]) -> Vec<Crossing> {
    let same_way = |a: f64, b: f64| (a - b + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    // The heading most crossings share (within 45°).
    let main = all.iter().max_by_key(|c| all.iter().filter(|o| same_way(o.heading, c.heading).abs() < 0.785).count()).map(|c| c.heading);
    let Some(main) = main else { return Vec::new() };
    let mut out: Vec<Crossing> = Vec::new();
    for c in all.iter().filter(|c| same_way(c.heading, main).abs() < 0.785) {
        if out.last().is_none_or(|last| c.time_ms - last.time_ms >= MIN_LAP_MS) {
            out.push(*c);
        }
    }
    out
}

/// Whether the logger's laps look untimed: one long lap, or a lap that's
/// two or more of the others run together.
pub fn needs_retiming(session: &Session) -> bool {
    let Some(gps) = session.gps.as_ref().filter(|g| g.len() > 100) else { return false };
    // Driving, not a warm-up on the stand: covered some ground.
    let moving = gps.speed_kmh.iter().filter(|&&v| v >= 20.0).count();
    if moving < 200 {
        return false;
    }
    let laps = &session.laps;
    // Out-lap and in-lap aside, the laps that should be complete.
    let inner: Vec<i64> = laps.iter().skip(1).take(laps.len().saturating_sub(2)).map(Lap::duration_ms).collect();
    if inner.is_empty() {
        return true;
    }
    let mut sorted = inner.clone();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    inner.iter().any(|&d| d as f64 > median as f64 * 1.8)
}

/// Laps timed from GPS: the candidate start/finish (index into
/// `candidates`) whose crossings make the most laps of a consistent length,
/// and the laps, out-lap and in-lap included, numbered from 1 as the logger
/// numbers them. None when no candidate gives at least two full laps.
pub fn retime(session: &Session, candidates: &[(f64, f64)]) -> Option<(usize, Vec<Lap>)> {
    let gps = session.gps.as_ref()?;
    let start = session.channels.iter().filter_map(|c| c.times_ms.first()).min().copied()?;
    let end = session.channels.iter().filter_map(|c| c.times_ms.last()).max().copied()?;
    // (candidate, crossings, laps of a typical length, how near the kart passes).
    let mut best: Option<(usize, Vec<Crossing>, usize, f64)> = None;
    for (k, &sf) in candidates.iter().enumerate() {
        let passes = lap_crossings(&crossings(gps, sf));
        if passes.len() < 3 {
            continue;
        }
        let mut times: Vec<i64> = passes.windows(2).map(|w| w[1].time_ms - w[0].time_ms).collect();
        times.sort_unstable();
        let median = times[times.len() / 2] as f64;
        // Laps within a fifth of the typical one: real laps, not a pit stop or a spin.
        let good = times.iter().filter(|&&t| (t as f64 - median).abs() <= median * 0.2).count();
        if good < 2 || good * 2 < times.len() {
            continue;
        }
        let near = passes.iter().map(|c| c.distance_m).sum::<f64>() / passes.len() as f64;
        // Most laps first; between lines giving the same laps, the one driven
        // over most squarely (the track's own line, not one beside it).
        let better = best.as_ref().is_none_or(|b| good > b.2 || (good == b.2 && near < b.3));
        if better {
            best = Some((k, passes, good, near));
        }
    }
    let (k, passes, _, _) = best?;
    let mut bounds = vec![start];
    bounds.extend(passes.iter().map(|c| c.time_ms).filter(|&t| t > start && t < end));
    bounds.push(end);
    let laps = bounds.windows(2).enumerate().map(|(i, w)| Lap { number: i as i32 + 1, start_ms: w[0], end_ms: w[1] }).collect();
    Some((k, laps))
}

/// The laps to use instead of the logger's, when its look untimed and GPS
/// finds more: the candidate used and the laps. A session the logger timed
/// right is left alone, stops and all (a stop crosses no line, so GPS finds
/// no more laps than the logger did).
pub fn fix(session: &Session, candidates: &[(f64, f64)]) -> Option<(usize, Vec<Lap>)> {
    if !needs_retiming(session) {
        return None;
    }
    retime(session, candidates).filter(|(_, laps)| laps.len() > session.laps.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f64 = 200.0 / std::f64::consts::TAU;

    /// A kart lapping a 200 m circle at 36 km/h (20 s a lap) for `laps` laps,
    /// sampled every 100 ms, starting at the circle's top.
    fn circle(laps: usize) -> GpsTrack {
        let (lat0, lon0) = (41.0, -73.0);
        let mut g = GpsTrack::default();
        for i in 0..=(laps * 200) {
            let a = i as f64 / 200.0 * std::f64::consts::TAU;
            let (x, y) = (R * a.sin(), R * a.cos());
            g.times_ms.push(i as i64 * 100);
            g.latitude.push(lat0 + y / 110_540.0);
            g.longitude.push(lon0 + x / (111_320.0 * lat0.to_radians().cos()));
            g.speed_kmh.push(36.0);
            g.yaw_rate_dps.push(0.0);
        }
        g
    }

    #[test]
    fn crossings_are_once_a_lap_at_the_line() {
        // The line at the bottom of the circle: crossed at 10 s, then every 20 s.
        let t = crossings(&circle(3), (41.0 - R / 110_540.0, -73.0));
        assert_eq!(t.len(), 3, "{t:?}");
        for (k, c) in t.iter().enumerate() {
            assert!((c.time_ms - (10_000 + 20_000 * k as i64)).abs() <= 20, "{t:?}");
        }
    }

    #[test]
    fn a_spot_passed_both_ways_counts_once_a_lap() {
        // An out-and-back: east along y = 0 for 20 s, then west along y = 4 m for 20 s.
        let (lat0, lon0) = (41.0, -73.0);
        let mut g = GpsTrack::default();
        for i in 0..1200 {
            let t = i as f64 / 10.0;
            let phase = t % 40.0;
            let (x, y) = if phase < 20.0 { (phase * 10.0 - 100.0, 0.0) } else { (100.0 - (phase - 20.0) * 10.0, 4.0) };
            g.times_ms.push((t * 1000.0) as i64);
            g.latitude.push(lat0 + y / 110_540.0);
            g.longitude.push(lon0 + x / (111_320.0 * lat0.to_radians().cos()));
            g.speed_kmh.push(36.0);
            g.yaw_rate_dps.push(0.0);
        }
        // The point at x = 0 is passed twice a lap, one way then the other: laps, not half-laps.
        let laps = lap_crossings(&crossings(&g, (lat0, lon0)));
        assert!(laps.windows(2).all(|w| (w[1].time_ms - w[0].time_ms - 40_000).abs() < 100), "{laps:?}");
    }

    #[test]
    fn a_line_never_driven_gives_nothing() {
        assert!(crossings(&circle(3), (41.01, -73.0)).is_empty());
    }

    #[test]
    fn one_long_lap_is_retimed_from_the_line_crossed() {
        let gps = circle(5);
        let session = Session {
            info: Default::default(),
            channels: vec![crate::Channel { name: "RPM".into(), group: None, units: String::new(), decimals: 0, interpolate: true, times_ms: gps.times_ms.clone(), values: vec![9000.0; gps.len()] }],
            laps: vec![Lap { number: 1, start_ms: 0, end_ms: 100_000 }],
            gps: Some(gps),
        };
        // An outdated line nobody drives, then the real one.
        let (k, laps) = retime(&session, &[(41.01, -73.0), (41.0 - R / 110_540.0, -73.0)]).unwrap();
        assert_eq!(k, 1);
        // Out-lap to 10 s, four 20 s laps, in-lap from 90 s.
        assert_eq!(laps.len(), 6);
        assert!(laps[1..5].iter().all(|l| (l.duration_ms() - 20_000).abs() <= 40), "{laps:?}");
    }
}
