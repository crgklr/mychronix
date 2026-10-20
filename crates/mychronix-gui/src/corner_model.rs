//! Corners as a data team reads them: each corner runs from its braking point
//! to the next corner's braking point, so the exit and the straight after it
//! count (a fast entry that spoils the exit is a slow corner). The corners add
//! up to the lap, so the fastest of each, from laps that count, add up to the
//! "best of every corner": a lap made only of corners the driver has driven.
//!
//! Every lap is sampled every 2 m along the track, on the best lap's scale of
//! distance, so laps line up corner for corner. Built once per session.

use aim_xrk::{Lap, Session};

use crate::braking;
use crate::compare::DistanceProfile;

/// Samples along the lap, this far apart (m).
pub const STEP_M: f64 = 2.0;
/// Harder than this is braking (g), RaceStudio's threshold.
const BRAKING_G: f64 = -0.15;
/// More cornering than this is turning (g), for trail braking.
const TURNING_G: f64 = 0.3;

/// What the kart was doing while slowing for a corner, 2 m at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slowing {
    /// On the brakes, going straight.
    Brake,
    /// On the brakes while already turning (only with the IMU's forward g).
    Trail,
    /// Slowing off the brakes: lifting, or the kart sliding.
    Lift,
}

/// One corner: meters along the best lap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub from: f64,
    pub apex: f64,
    pub to: f64,
}

/// One lap, sampled every [`STEP_M`], plus a last sample at the lap's very
/// end, so times through the corners add up to the whole lap.
#[derive(Debug, Clone)]
pub struct Trace {
    /// Index in `session.laps`.
    pub lap: usize,
    pub number: i32,
    /// Seconds from the lap's start.
    pub t: Vec<f64>,
    pub kmh: Vec<f64>,
    /// Forward g: the IMU's when there is one, else the GPS's.
    pub forward: Vec<f64>,
    /// Cornering g (GPS).
    pub cornering: Vec<f64>,
    /// Steering angle, degrees, when the session has one.
    pub steering: Option<Vec<f64>>,
    /// Position, meters east and north of the session's first GPS point.
    pub xy: Vec<(f64, f64)>,
}

/// One lap through one corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    /// Braking point to the next corner's braking point.
    pub time_s: f64,
    /// Where braking starts (the top speed before the corner), meters into the lap.
    pub brake_m: f64,
    pub entry_kmh: f64,
    /// The slowest in the corner, and where.
    pub min_kmh: f64,
    pub min_m: f64,
    /// The fastest after it, before the next braking point.
    pub exit_kmh: f64,
    /// From the braking point to the slowest point, 2 m at a time.
    pub slowing: Vec<Slowing>,
    pub brake_s: f64,
    /// With the IMU only.
    pub trail_s: Option<f64>,
    pub peak_g: f64,
    /// Largest angle either way, and direction changes of 5° or more.
    pub steering: Option<(f64, usize)>,
}

impl Stats {
    pub fn slowing_m(&self) -> f64 {
        self.slowing.len() as f64 * STEP_M
    }
}

/// How a note should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Attention,
    Plain,
}

pub struct CornerModel {
    pub length_m: f64,
    pub segments: Vec<Segment>,
    pub traces: Vec<Trace>,
    /// `stats[trace][corner]`.
    pub stats: Vec<Vec<Stats>>,
    /// Whether each trace's lap counts toward bests (not an out-lap, in-lap or slow lap).
    pub counts: Vec<bool>,
    /// Per corner, the trace with the fastest pass among laps that count.
    pub best_of: Vec<usize>,
    /// The trace of the session's best lap.
    pub best: usize,
    /// Trail braking can be told apart (the session has the IMU's forward g).
    pub has_trail: bool,
}

/// Meters east and north of a reference point.
fn local_xy(lat0: f64, lon0: f64, lat: f64, lon: f64) -> (f64, f64) {
    ((lon - lon0) * 111_320.0 * lat0.to_radians().cos(), (lat - lat0) * 110_540.0)
}

/// Linear interpolation of `ys` at `x` over increasing `xs`, clamped at the ends.
fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let i = xs.partition_point(|&v| v < x);
    if i == 0 {
        return ys[0];
    }
    if i >= xs.len() {
        return ys[ys.len() - 1];
    }
    let f = if xs[i] > xs[i - 1] { (x - xs[i - 1]) / (xs[i] - xs[i - 1]) } else { 0.0 };
    ys[i - 1] + f * (ys[i] - ys[i - 1])
}

/// The sample of `c` at or before `t` (ms).
fn sample(c: &aim_xrk::Channel, t: f64) -> f64 {
    let i = c.times_ms.partition_point(|&x| (x as f64) <= t).saturating_sub(1);
    c.values.get(i).copied().unwrap_or(0.0)
}

impl CornerModel {
    /// None without GPS, a best lap, or corners on it. `counts[i]`: whether
    /// session lap `i` counts toward bests.
    pub fn build(session: &Session, best: &Lap, counts: &[bool]) -> Option<Self> {
        let gps = session.gps.as_ref()?;
        let (zones, best_profile) = braking::lap_zones(session, best)?;
        if zones.is_empty() {
            return None;
        }
        let length_m = best_profile.length();
        let starts: Vec<f64> = zones.iter().enumerate().map(|(k, z)| if k == 0 { 0.0 } else { z.start_m }).collect();
        let segments: Vec<Segment> = zones
            .iter()
            .enumerate()
            .map(|(k, z)| Segment { from: starts[k], apex: z.apex_m, to: starts.get(k + 1).copied().unwrap_or(length_m) })
            .collect();
        let imu = session.channel("Forward g");
        let forward = imu.or_else(|| session.channel("GPS LonAcc"))?;
        let cornering = session.channel("GPS LatAcc")?;
        let steering = session.channels.iter().find(|c| c.name.to_lowercase().contains("steer") && c.values.len() > 1);
        let (lat0, lon0) = (*gps.latitude.first()?, *gps.longitude.first()?);
        let gps_t: Vec<f64> = gps.times_ms.iter().map(|&t| t as f64).collect();
        let n = (length_m / STEP_M) as usize;
        // Every STEP_M along the best lap, and its very end (the last step is
        // usually shorter): without it the corners would miss up to STEP_M of the lap.
        let mut at_m: Vec<f64> = (0..=n).map(|k| k as f64 * STEP_M).collect();
        if length_m - n as f64 * STEP_M > 0.01 {
            at_m.push(length_m);
        }

        let mut traces = Vec::new();
        let mut trace_counts = Vec::new();
        for (i, lap) in session.laps.iter().enumerate() {
            // Out-laps and in-laps don't go all the way round.
            if i == 0 || i + 1 == session.laps.len() {
                continue;
            }
            let Some(p) = DistanceProfile::from_gps(session, lap) else { continue };
            let times: Vec<f64> = p.times_ms.iter().map(|&t| t as f64).collect();
            let scale = p.length() / length_m;
            let mut tr = Trace {
                lap: i,
                number: lap.number,
                t: Vec::with_capacity(at_m.len()),
                kmh: Vec::with_capacity(at_m.len()),
                forward: Vec::with_capacity(at_m.len()),
                cornering: Vec::with_capacity(at_m.len()),
                steering: steering.map(|_| Vec::with_capacity(at_m.len())),
                xy: Vec::with_capacity(at_m.len()),
            };
            for &m in &at_m {
                let t = interp(&p.dist_m, &times, m * scale);
                tr.t.push((t - lap.start_ms as f64) / 1000.0);
                tr.kmh.push(interp(&gps_t, &gps.speed_kmh, t));
                tr.forward.push(sample(forward, t));
                tr.cornering.push(sample(cornering, t));
                if let (Some(s), Some(v)) = (steering, tr.steering.as_mut()) {
                    v.push(sample(s, t));
                }
                tr.xy.push(local_xy(lat0, lon0, interp(&gps_t, &gps.latitude, t), interp(&gps_t, &gps.longitude, t)));
            }
            traces.push(tr);
            trace_counts.push(counts.get(i).copied().unwrap_or(false));
        }
        let best_trace = traces.iter().position(|t| session.laps[t.lap] == *best)?;
        let has_trail = imu.is_some();
        let stats: Vec<Vec<Stats>> = traces.iter().map(|tr| segments.iter().enumerate().map(|(k, s)| corner_stats(tr, &segments, k, s, has_trail)).collect()).collect();
        let racing: Vec<usize> = (0..traces.len()).filter(|&i| trace_counts[i]).collect();
        let best_of = (0..segments.len())
            .map(|k| racing.iter().copied().min_by(|&a, &b| stats[a][k].time_s.total_cmp(&stats[b][k].time_s)).unwrap_or(best_trace))
            .collect();
        Some(Self { length_m, segments, traces, stats, counts: trace_counts, best_of, best: best_trace, has_trail })
    }

    /// The best of every corner: the fastest of each, added up.
    pub fn ideal_s(&self) -> f64 {
        self.best_of.iter().enumerate().map(|(k, &b)| self.stats[b][k].time_s).sum()
    }

    /// How far `trace` is off the best through corner `k` (s).
    pub fn loss(&self, trace: usize, k: usize) -> f64 {
        self.stats[trace][k].time_s - self.stats[self.best_of[k]][k].time_s
    }

    /// The corner where `trace` lost the most against the best.
    pub fn worst(&self, trace: usize) -> usize {
        (0..self.segments.len()).max_by(|&a, &b| self.loss(trace, a).total_cmp(&self.loss(trace, b))).unwrap_or(0)
    }

    /// The trace for session lap `lap`.
    pub fn trace_of(&self, lap: usize) -> Option<usize> {
        self.traces.iter().position(|t| t.lap == lap)
    }

    /// What stands out about `trace` in corner `k`: at most two short notes,
    /// the first saying what cost time, if anything did.
    pub fn notes(&self, trace: usize, k: usize) -> Vec<(Tone, String)> {
        let (a, b) = (&self.stats[trace][k], &self.stats[self.best_of[k]][k]);
        let mut out = Vec::new();
        let d = self.loss(trace, k);
        let speed = |kmh: f64| crate::units::speed(kmh);
        let (su, du) = (crate::units::speed_unit(), crate::units::distance_unit());
        if self.best_of[k] == trace {
            out.push((Tone::Good, format!("Your best corner {}", k + 1)));
        } else if d > 0.03 {
            // Only what cost time: slower at the apex, braking earlier, slower out.
            let mut why = Vec::new();
            let (d_min, d_exit) = (speed(a.min_kmh) - speed(b.min_kmh), speed(a.exit_kmh) - speed(b.exit_kmh));
            let d_brake = crate::units::distance(a.brake_m - b.brake_m);
            let min_gap = if crate::units::imperial() { 0.6 } else { 1.0 };
            if d_min <= -min_gap {
                why.push(format!("{:.1} {su} slower at the apex", -d_min));
            }
            if d_brake <= -crate::units::distance(2.5) {
                why.push(format!("brakes {:.0} {du} earlier", -d_brake));
            }
            if d_exit <= -min_gap && why.len() < 2 {
                why.push(format!("{:.1} {su} slower down the next straight", -d_exit));
            }
            out.push((Tone::Attention, if why.is_empty() { format!("{d:.2} s off the best") } else { format!("{d:.2} s off the best: {}", why.join(", ")) }));
        }
        if a.brake_s < 0.05 && a.entry_kmh - a.min_kmh >= 3.0 {
            out.push((Tone::Plain, "Slows without braking: lift or slide".into()));
        } else if let Some(t) = a.trail_s.filter(|&t| t >= 0.6) {
            out.push((Tone::Plain, format!("Trail brakes {t:.1} s")));
        }
        if out.len() < 2 && a.steering.is_some_and(|s| s.1 >= 3) {
            out.push((Tone::Attention, format!("{} steering corrections", a.steering.map(|s| s.1).unwrap_or(0))));
        }
        out.truncate(2);
        out
    }
}

/// One trace through corner `k`.
fn corner_stats(tr: &Trace, segments: &[Segment], k: usize, s: &Segment, has_trail: bool) -> Stats {
    let last = tr.kmh.len() - 1;
    let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
    // The last corner runs to the lap's very end (the trace's last sample).
    let end = if k + 1 == segments.len() { last } else { idx(s.to) };
    let time_s = tr.t[end] - tr.t[idx(s.from)];
    // Braking point: the top speed before the corner, looking no further back
    // than just past the previous corner's apex.
    let back = if k > 0 { (segments[k - 1].apex + 4.0).max(s.from - 30.0) } else { 0.0 };
    let (lo, hi) = (idx(back), idx(s.apex).max(idx(back)));
    let bi = (lo..=hi).max_by(|&a, &b| tr.kmh[a].total_cmp(&tr.kmh[b])).unwrap_or(lo);
    let mi = (bi..=idx(s.to.min(s.apex + 25.0)).max(bi)).min_by(|&a, &b| tr.kmh[a].total_cmp(&tr.kmh[b])).unwrap_or(bi);
    let exit_kmh = (mi..=idx(s.to).max(mi)).map(|i| tr.kmh[i]).fold(0.0, f64::max);
    let (mut slowing, mut brake_s, mut trail_s, mut peak_g) = (Vec::new(), 0.0, 0.0, 0.0f64);
    for i in bi..mi {
        let dt = (tr.t[i + 1] - tr.t[i]).max(0.0);
        let braking = tr.forward[i] < BRAKING_G;
        let state = match (braking, has_trail && tr.cornering[i].abs() > TURNING_G) {
            (true, true) => Slowing::Trail,
            (true, false) => Slowing::Brake,
            _ => Slowing::Lift,
        };
        if braking {
            brake_s += dt;
        }
        if state == Slowing::Trail {
            trail_s += dt;
        }
        peak_g = peak_g.max(-tr.forward[i]);
        slowing.push(state);
    }
    let steering = tr.steering.as_ref().map(|v| {
        let w = &v[bi..=idx(s.to.min(s.apex + 20.0)).max(bi)];
        (w.iter().fold(0.0f64, |m, x| m.max(x.abs())), crate::report::reversals(w, 5.0))
    });
    Stats {
        time_s,
        brake_m: bi as f64 * STEP_M,
        entry_kmh: tr.kmh[bi],
        min_kmh: tr.kmh[mi],
        min_m: mi as f64 * STEP_M,
        exit_kmh,
        slowing,
        brake_s,
        trail_s: has_trail.then_some(trail_s),
        peak_g,
        steering,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lap at 40 km/h that brakes to 20 into a corner at 50 m and is back
    /// up to 40 by 100 m.
    fn trace(brake_at: f64) -> Trace {
        let n = 100;
        let mut tr = Trace { lap: 1, number: 2, t: vec![], kmh: vec![], forward: vec![], cornering: vec![], steering: None, xy: vec![] };
        let mut t = 0.0;
        for k in 0..=n {
            let m = k as f64 * STEP_M;
            let v = if m < brake_at { 40.0 } else if m < 50.0 { 40.0 - 20.0 * (m - brake_at) / (50.0 - brake_at) } else if m < 100.0 { 20.0 + 20.0 * (m - 50.0) / 50.0 } else { 40.0 };
            tr.t.push(t);
            t += STEP_M / (v / 3.6);
            tr.kmh.push(v);
            tr.forward.push(if (brake_at..50.0).contains(&m) { -0.4 } else { 0.0 });
            tr.cornering.push(if (40.0..60.0).contains(&m) { 0.5 } else { 0.0 });
            tr.xy.push((m, 0.0));
        }
        tr
    }

    #[test]
    fn a_corner_reads_as_braking_then_trail() {
        let tr = trace(30.0);
        let segs = [Segment { from: 0.0, apex: 50.0, to: 200.0 }];
        let s = corner_stats(&tr, &segs, 0, &segs[0], true);
        assert_eq!((s.brake_m, s.entry_kmh, s.min_kmh, s.min_m), (30.0, 40.0, 20.0, 50.0));
        assert_eq!(s.slowing.len(), 10);
        assert_eq!(s.slowing[..5], [Slowing::Brake; 5]);
        assert_eq!(s.slowing[5..], [Slowing::Trail; 5]);
        assert!(s.trail_s.unwrap() > 0.0 && (s.peak_g - 0.4).abs() < 1e-9);
        // Without the IMU there's no telling trail braking apart.
        let s = corner_stats(&tr, &segs, 0, &segs[0], false);
        assert!(s.slowing.iter().all(|&x| x == Slowing::Brake) && s.trail_s.is_none());
    }

    #[test]
    fn corner_times_add_up_to_the_whole_lap() {
        // A 200.8 m lap: the last sample, at the lap's end, is 0.8 m past the last whole step.
        let mut tr = trace(30.0);
        let end = tr.t.last().unwrap() + 0.8 / (40.0 / 3.6);
        tr.t.push(end);
        tr.kmh.push(40.0);
        tr.forward.push(0.0);
        tr.cornering.push(0.0);
        tr.xy.push((200.8, 0.0));
        let segs = [Segment { from: 0.0, apex: 50.0, to: 120.0 }, Segment { from: 120.0, apex: 160.0, to: 200.8 }];
        let total: f64 = (0..2).map(|k| corner_stats(&tr, &segs, k, &segs[k], true).time_s).sum();
        assert!((total - end).abs() < 1e-9, "{total} vs {end}");
    }
}
