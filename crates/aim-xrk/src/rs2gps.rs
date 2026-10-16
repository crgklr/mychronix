//! GPS module positions RaceStudio 2 keeps beside a `.drk`, in the logger's
//! clock: `.rrk` holds the receiver's raw solutions (`NSOL` records: the
//! logger's tick count, then a u-blox NAV-SOL payload with earth-centered
//! position and velocity in cm and cm/s, about 4 a second); `.gpk` holds
//! RaceStudio 2's processed track (a `PROV` record with an earth-centered
//! reference point, then `PSOL` records: the tick count and east, north, up
//! offsets from it in meters, about 8 a second). See docs/protocol-notes.md.

use crate::GpsTrack;

/// One position: the logger's tick count when it arrived, degrees, and the
/// receiver's own speed (km/h) and heading (degrees) when it has them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fix {
    pub ticks: u32,
    pub lat: f64,
    pub lon: f64,
    pub speed_kmh: Option<f64>,
    pub heading: Option<f64>,
}

const A: f64 = 6_378_137.0;
const E2: f64 = 6.694_379_990_14e-3;

/// Earth-centered meters to latitude and longitude (degrees), WGS-84.
fn to_lat_lon(x: f64, y: f64, z: f64) -> (f64, f64) {
    let b = A * (1.0 - E2).sqrt();
    let ep2 = (A * A - b * b) / (b * b);
    let p = x.hypot(y);
    let th = (z * A).atan2(p * b);
    let lat = (z + ep2 * b * th.sin().powi(3)).atan2(p - E2 * A * th.cos().powi(3));
    (lat.to_degrees(), y.atan2(x).to_degrees())
}

/// East/north/up unit vectors at a latitude and longitude (degrees).
fn enu_axes(lat: f64, lon: f64) -> [[f64; 3]; 3] {
    let (sp, cp) = lat.to_radians().sin_cos();
    let (sl, cl) = lon.to_radians().sin_cos();
    [[-sl, cl, 0.0], [-sp * cl, -sp * sl, cp], [cp * cl, cp * sl, sp]]
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn i32_at(b: &[u8], o: usize) -> Option<i32> {
    u32_at(b, o).map(|v| v as i32)
}

fn f64_at(b: &[u8], o: usize) -> Option<f64> {
    b.get(o..o + 8).map(|x| f64::from_le_bytes(x.try_into().unwrap()))
}

/// Each record of `tag` that holds data (not the section header, whose
/// next word is 0x100), by offset.
fn records<'a>(data: &'a [u8], tag: &'a [u8; 4]) -> impl Iterator<Item = usize> + 'a {
    let mut i = 0;
    std::iter::from_fn(move || {
        while i + 8 <= data.len() {
            let at = i;
            i += 1;
            if &data[at..at + 4] == tag && u32_at(data, at + 4) != Some(0x100) {
                i = at + 4;
                return Some(at);
            }
        }
        None
    })
}

/// The raw solutions in a `.rrk`, 3D fixes only.
pub fn read_rrk(data: &[u8]) -> Vec<Fix> {
    records(data, b"NSOL")
        .filter_map(|at| {
            let p = at + 8; // NAV-SOL payload
            let ticks = u32_at(data, at + 4)?;
            let fix = *data.get(p + 10)?;
            if fix < 3 {
                return None;
            }
            let c = |o| i32_at(data, p + o).map(|v| v as f64 / 100.0);
            let (x, y, z) = (c(12)?, c(16)?, c(20)?);
            let (vx, vy, vz) = (c(28)?, c(32)?, c(36)?);
            let (lat, lon) = to_lat_lon(x, y, z);
            let [e, n, _] = enu_axes(lat, lon);
            let ve = e[0] * vx + e[1] * vy + e[2] * vz;
            let vn = n[0] * vx + n[1] * vy + n[2] * vz;
            Some(Fix { ticks, lat, lon, speed_kmh: Some(ve.hypot(vn) * 3.6), heading: Some(ve.atan2(vn).to_degrees().rem_euclid(360.0)) })
        })
        .filter(|f| f.lat.is_finite() && f.lon.is_finite() && (f.lat != 0.0 || f.lon != 0.0))
        .collect()
}

/// RaceStudio 2's processed track in a `.gpk`.
pub fn read_gpk(data: &[u8]) -> Vec<Fix> {
    // The reference point: the PROV record whose three doubles at +14 are a
    // point on the earth's surface (earth-centered meters).
    let on_earth = |p: &(f64, f64, f64)| (6.3e6..6.4e6).contains(&(p.0.powi(2) + p.1.powi(2) + p.2.powi(2)).sqrt());
    let Some(origin) = (0..data.len().saturating_sub(4))
        .filter(|&i| &data[i..i + 4] == b"PROV")
        .filter_map(|at| Some((f64_at(data, at + 14)?, f64_at(data, at + 22)?, f64_at(data, at + 30)?)))
        .find(on_earth)
    else {
        return Vec::new();
    };
    let (lat0, lon0) = to_lat_lon(origin.0, origin.1, origin.2);
    let [e, n, u] = enu_axes(lat0, lon0);
    records(data, b"PSOL")
        .filter_map(|at| {
            let ticks = u32_at(data, at + 4)?;
            let (de, dn, du) = (f64_at(data, at + 16)?, f64_at(data, at + 24)?, f64_at(data, at + 32)?);
            if !(de.abs() < 1e5 && dn.abs() < 1e5 && du.abs() < 1e5) {
                return None;
            }
            let x = origin.0 + e[0] * de + n[0] * dn + u[0] * du;
            let y = origin.1 + e[1] * de + n[1] * dn + u[1] * du;
            let z = origin.2 + e[2] * de + n[2] * dn + u[2] * du;
            let (lat, lon) = to_lat_lon(x, y, z);
            Some(Fix { ticks, lat, lon, speed_kmh: None, heading: None })
        })
        .collect()
}

/// Gives `track`'s positions the receiver's speed and heading from `raw`
/// (a `.rrk`'s fixes), by tick: RaceStudio 2's processed positions are
/// smoother, its receiver velocities more exact than differenced positions.
pub fn with_velocity(mut track: Vec<Fix>, raw: &[Fix]) -> Vec<Fix> {
    let mut raw: Vec<&Fix> = raw.iter().filter(|f| f.speed_kmh.is_some()).collect();
    raw.sort_by_key(|f| f.ticks);
    if raw.len() < 2 {
        return track;
    }
    for f in &mut track {
        let i = raw.partition_point(|r| r.ticks <= f.ticks);
        let (a, b) = (raw[i.saturating_sub(1)], raw[i.min(raw.len() - 1)]);
        // Only between two receiver fixes less than a second apart.
        if b.ticks.saturating_sub(a.ticks) > 1000 || f.ticks < a.ticks || f.ticks > b.ticks {
            continue;
        }
        let w = if b.ticks > a.ticks { (f.ticks - a.ticks) as f64 / (b.ticks - a.ticks) as f64 } else { 0.0 };
        let (sa, sb) = (a.speed_kmh.unwrap(), b.speed_kmh.unwrap());
        f.speed_kmh = Some(sa + (sb - sa) * w);
        f.heading = if w < 0.5 { a.heading } else { b.heading };
    }
    track
}

/// Meters between two positions (equirectangular; fine at these distances).
fn meters(a: &Fix, b: &Fix) -> (f64, f64) {
    let r = 6_371_000.0;
    let de = (b.lon - a.lon).to_radians() * r * ((a.lat + b.lat) / 2.0).to_radians().cos();
    let dn = (b.lat - a.lat).to_radians() * r;
    (de, dn)
}

/// The positions as a track on the session's clock (`ms_per_tick` from its
/// laps), shifted by `offset_ms`. Speed and heading come from the receiver
/// when it reports them, else from the positions themselves.
pub fn track(fixes: &[Fix], ms_per_tick: f64, offset_ms: i64) -> GpsTrack {
    let mut fixes: Vec<Fix> = fixes.to_vec();
    fixes.sort_by_key(|f| f.ticks);
    fixes.dedup_by_key(|f| f.ticks);
    let n = fixes.len();
    let ms = |f: &Fix| (f.ticks as f64 * ms_per_tick).round() as i64 + offset_ms;
    let mut t = GpsTrack::default();
    let mut heading = Vec::with_capacity(n);
    for k in 0..n {
        // Neighbors two either side smooth the 8 Hz track.
        let (a, b) = (k.saturating_sub(2), (k + 2).min(n - 1));
        let dt = (ms(&fixes[b]) - ms(&fixes[a])) as f64 / 1000.0;
        let (de, dn) = meters(&fixes[a], &fixes[b]);
        let f = &fixes[k];
        let speed = f.speed_kmh.unwrap_or(if dt > 0.0 { de.hypot(dn) / dt * 3.6 } else { 0.0 });
        heading.push(f.heading.unwrap_or(de.atan2(dn).to_degrees().rem_euclid(360.0)));
        t.times_ms.push(ms(f));
        t.latitude.push(f.lat);
        t.longitude.push(f.lon);
        t.speed_kmh.push(speed);
    }
    // Yaw rate from the change in heading (wrapped), clockwise positive.
    for k in 0..n {
        let (a, b) = (k.saturating_sub(1), (k + 1).min(n - 1));
        let dt = (t.times_ms[b] - t.times_ms[a]) as f64 / 1000.0;
        let dh = ((heading[b] - heading[a] + 540.0).rem_euclid(360.0)) - 180.0;
        // Heading means little when nearly stopped.
        let moving = t.speed_kmh[k] > 5.0;
        t.yaw_rate_dps.push(if dt > 0.0 && moving { dh / dt } else { 0.0 });
    }
    t
}

/// The shift (ms, within ±3 s) that best lines the track's speed up with
/// `speed` (a GPS Speed channel on the session's clock), and how well (the
/// correlation, 0–1). None when there's too little overlap to tell.
pub fn best_offset(fixes: &[Fix], ms_per_tick: f64, speed: &crate::Channel) -> Option<(i64, f64)> {
    if speed.values.len() < 50 || fixes.len() < 50 {
        return None;
    }
    let base = track(fixes, ms_per_tick, 0);
    let at = |t: i64| -> Option<f64> {
        let i = speed.times_ms.partition_point(|&x| x < t);
        (i > 0 && i < speed.times_ms.len()).then(|| speed.values[i])
    };
    let corr = |off: i64| -> Option<f64> {
        let pairs: Vec<(f64, f64)> = base.times_ms.iter().zip(&base.speed_kmh).filter_map(|(&t, &v)| Some((v, at(t + off)?))).collect();
        if pairs.len() < 50 {
            return None;
        }
        let n = pairs.len() as f64;
        let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
        let (sxy, sxx, syy) = pairs.iter().fold((0.0, 0.0, 0.0), |a, p| (a.0 + (p.0 - mx) * (p.1 - my), a.1 + (p.0 - mx).powi(2), a.2 + (p.1 - my).powi(2)));
        (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt())
    };
    (-3000..=3000).step_by(25).filter_map(|off| Some((off as i64, corr(off as i64)?))).max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earth_centered_to_degrees() {
        // A point at the Ottobiano circuit, Italy.
        let (lat, lon) = to_lat_lon(4_451_510.40, 690_429.26, 4_500_352.11);
        assert!((lat - 45.164_533).abs() < 1e-5 && (lon - 8.816_328).abs() < 1e-5, "{lat} {lon}");
    }

    #[test]
    fn track_speed_and_heading_from_positions() {
        // Due east at 20 m/s, a fix every 125 ticks.
        let fixes: Vec<Fix> = (0..20)
            .map(|k| Fix { ticks: k * 125, lat: 45.0, lon: 8.0 + k as f64 * 2.5 / (6_371_000.0 * 45f64.to_radians().cos()) * 180.0 / std::f64::consts::PI, speed_kmh: None, heading: None })
            .collect();
        let t = track(&fixes, 1.0, 0);
        assert!((t.speed_kmh[10] - 72.0).abs() < 0.5, "{}", t.speed_kmh[10]);
        assert!(t.yaw_rate_dps[10].abs() < 0.01);
        assert_eq!(t.times_ms[4], 500);
    }
}
