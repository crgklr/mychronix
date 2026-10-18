//! The MyChron 6's built-in IMU, turned into the kart's own directions.
//!
//! The accelerometers measure along the logger's axes, and a MyChron is
//! mounted at whatever angle its bracket holds it (often leaned back 20–30°),
//! so its X, Y and Z mix braking, cornering and gravity. Here the mounting is
//! worked out from the session itself, with nothing to set:
//!
//! - **Up** is the direction of gravity while the kart sits still.
//! - **Forward** is the level direction whose acceleration best matches the
//!   GPS's speeding up and slowing down.
//!
//! Only forward and vertical acceleration are given: in corners the chassis
//! rolls, so the IMU's sideways reading falls about 20% short of the path's
//! cornering g (Oct 2026, a MyChron 6 against its own GPS), which stays the
//! measure there. A logger on the steering wheel turns with it, and its gyro
//! gives the steering angle (see [`steering`]). And nothing is given unless the
//! result checks out against the GPS: no still moment to find gravity, or a
//! poor match, and the session simply has no IMU channels.

use crate::{Channel, GpsTrack};

type V = [f64; 3];

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: V) -> V {
    let n = dot(a, a).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}

/// Stopped: GPS speed under this (km/h).
const STILL_KMH: f64 = 1.0;
/// At least this long stopped to trust the gravity reading (ms).
const STILL_MS: i64 = 5_000;
/// Moving fast enough for the GPS's acceleration to be meaningful (km/h).
const MOVING_KMH: f64 = 15.0;
/// The forward direction must explain at least this much of the GPS's
/// acceleration (R²); a MyChron 6 on its own data scores 0.76–0.79.
const MIN_FIT: f64 = 0.6;

/// How the logger sits on the kart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mount {
    /// How far the logger leans from level, degrees.
    pub tilt_deg: f64,
    /// Gravity's strength as the logger reads it, g (about 1).
    gravity: f64,
    up: V,
    forward: V,
    /// What the logger reads forward on average beyond the GPS (a small
    /// fixed bias, about 0.04 g), taken off so a lap averages out to zero.
    forward_bias: f64,
    /// How well forward acceleration matches the GPS's (R², 0–1).
    pub fit: f64,
}

/// The value of `c` at or just before `t`.
fn at(c: &[(i64, f64)], t: i64) -> f64 {
    let i = c.partition_point(|&(x, _)| x <= t).saturating_sub(1);
    c.get(i).map(|p| p.1).unwrap_or(0.0)
}

/// Why a session's mount couldn't be worked out (for the log).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoMount {
    TooShort,
    NeverStill,
    OddGravity(f64),
    TooLittleDriving,
    PoorFit(f64),
}

/// Works out the mount from the accelerometers (X, Y, Z, same timebase) and
/// the GPS's speed and forward acceleration (`gps_lon_g`, one per GPS point).
pub fn find_mount(accel: [&Channel; 3], gps: &GpsTrack, gps_lon_g: &[f64]) -> Result<Mount, NoMount> {
    let [x, y, z] = accel;
    let n = x.values.len().min(y.values.len()).min(z.values.len());
    if n < 100 || gps.len() < 100 || gps_lon_g.len() != gps.len() {
        return Err(NoMount::TooShort);
    }
    let speed: Vec<(i64, f64)> = gps.times_ms.iter().copied().zip(gps.speed_kmh.iter().copied()).collect();
    let lon: Vec<(i64, f64)> = gps.times_ms.iter().copied().zip(gps_lon_g.iter().copied()).collect();
    let a = |i: usize| [x.values[i], y.values[i], z.values[i]];

    // Up: gravity, averaged over every moment the kart was still.
    let (mut sum, mut still_ms, mut count) = ([0.0; 3], 0i64, 0usize);
    for i in 0..n {
        if at(&speed, x.times_ms[i]) < STILL_KMH {
            let v = a(i);
            (0..3).for_each(|k| sum[k] += v[k]);
            count += 1;
            if i > 0 {
                still_ms += (x.times_ms[i] - x.times_ms[i - 1]).min(100);
            }
        }
    }
    if still_ms < STILL_MS {
        return Err(NoMount::NeverStill);
    }
    let g = [sum[0] / count as f64, sum[1] / count as f64, sum[2] / count as f64];
    let gravity = dot(g, g).sqrt();
    if !(0.85..=1.15).contains(&gravity) {
        return Err(NoMount::OddGravity(gravity));
    }
    let up = unit(g);

    // Forward: the level direction that best matches the GPS (least squares
    // over two level axes, then its angle).
    let e1 = unit(cross(up, if up[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
    let e2 = cross(up, e1);
    let rows: Vec<(f64, f64, f64)> = (0..n)
        .filter(|&i| at(&speed, x.times_ms[i]) >= MOVING_KMH)
        .map(|i| (dot(a(i), e1), dot(a(i), e2), at(&lon, x.times_ms[i])))
        .collect();
    if rows.len() < 500 {
        return Err(NoMount::TooLittleDriving);
    }
    let (mut s11, mut s12, mut s22, mut s1y, mut s2y) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(h1, h2, gl) in &rows {
        s11 += h1 * h1;
        s12 += h1 * h2;
        s22 += h2 * h2;
        s1y += h1 * gl;
        s2y += h2 * gl;
    }
    // A touch of damping keeps it solvable when one axis never moves.
    let damp = 1e-9 * (s11 + s22);
    let (s11, s22) = (s11 + damp, s22 + damp);
    let det = s11 * s22 - s12 * s12;
    if det <= 0.0 {
        return Err(NoMount::TooLittleDriving);
    }
    let (p, q) = ((s22 * s1y - s12 * s2y) / det, (s11 * s2y - s12 * s1y) / det);
    let angle = q.atan2(p);
    let forward = [0, 1, 2].map(|k| angle.cos() * e1[k] + angle.sin() * e2[k]);

    // How well it matches, smoothed to about the GPS's own smoothing.
    let raw: Vec<f64> = rows.iter().map(|&(h1, h2, _)| angle.cos() * h1 + angle.sin() * h2).collect();
    let smooth: Vec<f64> = (0..raw.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(4), (i + 5).min(raw.len()));
            raw[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect();
    let forward_bias = raw.iter().zip(&rows).map(|(f, r)| f - r.2).sum::<f64>() / rows.len() as f64;
    let smooth: Vec<f64> = smooth.iter().map(|v| v - forward_bias).collect();
    let mean = rows.iter().map(|r| r.2).sum::<f64>() / rows.len() as f64;
    let total: f64 = rows.iter().map(|r| (r.2 - mean).powi(2)).sum();
    let error: f64 = rows.iter().zip(&smooth).map(|(r, s)| (r.2 - s).powi(2)).sum();
    let fit = if total > 0.0 { 1.0 - error / total } else { 0.0 };
    if fit < MIN_FIT {
        return Err(NoMount::PoorFit(fit));
    }
    Ok(Mount { tilt_deg: up[2].abs().min(1.0).acos().to_degrees(), gravity, up, forward, forward_bias, fit })
}

/// "Forward g" (positive speeding up, negative braking) and "Vertical g"
/// (0 on level ground; bumps, curbs and hopping), at the IMU's own rate.
pub fn kart_channels(accel: [&Channel; 3], mount: &Mount) -> Vec<Channel> {
    let [x, y, z] = accel;
    let n = x.values.len().min(y.values.len()).min(z.values.len());
    let a = |i: usize| [x.values[i], y.values[i], z.values[i]];
    let channel = |name: &str, values: Vec<f64>| Channel {
        name: name.into(),
        group: Some("IMU".into()),
        units: "g".into(),
        decimals: 2,
        interpolate: true,
        times_ms: x.times_ms[..n].to_vec(),
        values,
    };
    vec![
        channel("Forward g", (0..n).map(|i| dot(a(i), mount.forward) - mount.forward_bias).collect()),
        channel("Vertical g", (0..n).map(|i| dot(a(i), mount.up) - mount.gravity).collect()),
    ]
}

/// GPS heading lags the IMU by about this much (ms), found by lining up the
/// gyro's turn rate with the GPS's on a MyChron 6 (Oct 2026).
const GPS_LAG_MS: i64 = 60;
/// The steering estimate must explain this much of how tightly the kart
/// turns (R²); a MyChron 6 on the steering wheel scores 0.65–0.76.
const MIN_STEER_FIT: f64 = 0.5;

/// "Steering": the steering wheel's angle, degrees (positive turning right),
/// for a MyChron mounted on the wheel, which turns with it. Worked out from
/// the gyro: its rotation, less the kart's own turning (from the GPS), is the
/// wheel turning; the column's direction is the one whose added-up rotation
/// best follows how tightly the kart turns, and the angle is set back to zero
/// on every straight so it can't drift. None when the logger doesn't turn
/// with the wheel (a fixed bracket) or the result doesn't check out.
pub fn steering(gyro: [&Channel; 3], mount: &Mount, gps: &GpsTrack) -> Option<Channel> {
    let [x, y, z] = gyro;
    let n = x.values.len().min(y.values.len()).min(z.values.len());
    if n < 500 || gps.len() < 100 {
        return None;
    }
    let t = &x.times_ms[..n];
    let gps_at = |v: &[f64], ms: i64| {
        let i = gps.times_ms.partition_point(|&q| q <= ms).saturating_sub(1);
        v[i.min(v.len() - 1)]
    };
    let speed: Vec<f64> = t.iter().map(|&ms| gps_at(&gps.speed_kmh, ms)).collect();
    let yaw: Vec<f64> = t.iter().map(|&ms| gps_at(&gps.yaw_rate_dps, ms + GPS_LAG_MS)).collect();
    let w = |i: usize| [x.values[i], y.values[i], z.values[i]];
    // The gyro's zero, while the kart sits still.
    let still: Vec<usize> = (0..n).filter(|&i| speed[i] < STILL_KMH).collect();
    if still.len() < 100 {
        return None;
    }
    let bias = [0, 1, 2].map(|k| still.iter().map(|&i| w(i)[k]).sum::<f64>() / still.len() as f64);
    // Rotation left once the kart's own turning is taken off (GPS yaw is
    // clockwise positive, a turn about −up), added up axis by axis.
    let mut total = vec![[0.0; 3]; n];
    for i in 1..n {
        let dt = (t[i] - t[i - 1]) as f64 / 1000.0;
        let wi = w(i);
        for k in 0..3 {
            total[i][k] = total[i - 1][k] + (wi[k] - bias[k] + yaw[i] * mount.up[k]) * dt;
        }
    }
    // Back to zero on straights: the drift between two straights is taken out
    // in a straight line.
    let straight: Vec<usize> = (0..n).filter(|&i| yaw[i].abs() < 3.0 && speed[i] > 20.0).collect();
    if straight.len() < 50 {
        return None;
    }
    let angle: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let j = straight.partition_point(|&q| q <= i);
            let (a, b) = (straight[j.saturating_sub(1)], straight[j.min(straight.len() - 1)]);
            let f = if b > a { (i as f64 - a as f64) / (b - a) as f64 } else { 0.0 };
            [0, 1, 2].map(|k| total[i][k] - (total[a][k] + f.clamp(0.0, 1.0) * (total[b][k] - total[a][k])))
        })
        .collect();
    // The column: the direction whose angle best follows the path's curvature.
    let moving: Vec<usize> = (0..n).filter(|&i| speed[i] > MOVING_KMH).collect();
    let curvature = |i: usize| yaw[i] / (speed[i] / 3.6);
    let mut m = [[0.0f64; 4]; 3];
    for &i in &moving {
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] += angle[i][r] * angle[i][c];
            }
            m[r][3] += angle[i][r] * curvature(i);
        }
    }
    for i in 0..3 {
        let p = m[i][i];
        if p.abs() < 1e-9 {
            return None;
        }
        m[i][i..].iter_mut().for_each(|v| *v /= p);
        let pivot = m[i];
        for r in (0..3).filter(|&r| r != i) {
            let f = m[r][i];
            m[r][i..].iter_mut().zip(&pivot[i..]).for_each(|(v, pv)| *v -= f * pv);
        }
    }
    let fit_w = [m[0][3], m[1][3], m[2][3]];
    let mean = moving.iter().map(|&i| curvature(i)).sum::<f64>() / moving.len() as f64;
    let total_var: f64 = moving.iter().map(|&i| (curvature(i) - mean).powi(2)).sum();
    let error: f64 = moving.iter().map(|&i| (curvature(i) - dot(fit_w, angle[i])).powi(2)).sum();
    let fit = 1.0 - error / total_var;
    if fit < MIN_STEER_FIT {
        log::debug!("IMU: no steering (it follows the kart's turning with R² {fit:.2} only)");
        return None;
    }
    let column = unit(fit_w);
    log::debug!("IMU: steering from the gyro, R² {fit:.2}, column {column:.2?}");
    Some(Channel {
        name: "Steering".into(),
        group: Some("IMU".into()),
        units: "deg".into(),
        decimals: 0,
        interpolate: true,
        times_ms: t.to_vec(),
        values: angle.iter().map(|a| dot(*a, column)).collect(),
    })
}

/// "Grip used": how much of the session's grip the kart is using, %, from
/// Forward g and the GPS's cornering g together (the length of the arrow on a
/// grip circle). 100 % is the session's own limit: what the kart reached
/// under hard braking or cornering, taken as the 98th percentile while it was
/// doing something (over 0.2 g), so a single bump doesn't set it.
pub fn grip_used(forward: &Channel, lat_times_ms: &[i64], lat_g: &[f64]) -> Option<Channel> {
    if lat_times_ms.len() < 2 || lat_g.len() != lat_times_ms.len() {
        return None;
    }
    let lat = |t: i64| {
        let i = lat_times_ms.partition_point(|&x| x <= t).saturating_sub(1);
        lat_g[i.min(lat_g.len() - 1)]
    };
    let combined: Vec<f64> = forward.times_ms.iter().zip(&forward.values).map(|(&t, &f)| (f * f + lat(t).powi(2)).sqrt()).collect();
    let mut busy: Vec<f64> = combined.iter().copied().filter(|&g| g > 0.2).collect();
    if busy.len() < 100 {
        return None;
    }
    busy.sort_by(f64::total_cmp);
    let limit = busy[busy.len() * 98 / 100];
    Some(Channel {
        name: "Grip used".into(),
        group: Some("IMU".into()),
        units: "%".into(),
        decimals: 0,
        interpolate: true,
        times_ms: forward.times_ms.clone(),
        values: combined.iter().map(|g| (g / limit * 100.0).min(120.0)).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A logger leaned back `tilt` degrees (about its Y axis): a kart that sits
    /// still for 10 s, then speeds up and brakes in a 4 s rhythm.
    fn drive(tilt: f64) -> ([Channel; 3], GpsTrack, Vec<f64>) {
        let (s, c) = tilt.to_radians().sin_cos();
        let mut acc = [vec![], vec![], vec![]];
        let mut times = vec![];
        let mut gps = GpsTrack::default();
        let mut lon = vec![];
        for i in 0..3000 {
            let t = i as i64 * 20;
            let moving = t >= 10_000;
            let f = if moving { 0.4 * ((t - 10_000) as f64 / 4000.0 * std::f64::consts::TAU).sin() } else { 0.0 };
            // Kart frame: forward f, up 1 g. Logger frame: rotated by the tilt.
            acc[0].push(f * c + s);
            acc[1].push(0.0);
            acc[2].push(-f * s + c);
            times.push(t);
            if i % 2 == 0 {
                gps.times_ms.push(t);
                gps.speed_kmh.push(if moving { 40.0 } else { 0.0 });
                gps.latitude.push(0.0);
                gps.longitude.push(0.0);
                gps.yaw_rate_dps.push(0.0);
                lon.push(f);
            }
        }
        let ch = |v: Vec<f64>| Channel { name: String::new(), group: None, units: "g".into(), decimals: 2, interpolate: true, times_ms: times.clone(), values: v };
        let [ax, ay, az] = acc;
        ([ch(ax), ch(ay), ch(az)], gps, lon)
    }

    #[test]
    fn a_leaned_back_logger_reads_true_forward_and_vertical() {
        let (acc, gps, lon) = drive(30.0);
        let m = find_mount([&acc[0], &acc[1], &acc[2]], &gps, &lon).expect("a mount");
        assert!((m.tilt_deg - 30.0).abs() < 0.5, "{}", m.tilt_deg);
        let out = kart_channels([&acc[0], &acc[1], &acc[2]], &m);
        // Forward matches the made-up forward acceleration; vertical stays level.
        let i = 1000;
        assert!((out[0].values[i] - lon[i / 2]).abs() < 0.01);
        assert!(out[1].values.iter().all(|v| v.abs() < 0.01));
    }

    #[test]
    fn nothing_without_a_still_moment() {
        let (acc, mut gps, lon) = drive(30.0);
        gps.speed_kmh.iter_mut().for_each(|s| *s = 40.0);
        assert_eq!(find_mount([&acc[0], &acc[1], &acc[2]], &gps, &lon), Err(NoMount::NeverStill));
    }
}
