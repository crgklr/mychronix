//! Splits worked out from GPS: split lines placed at equal distances around a
//! track, and the time each lap takes between them.
//!
//! The logger records split crossings only when the track it's using has
//! splits defined; ours don't, so Mychronix places its own.

use aim_track::LatLon;

/// A track divided into sectors.
pub struct Sectors {
    /// The reference path, rotated to start at the start/finish.
    pub path: Vec<LatLon>,
    /// Index into `path` where each sector after the first begins.
    pub boundaries: Vec<usize>,
}

impl Sectors {
    /// Divides `path` into `count` sectors of equal length, starting at the
    /// point nearest `start_finish` (or the path's first point).
    pub fn even(path: &[LatLon], start_finish: Option<LatLon>, count: usize) -> Option<Self> {
        if path.len() < 4 || count < 2 {
            return None;
        }
        let start = start_finish
            .and_then(|sf| (0..path.len()).min_by(|&a, &b| path[a].distance_m(&sf).total_cmp(&path[b].distance_m(&sf))))
            .unwrap_or(0);
        let mut rotated: Vec<LatLon> = path[start..].iter().chain(&path[..start]).copied().collect();
        // Close the loop so the last sector ends back at the line.
        if rotated[0].distance_m(&rotated[rotated.len() - 1]) > 1.0 {
            rotated.push(rotated[0]);
        }
        let mut cumulative = vec![0.0];
        for w in rotated.windows(2) {
            cumulative.push(cumulative.last().unwrap() + w[0].distance_m(&w[1]));
        }
        let total = *cumulative.last().unwrap();
        let boundaries = (1..count)
            .map(|k| cumulative.partition_point(|&d| d < total * k as f64 / count as f64).min(rotated.len() - 1))
            .collect();
        Some(Self { path: rotated, boundaries })
    }

    pub fn count(&self) -> usize {
        self.boundaries.len() + 1
    }

    /// Where each split line sits (excluding the start/finish).
    pub fn split_points(&self) -> Vec<LatLon> {
        self.boundaries.iter().map(|&i| self.path[i]).collect()
    }
}

/// Sector times in ms for one lap, from GPS samples (`times_ms`, `points`)
/// covering [lap_start_ms, lap_end_ms]. Each split is timed where the kart
/// passes closest to it, interpolated between samples; splits must be passed
/// in order.
pub fn sector_times(times_ms: &[i64], points: &[LatLon], lap_start_ms: i64, lap_end_ms: i64, splits: &[LatLon]) -> Option<Vec<i64>> {
    let lo = times_ms.partition_point(|&t| t < lap_start_ms);
    let hi = times_ms.partition_point(|&t| t <= lap_end_ms);
    if hi < lo + 4 {
        return None;
    }
    let mut crossings = vec![lap_start_ms as f64];
    let mut from = lo;
    for split in splits {
        // Closest sample to the split, searching forward from the last one.
        let best = (from..hi).min_by(|&a, &b| points[a].distance_m(split).total_cmp(&points[b].distance_m(split)))?;
        if points[best].distance_m(split) > 30.0 {
            return None; // this lap never came near the split (e.g. a pit lap)
        }
        crossings.push(refine(times_ms, points, best, split));
        from = best;
    }
    crossings.push(lap_end_ms as f64);
    let times: Vec<i64> = crossings.windows(2).map(|w| (w[1] - w[0]).round() as i64).collect();
    times.iter().all(|&t| t > 0).then_some(times)
}

/// The time the kart was closest to `target`, between sample `i` and its
/// better neighbour, by projecting the target onto that segment.
fn refine(times_ms: &[i64], points: &[LatLon], i: usize, target: &LatLon) -> f64 {
    let neighbour = [i.checked_sub(1), (i + 1 < points.len()).then_some(i + 1)]
        .into_iter()
        .flatten()
        .min_by(|&a, &b| points[a].distance_m(target).total_cmp(&points[b].distance_m(target)));
    let Some(j) = neighbour else { return times_ms[i] as f64 };
    let (a, b) = if j < i { (j, i) } else { (i, j) };
    // Flat local coordinates in metres around `a`.
    let kx = 111_320.0 * points[a].lat.to_radians().cos();
    let to_xy = |p: &LatLon| ((p.lon - points[a].lon) * kx, (p.lat - points[a].lat) * 110_540.0);
    let (bx, by) = to_xy(&points[b]);
    let (tx, ty) = to_xy(target);
    let len2 = bx * bx + by * by;
    let f = if len2 > 0.0 { ((tx * bx + ty * by) / len2).clamp(0.0, 1.0) } else { 0.0 };
    times_ms[a] as f64 + f * (times_ms[b] - times_ms[a]) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A circle of radius ~100 m, `n` points, starting due east and going anticlockwise.
    fn circle(n: usize) -> Vec<LatLon> {
        (0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / n as f64;
                LatLon { lat: 45.0 + 100.0 * a.sin() / 110_540.0, lon: 9.0 + 100.0 * a.cos() / (111_320.0 * 45f64.to_radians().cos()) }
            })
            .collect()
    }

    #[test]
    fn even_sectors_split_the_circle_in_thirds() {
        let path = circle(120);
        let s = Sectors::even(&path, Some(path[30]), 3).unwrap();
        assert_eq!(s.count(), 3);
        assert_eq!(s.path[0], path[30]);
        // Thirds of 121 closed-loop points land about 40 points apart.
        assert!((s.boundaries[0] as i64 - 40).abs() <= 1 && (s.boundaries[1] as i64 - 80).abs() <= 1);
    }

    #[test]
    fn constant_speed_gives_equal_sectors() {
        // One lap of the circle in 30 s, sampled at 25 Hz.
        let n = 750;
        let samples = circle(n);
        let times: Vec<i64> = (0..n as i64).map(|k| k * 40).collect();
        let s = Sectors::even(&circle(360), Some(samples[0]), 3).unwrap();
        let t = sector_times(&times, &samples, 0, 30_000, &s.split_points()).unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t.iter().sum::<i64>(), 30_000);
        // Splits snap to path points (1° apart here), so time each split
        // against where it actually sits: 30 s × its share of the circle.
        let mut expected = Vec::new();
        let mut prev = 0.0;
        for &b in &s.boundaries {
            let at = 30_000.0 * b as f64 / 360.0;
            expected.push(at - prev);
            prev = at;
        }
        expected.push(30_000.0 - prev);
        for (got, want) in t.iter().zip(&expected) {
            assert!((*got as f64 - want).abs() <= 25.0, "{got} vs {want}");
        }
    }

    #[test]
    fn lap_that_misses_a_split_has_no_times() {
        let samples = circle(200);
        let times: Vec<i64> = (0..200).map(|k| k * 40).collect();
        let far = LatLon { lat: 46.0, lon: 9.0 };
        assert!(sector_times(&times, &samples, 0, 8_000, &[far]).is_none());
    }
}
