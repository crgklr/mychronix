//! The GPS receiver's speed and heading run behind its own positions.
//!
//! A receiver works its velocity out through a filter of its own, and on
//! every AiM logger measured so far the speed it reports trails the speed its
//! positions show, steady through a session: 63–148 ms on 58 kart sessions
//! (MyChron 6 median 79 ms), 79–102 ms on other AiM loggers at 25 Hz, and
//! 130 ms on an early MyChron 5 at 8 Hz (Nov 2026). Everything found from GPS
//! speed then sat late against where the kart was: a corner's braking point,
//! found at the top of the speed trace, landed a meter or two past the real
//! one on the map, and the distance into a lap (added up from speed) ran a
//! little behind.
//!
//! So each session's lag is measured, by lining up the speed worked out from
//! the positions with the receiver's (and the same for the turn rate), and the
//! receiver's speed and turn rate are moved earlier by it, once, as the
//! session is read. Positions stay as they are, and so do laps, whether the
//! logger timed them or they're timed again from the positions. When the lag
//! can't be measured cleanly nothing moves.
//!
//! The positions aren't moved against the logger's own channels. On the
//! MyChron 6 they line up with the gyro's turning (to within the
//! measurement's scatter, about ±30 ms a session); its accelerometers and RPM
//! run later than both, by their own filtering, so lining GPS up with those
//! would put the map out against the gyro instead (see
//! `docs/protocol-notes.md`, "GPS timing").

use crate::GpsTrack;

/// The common time step both signals are resampled to (ms).
const STEP_MS: i64 = 10;
/// Where the best match is looked for (ms; positive: the second signal late).
/// A little wider than what's accepted, so a match at the edge shows up as
/// one instead of passing for a peak.
const SEARCH_MS: (i64, i64) = (-50, 300);
/// The lags accepted (ms).
pub const ACCEPTED_MS: (i64, i64) = (0, 250);
/// The lags believable from a receiver giving 20 or more fixes a second
/// (ms): real ones measure 69–117 (MyChron 6 and others at 25 Hz). One short
/// run measured 148 cleanly but alone, and is left as recorded.
pub const FAST_RECEIVER_MS: (i64, i64) = (40, 130);
/// Both signals are smoothed alike, this much either side (ms), so the
/// position-derived one's jitter doesn't decide the match. The same smoothing
/// on both can't move the match.
const SMOOTH_HALF_MS: i64 = 100;
/// Only stretches faster than this count (km/h): standing still, heading and
/// position jitter say nothing.
const MOVING_KMH: f64 = 15.0;
/// At least this much driving to go on (ms).
const MIN_MOVING_MS: i64 = 60_000;
/// The match must be at least this good (correlation). The two speeds score
/// 0.98–1.00 on real sessions, the turn rates 0.93–0.99; a weaker match
/// means a damaged or jumpy track.
pub const MIN_CORRELATION: f64 = 0.9;
/// The first and second halves of the session must agree on the lag to this
/// much (ms): the peak is then a real feature, not noise. Real sessions'
/// speed halves mostly agree to within 10 ms.
pub const HALVES_AGREE_MS: f64 = 25.0;

/// A measured lag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lag {
    /// How late (ms).
    pub ms: i64,
    /// How well the signals match once lined up (correlation, up to 1).
    pub correlation: f64,
}

/// Why a lag wasn't measured (for the log).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoLag {
    /// Less than `MIN_MOVING_MS` of driving.
    TooLittleDriving,
    /// The best match is too poor (its correlation).
    Weak(f64),
    /// The best match is outside [`ACCEPTED_MS`] (ms).
    OutOfRange(i64),
    /// The session's two halves disagree (their lags, ms).
    Unsteady(i64, i64),
}

/// `values` sampled every [`STEP_MS`] from `from`, linearly between points;
/// NaN outside the samples and across a gap of over a second.
fn resample(times_ms: &[i64], values: &[f64], from: i64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|k| {
            let t = from + k as i64 * STEP_MS;
            let i = times_ms.partition_point(|&x| x < t);
            if i < times_ms.len() && times_ms[i] == t {
                return values[i];
            }
            if i == 0 || i >= times_ms.len() || times_ms[i] - times_ms[i - 1] > 1000 {
                return f64::NAN;
            }
            let (t0, t1) = (times_ms[i - 1] as f64, times_ms[i] as f64);
            let f = (t as f64 - t0) / (t1 - t0);
            values[i - 1] + f * (values[i] - values[i - 1])
        })
        .collect()
}

/// Moving average over `half` points either side; NaN where the window runs
/// off the ends or holds a NaN.
fn smooth(v: &[f64], half: usize) -> Vec<f64> {
    let mut sums = Vec::with_capacity(v.len() + 1);
    sums.push(0.0);
    let mut bad = Vec::with_capacity(v.len() + 1);
    bad.push(0usize);
    for &x in v {
        let (s, b) = (*sums.last().unwrap(), *bad.last().unwrap());
        if x.is_finite() {
            sums.push(s + x);
            bad.push(b);
        } else {
            sums.push(s);
            bad.push(b + 1);
        }
    }
    (0..v.len())
        .map(|i| {
            if i < half || i + half >= v.len() || bad[i + half + 1] != bad[i - half] {
                f64::NAN
            } else {
                (sums[i + half + 1] - sums[i - half]) / (2 * half + 1) as f64
            }
        })
        .collect()
}

/// Correlation of `a[i]` with `b[i + shift]` over the moving points of `range`.
fn correlation(
    a: &[f64],
    b: &[f64],
    moving: &[bool],
    shift: i64,
    range: std::ops::Range<usize>,
) -> Option<(f64, usize)> {
    let (mut n, mut sa, mut sb, mut saa, mut sbb, mut sab) = (0usize, 0.0, 0.0, 0.0, 0.0, 0.0);
    for i in range {
        let j = i as i64 + shift;
        if !moving[i] || j < 0 || j as usize >= b.len() {
            continue;
        }
        let (x, y) = (a[i], b[j as usize]);
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        n += 1;
        sa += x;
        sb += y;
        saa += x * x;
        sbb += y * y;
        sab += x * y;
    }
    let k = n as f64;
    let (va, vb) = (saa - sa * sa / k, sbb - sb * sb / k);
    (n >= 2 && va > 0.0 && vb > 0.0).then(|| ((sab - sa * sb / k) / (va * vb).sqrt(), n))
}

/// The best-matching lag over `range` (ms, refined between steps) and its
/// correlation; None with too little to go on or a peak at the search's edge.
fn best_lag(
    a: &[f64],
    b: &[f64],
    moving: &[bool],
    range: std::ops::Range<usize>,
    min_points: usize,
) -> Result<(f64, f64), NoLag> {
    let shifts: Vec<i64> = (SEARCH_MS.0 / STEP_MS..=SEARCH_MS.1 / STEP_MS).collect();
    let mut scores = Vec::with_capacity(shifts.len());
    for &s in &shifts {
        match correlation(a, b, moving, s, range.clone()) {
            Some((r, n)) if n >= min_points => scores.push(r),
            _ => return Err(NoLag::TooLittleDriving),
        }
    }
    let (k, &r) = scores
        .iter()
        .enumerate()
        .max_by(|x, y| x.1.total_cmp(y.1))
        .expect("at least one shift");
    if k == 0 || k + 1 == scores.len() {
        return Err(NoLag::OutOfRange(shifts[k] * STEP_MS));
    }
    // A parabola through the peak and its neighbors places it between steps.
    let (y0, y2) = (scores[k - 1], scores[k + 1]);
    let curve = y0 - 2.0 * r + y2;
    let nudge = if curve < 0.0 {
        (0.5 * (y0 - y2) / curve).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Ok(((shifts[k] as f64 + nudge) * STEP_MS as f64, r))
}

/// How late `late` runs behind `reference` (both sampled at `times_ms`, with
/// the kart's speed `speed_kmh` there): the shift that best lines them up
/// while moving, if the match is good and clear (see [`MIN_CORRELATION`],
/// [`HALVES_AGREE_MS`] and [`ACCEPTED_MS`]).
pub fn estimate(
    times_ms: &[i64],
    reference: &[f64],
    late: &[f64],
    speed_kmh: &[f64],
) -> Result<Lag, NoLag> {
    let n_in = times_ms
        .len()
        .min(reference.len())
        .min(late.len())
        .min(speed_kmh.len());
    if n_in < 2 {
        return Err(NoLag::TooLittleDriving);
    }
    let t = &times_ms[..n_in];
    let from = t[0];
    let n = ((t[n_in - 1] - from) / STEP_MS + 1) as usize;
    let half = (SMOOTH_HALF_MS / STEP_MS) as usize;
    let a = smooth(&resample(t, &reference[..n_in], from, n), half);
    let b = smooth(&resample(t, &late[..n_in], from, n), half);
    let moving: Vec<bool> = resample(t, &speed_kmh[..n_in], from, n)
        .iter()
        .map(|&v| v >= MOVING_KMH)
        .collect();
    let min_points = (MIN_MOVING_MS / STEP_MS) as usize;
    let (ms, r) = best_lag(&a, &b, &moving, 0..n, min_points)?;
    let rounded = ms.round() as i64;
    if !(ACCEPTED_MS.0..=ACCEPTED_MS.1).contains(&rounded) {
        return Err(NoLag::OutOfRange(rounded));
    }
    if r < MIN_CORRELATION {
        return Err(NoLag::Weak(r));
    }
    let (first, _) = best_lag(&a, &b, &moving, 0..n / 2, min_points / 4)?;
    let (second, _) = best_lag(&a, &b, &moving, n / 2..n, min_points / 4)?;
    if (first - second).abs() > HALVES_AGREE_MS {
        return Err(NoLag::Unsteady(first.round() as i64, second.round() as i64));
    }
    Ok(Lag {
        ms: rounded,
        correlation: r,
    })
}

/// Speed (km/h) and turn rate (degrees per second, clockwise positive) worked
/// out from the positions alone, each centered on its point.
fn from_positions(g: &GpsTrack) -> (Vec<f64>, Vec<f64>) {
    let n = g.len();
    let (mut speed, mut heading, mut yaw) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    let meters = |a: usize, b: usize| {
        let r = 6_371_000.0;
        let lat = ((g.latitude[a] + g.latitude[b]) / 2.0).to_radians();
        (
            (g.longitude[b] - g.longitude[a]).to_radians() * r * lat.cos(),
            (g.latitude[b] - g.latitude[a]).to_radians() * r,
        )
    };
    for i in 1..n.saturating_sub(1) {
        let dt = (g.times_ms[i + 1] - g.times_ms[i - 1]) as f64 / 1000.0;
        let (de, dn) = meters(i - 1, i + 1);
        speed[i] = if dt > 0.0 {
            de.hypot(dn) / dt * 3.6
        } else {
            0.0
        };
        heading[i] = de.atan2(dn).to_degrees();
    }
    for i in 2..n.saturating_sub(2) {
        let dt = (g.times_ms[i + 1] - g.times_ms[i - 1]) as f64 / 1000.0;
        let dh = (heading[i + 1] - heading[i - 1] + 540.0).rem_euclid(360.0) - 180.0;
        yaw[i] = if dt > 0.0 { dh / dt } else { 0.0 };
    }
    (speed, yaw)
}

/// `values` (sampled at `times_ms`) read `lag_ms` later at every point: moved
/// earlier by the lag. The last points hold the final value.
fn moved_earlier(times_ms: &[i64], values: &[f64], lag_ms: i64) -> Vec<f64> {
    times_ms
        .iter()
        .enumerate()
        .map(|(k, &t)| {
            let t = t + lag_ms;
            let i = times_ms.partition_point(|&x| x < t);
            if i >= times_ms.len() {
                return *values.last().unwrap_or(&0.0);
            }
            if i == 0 || times_ms[i] == t {
                return values[i];
            }
            // Not across a stretch the receiver didn't record: as it was.
            if times_ms[i] - times_ms[i - 1] > 1000 {
                return values[k];
            }
            let (t0, t1) = (times_ms[i - 1] as f64, times_ms[i] as f64);
            let f = (t as f64 - t0) / (t1 - t0);
            values[i - 1] + f * (values[i] - values[i - 1])
        })
        .collect()
}

/// What [`align`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aligned {
    /// The receiver's speed was this late, and moved earlier by it.
    pub speed: Lag,
    /// And its turn rate this much (ms): measured the same way, else the
    /// speed's lag.
    pub yaw_ms: i64,
}

/// Measures how late `track`'s speed and turn rate run behind its positions
/// and moves them earlier by it. An error (and nothing changed) when the
/// speed's lag can't be measured cleanly.
pub fn align(track: &mut GpsTrack) -> Result<Aligned, NoLag> {
    let (speed, yaw) = from_positions(track);
    let lag = estimate(&track.times_ms, &speed, &track.speed_kmh, &track.speed_kmh)?;
    // A fast receiver's lag outside what such receivers show is a one-off,
    // however clean the match: leave that session alone.
    let mut steps: Vec<i64> = track.times_ms.windows(2).map(|w| w[1] - w[0]).collect();
    steps.sort_unstable();
    let fast = steps.get(steps.len() / 2).is_some_and(|&s| s <= 50);
    if fast && !(FAST_RECEIVER_MS.0..=FAST_RECEIVER_MS.1).contains(&lag.ms) {
        return Err(NoLag::OutOfRange(lag.ms));
    }
    let yaw_ms = match estimate(&track.times_ms, &yaw, &track.yaw_rate_dps, &track.speed_kmh) {
        Ok(y) => y.ms,
        Err(why) => {
            log::debug!("GPS: turn rate's own lag not measured ({why:?}); the speed's used");
            lag.ms
        }
    };
    track.speed_kmh = moved_earlier(&track.times_ms, &track.speed_kmh, lag.ms);
    track.yaw_rate_dps = moved_earlier(&track.times_ms, &track.yaw_rate_dps, yaw_ms);
    Ok(Aligned { speed: lag, yaw_ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kart lapping a 40 m-radius circle at a speed that swells and fades in
    /// an uneven rhythm, sampled at `hz`, with the receiver's speed and turn
    /// rate `lag_ms` late and `noise` km/h of jitter on them.
    fn lapping(hz: i64, lag_ms: i64, noise: f64) -> GpsTrack {
        let speed_at = |t: f64| {
            60.0 + 25.0 * (t / 3.1).sin() + 12.0 * (t / 1.3 + 0.7).sin() + 6.0 * (t / 0.7).cos()
        };
        let mut g = GpsTrack::default();
        let (mut angle, mut seed) = (0.0f64, 12345u64);
        let mut rand = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        };
        let step = 1000 / hz;
        let r = 40.0;
        for k in 0..(300 * hz) {
            let t = k * step;
            let ts = t as f64 / 1000.0;
            // Positions on time; the angle advances with the true speed (at
            // the middle of each step, so the positions don't run ahead).
            if k > 0 {
                let mid = ts - step as f64 / 2000.0;
                angle += speed_at(mid) / 3.6 * step as f64 / 1000.0 / r;
            }
            g.times_ms.push(t);
            g.latitude
                .push(45.0 + r * angle.cos() / 6_371_000.0 * 57.29578);
            g.longitude
                .push(10.0 + r * angle.sin() / (6_371_000.0 * 45f64.to_radians().cos()) * 57.29578);
            let late = (ts - lag_ms as f64 / 1000.0).max(0.0);
            let v = speed_at(late) + noise * rand();
            g.speed_kmh.push(v);
            g.yaw_rate_dps.push((v / 3.6 / r).to_degrees());
        }
        g
    }

    #[test]
    fn a_known_lag_is_found() {
        for (hz, lag) in [(25, 70), (25, 0), (10, 120), (25, 200), (8, 90)] {
            let mut g = lapping(hz, lag, 0.3);
            let (speed, _) = from_positions(&g);
            let found = estimate(&g.times_ms, &speed, &g.speed_kmh, &g.speed_kmh)
                .unwrap_or_else(|e| panic!("{hz} Hz, {lag} ms: {e:?}"));
            assert!(
                (found.ms - lag).abs() <= 10,
                "{hz} Hz: {lag} ms read as {found:?}"
            );
            assert!(found.correlation > 0.95);
            // Moved only when believable for the receiver's rate.
            let believable = hz < 20 || (FAST_RECEIVER_MS.0..=FAST_RECEIVER_MS.1).contains(&lag);
            match align(&mut g) {
                Ok(a) => {
                    assert!(believable, "{hz} Hz, {lag} ms moved");
                    assert!((a.speed.ms - lag).abs() <= 10 && (a.yaw_ms - lag).abs() <= 10);
                }
                Err(NoLag::OutOfRange(_)) => assert!(!believable, "{hz} Hz, {lag} ms left"),
                Err(e) => panic!("{hz} Hz, {lag} ms: {e:?}"),
            }
        }
    }

    #[test]
    fn aligning_twice_moves_nothing_more() {
        let mut g = lapping(25, 80, 0.3);
        let first = align(&mut g).expect("aligned");
        assert!((first.speed.ms - 80).abs() <= 10);
        // Once moved, the speed matches the positions as it is.
        let (speed, _) = from_positions(&g);
        let again = estimate(&g.times_ms, &speed, &g.speed_kmh, &g.speed_kmh).expect("matched");
        assert!(again.ms.abs() <= 10, "{again:?}");
    }

    #[test]
    fn a_receiver_speed_that_doesnt_follow_the_positions_gives_nothing() {
        let mut g = lapping(25, 70, 0.3);
        // Speed that's mostly jitter.
        let mut seed = 99u64;
        for v in &mut g.speed_kmh {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *v = 40.0 + 30.0 * ((seed >> 33) as f64 / (1u64 << 31) as f64);
        }
        let before = g.clone();
        assert!(matches!(
            align(&mut g),
            Err(NoLag::Weak(_) | NoLag::OutOfRange(_) | NoLag::Unsteady(..))
        ));
        assert_eq!(g.speed_kmh, before.speed_kmh);
    }

    #[test]
    fn a_lag_beyond_the_accepted_range_gives_nothing() {
        let g = lapping(25, 400, 0.3);
        let (speed, _) = from_positions(&g);
        assert!(matches!(
            estimate(&g.times_ms, &speed, &g.speed_kmh, &g.speed_kmh),
            Err(NoLag::OutOfRange(_))
        ));
    }

    #[test]
    fn too_short_a_drive_gives_nothing() {
        let mut g = lapping(25, 70, 0.3);
        let keep = 25 * 40; // 40 s
        g.times_ms.truncate(keep);
        g.latitude.truncate(keep);
        g.longitude.truncate(keep);
        g.speed_kmh.truncate(keep);
        g.yaw_rate_dps.truncate(keep);
        assert_eq!(align(&mut g), Err(NoLag::TooLittleDriving));
    }

    #[test]
    fn moving_earlier_reads_later_values() {
        let t = [0, 40, 80, 120];
        let v = [0.0, 4.0, 8.0, 12.0];
        assert_eq!(moved_earlier(&t, &v, 20), vec![2.0, 6.0, 10.0, 12.0]);
        assert_eq!(moved_earlier(&t, &v, 0), v.to_vec());
    }
}
