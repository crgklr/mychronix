//! Corners as a data team reads them: each corner runs from its braking point
//! to the next corner's braking point, so the exit and the straight after it
//! count (a fast entry that spoils the exit is a slow corner). The corners add
//! up to the lap, so the fastest of each, from laps that count, add up to the
//! "best of every corner": a lap made only of corners the driver has driven.
//!
//! Every lap is sampled every 2 m along the track, on the best lap's scale of
//! distance, so laps line up corner for corner. Built once per session.
//!
//! Where the corners are comes from the track's saved corners when there are
//! some and this session's best lap follows their line (see `track_corners`),
//! so the numbers match from session to session; otherwise from the braking
//! zones on this session's best lap.

use aim_xrk::geo::{from_local, to_local as local_xy};
use aim_xrk::{Lap, Session};

use crate::braking;
use crate::compare::DistanceProfile;
use crate::track_corners::{SavedCorner, TrackCorners};

/// Samples along the lap, this far apart (m).
pub const STEP_M: f64 = 2.0;
/// Harder than this is braking (g), RaceStudio's threshold.
const BRAKING_G: f64 = -0.15;
/// More cornering than this is turning (g), for trail braking.
const TURNING_G: f64 = 0.3;
/// A best lap follows a track's saved line when it's typically within this
/// of it (m)… GPS wanders a few meters from day to day (0.6 to 3.4 m typical,
/// 8 m at most, over 53 sessions at two tracks, Nov 2026).
const FIT_M: f64 = 6.0;
/// …and nine tenths of it, and every corner, within this: another layout
/// is 20 m or more off in places.
const FIT_MAX_M: f64 = 15.0;
/// How far along the lap to look for a saved point either side of where
/// its distance puts it (m): far enough for a lap of another length, near
/// enough not to land on another part of the track that passes close by.
const REACH_M: f64 = 60.0;

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
    /// The segments are the track's saved corners (see `track_corners`),
    /// not this session's own.
    pub from_track: bool,
    /// Where the traces' positions are measured from, (lat, lon).
    origin: (f64, f64),
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

/// Linear interpolation of `ys` at `x` over increasing `xs`, clamped at the ends.
fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let i = xs.partition_point(|&v| v < x);
    if i == 0 {
        return ys[0];
    }
    if i >= xs.len() {
        return ys[ys.len() - 1];
    }
    let f = if xs[i] > xs[i - 1] {
        (x - xs[i - 1]) / (xs[i] - xs[i - 1])
    } else {
        0.0
    };
    ys[i - 1] + f * (ys[i] - ys[i - 1])
}

/// The sample of `c` at or before `t` (ms).
fn sample(c: &aim_xrk::Channel, t: f64) -> f64 {
    let i = c
        .times_ms
        .partition_point(|&x| (x as f64) <= t)
        .saturating_sub(1);
    c.values.get(i).copied().unwrap_or(0.0)
}

impl CornerModel {
    /// None without GPS, a best lap, or corners on it. `counts[i]`: whether
    /// session lap `i` counts toward bests. `saved`: the track's corners, used
    /// when the best lap follows their line.
    pub fn build(
        session: &Session,
        best: &Lap,
        counts: &[bool],
        saved: Option<&TrackCorners>,
    ) -> Option<Self> {
        Self::build_with(session, best, counts, saved, true)
    }

    /// `on_track`: with saved corners the best lap follows, read every lap
    /// along their line (see `DistanceProfile::along_line`); else each lap's
    /// distance, scaled to the best lap's length.
    fn build_with(
        session: &Session,
        best: &Lap,
        counts: &[bool],
        saved: Option<&TrackCorners>,
        on_track: bool,
    ) -> Option<Self> {
        let gps = session.gps.as_ref()?;
        let (zones, best_profile) = braking::lap_zones(session, best)?;
        let (lat0, lon0) = (*gps.latitude.first()?, *gps.longitude.first()?);
        let origin = (lat0, lon0);
        // The saved corners, when the best lap follows their line: where they
        // fall on it, by its own distance.
        let placed = saved.and_then(|s| {
            let n = (best_profile.length() / STEP_M) as usize;
            let xy: Vec<(f64, f64)> = (0..=n)
                .map(|k| {
                    let t = best_profile.time_at(k as f64 * STEP_M);
                    let lat = interp_t(&gps.times_ms, &gps.latitude, t);
                    let lon = interp_t(&gps.times_ms, &gps.longitude, t);
                    local_xy(lat0, lon0, lat, lon)
                })
                .collect();
            Some((s, place(s, &xy, origin, best_profile.length())?))
        });
        // Read along the line: the corners where they were saved, on its scale.
        let line = placed.as_ref().filter(|_| on_track).map(|(s, _)| *s);
        let length_m = line.map_or(best_profile.length(), |s| s.length_m());
        use crate::channels::{Role, find};
        let forward = find(session, Role::ForwardG)?;
        let cornering = find(session, Role::CorneringG)?;
        let steering = crate::channels::steering(session);
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
            // Along the line where the lap follows it, else its distance scaled.
            let Some(p) = line
                .and_then(|s| DistanceProfile::along_line(session, lap, &s.line, STEP_M))
                .or_else(|| DistanceProfile::from_gps(session, lap))
            else {
                continue;
            };
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
                tr.xy.push(local_xy(
                    lat0,
                    lon0,
                    interp(&gps_t, &gps.latitude, t),
                    interp(&gps_t, &gps.longitude, t),
                ));
            }
            traces.push(tr);
            trace_counts.push(counts.get(i).copied().unwrap_or(false));
        }
        let best_trace = traces.iter().position(|t| session.laps[t.lap] == *best)?;
        let from_track = placed.is_some();
        let segments = match (line, placed) {
            (Some(s), _) => saved_segments(s),
            (None, Some((_, segs))) => segs,
            (None, None) if zones.is_empty() => return None,
            (None, None) => {
                let starts: Vec<f64> = zones
                    .iter()
                    .enumerate()
                    .map(|(k, z)| if k == 0 { 0.0 } else { z.start_m })
                    .collect();
                zones
                    .iter()
                    .enumerate()
                    .map(|(k, z)| Segment {
                        from: starts[k],
                        apex: z.apex_m,
                        to: starts.get(k + 1).copied().unwrap_or(length_m),
                    })
                    .collect()
            }
        };
        // Trail braking needs the IMU's forward g (the role's first name), not the GPS's.
        let has_trail = forward.name == Role::ForwardG.names()[0];
        let stats: Vec<Vec<Stats>> = traces
            .iter()
            .map(|tr| {
                segments
                    .iter()
                    .enumerate()
                    .map(|(k, s)| corner_stats(tr, &segments, k, s, has_trail))
                    .collect()
            })
            .collect();
        let racing: Vec<usize> = (0..traces.len()).filter(|&i| trace_counts[i]).collect();
        let best_of = (0..segments.len())
            .map(|k| {
                racing
                    .iter()
                    .copied()
                    .min_by(|&a, &b| stats[a][k].time_s.total_cmp(&stats[b][k].time_s))
                    .unwrap_or(best_trace)
            })
            .collect();
        Some(Self {
            length_m,
            segments,
            from_track,
            origin,
            traces,
            stats,
            counts: trace_counts,
            best_of,
            best: best_trace,
            has_trail,
        })
    }

    /// These corners, to save as the track's: positions along the best lap.
    pub fn to_saved(&self, session: String) -> TrackCorners {
        let tr = &self.traces[self.best];
        let at = |p: (f64, f64)| from_local(self.origin.0, self.origin.1, p);
        // The best lap every STEP_M (without the shorter last step).
        let n = (self.length_m / STEP_M) as usize;
        let line: Vec<(f64, f64)> = tr.xy[..=n.min(tr.xy.len() - 1)]
            .iter()
            .map(|&p| at(p))
            .collect();
        let last = line.len() - 1;
        let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
        let corners = self
            .segments
            .iter()
            .map(|s| SavedCorner {
                from: line[idx(s.from)],
                apex: line[idx(s.apex)],
                from_m: s.from,
                apex_m: s.apex,
            })
            .collect();
        TrackCorners::new(session, tr.number, line, corners)
    }

    /// The best of every corner: the fastest of each, added up.
    pub fn ideal_s(&self) -> f64 {
        self.best_of
            .iter()
            .enumerate()
            .map(|(k, &b)| self.stats[b][k].time_s)
            .sum()
    }

    /// How far `trace` is off the best through corner `k` (s).
    pub fn loss(&self, trace: usize, k: usize) -> f64 {
        self.stats[trace][k].time_s - self.stats[self.best_of[k]][k].time_s
    }

    /// The corner where `trace` lost the most against the best.
    pub fn worst(&self, trace: usize) -> usize {
        (0..self.segments.len())
            .max_by(|&a, &b| self.loss(trace, a).total_cmp(&self.loss(trace, b)))
            .unwrap_or(0)
    }

    /// When `trace` went through corner `k`: seconds from its lap's start, (in, out).
    pub fn span_s(&self, trace: usize, k: usize) -> (f64, f64) {
        let tr = &self.traces[trace];
        let last = tr.t.len() - 1;
        let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
        let s = &self.segments[k];
        let end = if k + 1 == self.segments.len() {
            last
        } else {
            idx(s.to)
        };
        (tr.t[idx(s.from)], tr.t[end])
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
            let (d_min, d_exit) = (
                speed(a.min_kmh) - speed(b.min_kmh),
                speed(a.exit_kmh) - speed(b.exit_kmh),
            );
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
            out.push((
                Tone::Attention,
                if why.is_empty() {
                    format!("{d:.2} s off the best")
                } else {
                    format!("{d:.2} s off the best: {}", why.join(", "))
                },
            ));
        }
        if a.brake_s < 0.05 && a.entry_kmh - a.min_kmh >= 3.0 {
            out.push((Tone::Plain, "Slows without braking: lift or slide".into()));
        } else if let Some(t) = a.trail_s.filter(|&t| t >= 0.6) {
            out.push((Tone::Plain, format!("Trail brakes {t:.1} s")));
        }
        if out.len() < 2 && a.steering.is_some_and(|s| s.1 >= 3) {
            out.push((
                Tone::Attention,
                format!(
                    "{} steering corrections",
                    a.steering.map(|s| s.1).unwrap_or(0)
                ),
            ));
        }
        out.truncate(2);
        out
    }
}

/// The saved corners as segments along their own line.
fn saved_segments(saved: &TrackCorners) -> Vec<Segment> {
    let n = saved.corners.len();
    (0..n)
        .map(|k| Segment {
            from: if k == 0 { 0.0 } else { saved.corners[k].from_m },
            apex: saved.corners[k].apex_m,
            to: saved
                .corners
                .get(k + 1)
                .map_or(saved.length_m(), |c| c.from_m),
        })
        .collect()
}

/// `values` at time `t` (ms), linearly between samples, clamped at the ends.
fn interp_t(times_ms: &[i64], values: &[f64], t: f64) -> f64 {
    let i = times_ms.partition_point(|&x| (x as f64) < t);
    if i == 0 {
        return values[0];
    }
    if i >= times_ms.len() {
        return values[values.len() - 1];
    }
    let (t0, t1) = (times_ms[i - 1] as f64, times_ms[i] as f64);
    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
    values[i - 1] + f * (values[i] - values[i - 1])
}

/// The saved corners on a best lap (its path `xy`, every [`STEP_M`] from the
/// line, in meters around `origin`). None when the lap doesn't follow the
/// saved line (another layout, or GPS too far off) or the corners don't fall
/// in order along it.
fn place(
    saved: &TrackCorners,
    xy: &[(f64, f64)],
    origin: (f64, f64),
    length_m: f64,
) -> Option<Vec<Segment>> {
    if saved.corners.is_empty() || xy.is_empty() {
        return None;
    }
    let scale = length_m / saved.length_m();
    if !(0.9..=1.1).contains(&scale) {
        return None;
    }
    let last = xy.len() - 1;
    // The point of the lap nearest `p`, looking around where its distance
    // along the saved line puts it: (meters along the lap, how far off).
    let nearest = |p: (f64, f64), saved_m: f64| {
        let (x, y) = local_xy(origin.0, origin.1, p.0, p.1);
        let m = saved_m * scale;
        let lo = ((m - REACH_M) / STEP_M).max(0.0) as usize;
        let hi = (((m + REACH_M) / STEP_M) as usize).min(last);
        (lo.min(hi)..=hi)
            .map(|i| (i as f64 * STEP_M, (xy[i].0 - x).hypot(xy[i].1 - y)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((m, f64::INFINITY))
    };
    // The lap follows the line: every 10 m of it, typically within FIT_M.
    let mut off: Vec<f64> = saved
        .line
        .iter()
        .enumerate()
        .step_by(5)
        .map(|(i, &p)| nearest(p, i as f64 * STEP_M).1)
        .collect();
    off.sort_by(f64::total_cmp);
    let typical = off[off.len() / 2];
    let worst = off[off.len() * 9 / 10];
    log::debug!("corners: the best lap is {typical:.1} m off the track's line (90%: {worst:.1} m)");
    if typical > FIT_M || worst > FIT_MAX_M {
        return None;
    }
    let mut points = Vec::with_capacity(saved.corners.len() * 2);
    for (k, c) in saved.corners.iter().enumerate() {
        let from = if k == 0 {
            (0.0, 0.0)
        } else {
            nearest(c.from, c.from_m)
        };
        let apex = nearest(c.apex, c.apex_m);
        if from.1 > FIT_MAX_M || apex.1 > FIT_MAX_M {
            return None;
        }
        points.push(from.0);
        points.push(apex.0);
    }
    // Braking point, apex, next braking point… one after another along the lap.
    if points.windows(2).any(|w| w[1] <= w[0]) || points[points.len() - 1] >= length_m {
        return None;
    }
    Some(
        (0..saved.corners.len())
            .map(|k| Segment {
                from: points[2 * k],
                apex: points[2 * k + 1],
                to: points.get(2 * k + 2).copied().unwrap_or(length_m),
            })
            .collect(),
    )
}

/// One trace through corner `k`.
fn corner_stats(tr: &Trace, segments: &[Segment], k: usize, s: &Segment, has_trail: bool) -> Stats {
    let last = tr.kmh.len() - 1;
    let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
    // The last corner runs to the lap's very end (the trace's last sample).
    let end = if k + 1 == segments.len() {
        last
    } else {
        idx(s.to)
    };
    let time_s = tr.t[end] - tr.t[idx(s.from)];
    // Braking point: the top speed before the corner, looking no further back
    // than just past the previous corner's apex.
    let back = if k > 0 {
        (segments[k - 1].apex + 4.0).max(s.from - 30.0)
    } else {
        0.0
    };
    let (lo, hi) = (idx(back), idx(s.apex).max(idx(back)));
    let bi = (lo..=hi)
        .max_by(|&a, &b| tr.kmh[a].total_cmp(&tr.kmh[b]))
        .unwrap_or(lo);
    let mi = (bi..=idx(s.to.min(s.apex + 25.0)).max(bi))
        .min_by(|&a, &b| tr.kmh[a].total_cmp(&tr.kmh[b]))
        .unwrap_or(bi);
    let exit_kmh = (mi..=idx(s.to).max(mi))
        .map(|i| tr.kmh[i])
        .fold(0.0, f64::max);
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
        (
            w.iter().fold(0.0f64, |m, x| m.max(x.abs())),
            crate::report::reversals(w, 5.0),
        )
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
        let mut tr = Trace {
            lap: 1,
            number: 2,
            t: vec![],
            kmh: vec![],
            forward: vec![],
            cornering: vec![],
            steering: None,
            xy: vec![],
        };
        let mut t = 0.0;
        for k in 0..=n {
            let m = k as f64 * STEP_M;
            let v = if m < brake_at {
                40.0
            } else if m < 50.0 {
                40.0 - 20.0 * (m - brake_at) / (50.0 - brake_at)
            } else if m < 100.0 {
                20.0 + 20.0 * (m - 50.0) / 50.0
            } else {
                40.0
            };
            tr.t.push(t);
            t += STEP_M / (v / 3.6);
            tr.kmh.push(v);
            tr.forward.push(if (brake_at..50.0).contains(&m) {
                -0.4
            } else {
                0.0
            });
            tr.cornering
                .push(if (40.0..60.0).contains(&m) { 0.5 } else { 0.0 });
            tr.xy.push((m, 0.0));
        }
        tr
    }

    #[test]
    fn a_corner_reads_as_braking_then_trail() {
        let tr = trace(30.0);
        let segs = [Segment {
            from: 0.0,
            apex: 50.0,
            to: 200.0,
        }];
        let s = corner_stats(&tr, &segs, 0, &segs[0], true);
        assert_eq!(
            (s.brake_m, s.entry_kmh, s.min_kmh, s.min_m),
            (30.0, 40.0, 20.0, 50.0)
        );
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
        let segs = [
            Segment {
                from: 0.0,
                apex: 50.0,
                to: 120.0,
            },
            Segment {
                from: 120.0,
                apex: 160.0,
                to: 200.8,
            },
        ];
        let total: f64 = (0..2)
            .map(|k| corner_stats(&tr, &segs, k, &segs[k], true).time_s)
            .sum();
        assert!((total - end).abs() < 1e-9, "{total} vs {end}");
    }

    /// A round track, 50 m across: its path every STEP_M from the line, in
    /// meters, shifted `dx` east.
    fn round(dx: f64) -> Vec<(f64, f64)> {
        let n = (std::f64::consts::TAU * 50.0 / STEP_M) as usize;
        (0..=n)
            .map(|i| {
                let a = i as f64 * STEP_M / 50.0;
                (50.0 * a.sin() + dx, 50.0 * (1.0 - a.cos()))
            })
            .collect()
    }

    /// Corners at 0/50, 100/150 and 200/250 m on the round track, saved.
    fn saved_round(origin: (f64, f64)) -> TrackCorners {
        let xy = round(0.0);
        let at = |m: f64| from_local(origin.0, origin.1, xy[(m / STEP_M) as usize]);
        let corners = [(0.0, 50.0), (100.0, 150.0), (200.0, 250.0)]
            .map(|(f, a)| SavedCorner {
                from: at(f),
                apex: at(a),
                from_m: f,
                apex_m: a,
            })
            .to_vec();
        TrackCorners::new(
            "a_0001.xrz".into(),
            3,
            xy.iter()
                .map(|&p| from_local(origin.0, origin.1, p))
                .collect(),
            corners,
        )
    }

    #[test]
    fn saved_corners_land_where_they_were() {
        let origin = (41.0, -73.0);
        let saved = saved_round(origin);
        let xy = round(0.0);
        let length = (xy.len() - 1) as f64 * STEP_M;
        let segs = place(&saved, &xy, origin, length).unwrap();
        let at: Vec<(f64, f64)> = segs.iter().map(|s| (s.from, s.apex)).collect();
        assert_eq!(at, [(0.0, 50.0), (100.0, 150.0), (200.0, 250.0)]);
        assert_eq!(segs[2].to, length);
        // GPS a few meters off on another day: the same corners.
        let off = round(3.0);
        assert_eq!(place(&saved, &off, origin, length).unwrap().len(), 3);
    }

    #[test]
    fn another_layout_keeps_its_own_corners() {
        let origin = (41.0, -73.0);
        let saved = saved_round(origin);
        let xy = round(0.0);
        let length = (xy.len() - 1) as f64 * STEP_M;
        // 30 m away: not this line.
        assert!(place(&saved, &round(30.0), origin, length).is_none());
        // A lap a fifth longer: not this track.
        assert!(place(&saved, &xy, origin, length * 1.2).is_none());
    }
}
