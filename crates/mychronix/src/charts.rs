//! Histogram and scatter calculations, as RaceStudio's Histogram and
//! Scatter layouts.

use aim_xrk::Channel;

use crate::units::Shown;

/// A tidy bin width giving about `target` bins over `span`: 1, 2, 2.5 or 5 × 10ⁿ.
pub fn nice_bin(span: f64, target: f64) -> f64 {
    if span <= 0.0 || !span.is_finite() {
        return 1.0;
    }
    let raw = span / target;
    let mag = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|&w| w >= raw)
        .unwrap_or(10.0 * mag)
}

/// Seconds spent in each bin of width `bin` starting at `lo`, over the time
/// windows given (ms). Each sample counts for the time until the next one
/// (capped at 1 s, so gaps in logging don't count). Bins are in the units shown.
pub fn histogram(c: &Channel, windows: &[(i64, i64)], lo: f64, bin: f64, bins: usize) -> Vec<f64> {
    let shown = Shown::of(c);
    let mut out = vec![0.0; bins];
    for &(start, end) in windows {
        let a = c.times_ms.partition_point(|&t| t < start);
        let b = c.times_ms.partition_point(|&t| t <= end);
        for i in a..b {
            let next = if i + 1 < c.times_ms.len() {
                c.times_ms[i + 1].min(end)
            } else {
                end
            };
            let dt = ((next - c.times_ms[i]) as f64 / 1000.0).clamp(0.0, 1.0);
            let v = shown.value(c.values[i]);
            let k = ((v - lo) / bin).floor();
            if k >= 0.0 && (k as usize) < bins && v.is_finite() {
                out[k as usize] += dt;
            }
        }
    }
    out
}

/// The channel's value at `t_ms`, interpolated between samples; None across
/// a gap in the recording (over a quarter second), rather than bridging it.
pub fn value_at(c: &Channel, t_ms: f64) -> Option<f64> {
    let i = c.times_ms.partition_point(|&t| (t as f64) < t_ms);
    if i == 0 || i >= c.times_ms.len() {
        return None;
    }
    let (t0, t1) = (c.times_ms[i - 1] as f64, c.times_ms[i] as f64);
    if t1 - t0 > 250.0 {
        return None;
    }
    let f = if t1 > t0 {
        (t_ms - t0) / (t1 - t0)
    } else {
        0.0
    };
    Some(c.values[i - 1] + f * (c.values[i] - c.values[i - 1]))
}

/// Points (x, y, color value) at x's sample times within the windows, at
/// most `max` of them (evenly thinned), in the units shown.
pub fn scatter(
    x: &Channel,
    y: &Channel,
    color: Option<&Channel>,
    windows: &[(i64, i64)],
    max: usize,
) -> Vec<[f64; 3]> {
    let times: Vec<i64> = windows
        .iter()
        .flat_map(|&(s, e)| {
            let a = x.times_ms.partition_point(|&t| t < s);
            let b = x.times_ms.partition_point(|&t| t <= e);
            a..b
        })
        .map(|i| x.times_ms[i])
        .collect();
    let step = (times.len() / max.max(1)).max(1);
    times
        .iter()
        .step_by(step)
        .filter_map(|&t| {
            let xv = Shown::of(x).value(value_at(x, t as f64)?);
            let yv = Shown::of(y).value(value_at(y, t as f64)?);
            let cv = color
                .and_then(|c| Some(Shown::of(c).value(value_at(c, t as f64)?)))
                .unwrap_or(0.0);
            (xv.is_finite() && yv.is_finite()).then_some([xv, yv, cv])
        })
        .collect()
}

/// The scatter to start on: (across, up, color by). The grip circle when the
/// session has the IMU's forward g and the GPS's lateral g, colored by speed
/// when there is one; otherwise RPM against speed. `has` says whether the
/// session recorded a channel.
pub fn default_scatter(
    has: impl Fn(&str) -> bool,
) -> (&'static str, &'static str, Option<&'static str>) {
    if has("Forward g") && has("GPS LatAcc") {
        (
            "GPS LatAcc",
            "Forward g",
            has("GPS Speed").then_some("GPS Speed"),
        )
    } else {
        ("GPS Speed", "RPM", None)
    }
}

/// The grip the kart reached on a grip circle (g): how far out its points
/// go, at the 98th percentile so a lone bump doesn't set it. None with 50
/// points or fewer beyond 0.2 g.
pub fn grip_reached(points: &[[f64; 3]]) -> Option<f64> {
    let mut r: Vec<f64> = points
        .iter()
        .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
        .filter(|r| *r > 0.2)
        .collect();
    r.sort_by(f64::total_cmp);
    (r.len() > 50).then(|| r[r.len() * 98 / 100])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scatter_starts_on_the_grip_circle_when_it_can() {
        let has = |names: &'static [&'static str]| move |n: &str| names.contains(&n);
        assert_eq!(
            default_scatter(has(&["Forward g", "GPS LatAcc", "GPS LonAcc", "GPS Speed"])),
            ("GPS LatAcc", "Forward g", Some("GPS Speed"))
        );
        // The GPS's own forward g isn't enough to start on it; nor is the IMU without GPS.
        assert_eq!(
            default_scatter(has(&["GPS LatAcc", "GPS LonAcc"])),
            ("GPS Speed", "RPM", None)
        );
        assert_eq!(
            default_scatter(has(&["Forward g", "RPM"])),
            ("GPS Speed", "RPM", None)
        );
    }

    #[test]
    fn the_grip_reached_ignores_a_lone_bump() {
        // 100 points at 1.2 g round the circle, near-still points, and one 3 g bump.
        let mut pts: Vec<[f64; 3]> = (0..100)
            .map(|k| {
                let a = k as f64 / 100.0 * std::f64::consts::TAU;
                [1.2 * a.cos(), 1.2 * a.sin(), 0.0]
            })
            .collect();
        pts.extend([[0.05, 0.0, 0.0]; 30]);
        pts.push([3.0, 0.0, 0.0]);
        assert!((grip_reached(&pts).unwrap() - 1.2).abs() < 1e-9);
        assert_eq!(grip_reached(&pts[..40]), None);
    }

    fn channel(times: &[i64], values: &[f64]) -> Channel {
        Channel {
            name: "X".into(),
            group: None,
            units: String::new(),
            decimals: 1,
            interpolate: true,
            times_ms: times.to_vec(),
            values: values.to_vec(),
        }
    }

    #[test]
    fn tidy_bins() {
        assert_eq!(nice_bin(10_000.0, 40.0), 250.0);
        assert_eq!(nice_bin(60.0, 30.0), 2.0);
        assert_eq!(nice_bin(3.0, 30.0), 0.1);
    }

    #[test]
    fn histogram_weights_by_time() {
        // 100 for 3 s, then 200 for 1 s.
        let c = channel(
            &[0, 1000, 2000, 3000, 4000],
            &[100.0, 100.0, 100.0, 200.0, 200.0],
        );
        let h = histogram(&c, &[(0, 4000)], 0.0, 100.0, 3);
        assert_eq!(h, [0.0, 3.0, 1.0]);
    }

    #[test]
    fn scatter_resamples_y_at_x_times() {
        let x = channel(&[0, 100, 200, 300], &[1.0, 2.0, 3.0, 4.0]);
        let y = channel(&[0, 200, 400], &[10.0, 30.0, 50.0]);
        let pts = scatter(&x, &y, None, &[(100, 300)], 100);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0][..2], [2.0, 20.0]);
        assert_eq!(pts[2][..2], [4.0, 40.0]);
        // Only the point sitting on a real sample (t = 100); none across the gap.
        let gappy = channel(&[0, 100, 1000, 1100], &[1.0, 1.0, 9.0, 9.0]);
        assert_eq!(scatter(&x, &gappy, None, &[(100, 300)], 100).len(), 1);
    }
}
