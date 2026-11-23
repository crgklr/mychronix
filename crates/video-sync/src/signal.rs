//! Evenly sampled signals and the math that lines two of them up.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// A signal sampled `rate` times a second, its first value at `start_s`.
#[derive(Debug, Clone)]
pub struct Signal {
    pub start_s: f64,
    pub rate: f64,
    pub values: Vec<f64>,
}

impl Signal {
    pub fn end_s(&self) -> f64 {
        self.start_s + self.values.len() as f64 / self.rate
    }

    /// Resamples irregular (time s, value) points onto an even grid: each slot is
    /// the mean of its points, empty slots are filled in from their neighbors.
    pub fn from_points(points: impl IntoIterator<Item = (f64, f64)>, rate: f64) -> Option<Self> {
        let points: Vec<(f64, f64)> = points.into_iter().filter(|p| p.1.is_finite()).collect();
        let start_s = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let end_s = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        if points.len() < 2 || end_s <= start_s {
            return None;
        }
        let n = ((end_s - start_s) * rate) as usize + 1;
        let (mut sum, mut count) = (vec![0.0; n], vec![0u32; n]);
        for (t, v) in points {
            let i = (((t - start_s) * rate) as usize).min(n - 1);
            sum[i] += v;
            count[i] += 1;
        }
        let known: Vec<(usize, f64)> = (0..n)
            .filter(|&i| count[i] > 0)
            .map(|i| (i, sum[i] / count[i] as f64))
            .collect();
        let mut values = vec![0.0; n];
        for w in known.windows(2) {
            let ((i0, v0), (i1, v1)) = (w[0], w[1]);
            for (k, v) in values[i0..=i1].iter_mut().enumerate() {
                *v = v0 + (v1 - v0) * k as f64 / (i1 - i0).max(1) as f64;
            }
        }
        Some(Signal {
            start_s,
            rate,
            values,
        })
    }

    /// The same signal at another rate (linear interpolation).
    pub fn resampled(&self, rate: f64) -> Signal {
        let n = (self.values.len() as f64 * rate / self.rate) as usize;
        let values = (0..n)
            .map(|i| {
                let x = i as f64 * self.rate / rate;
                let k = x as usize;
                let f = x - k as f64;
                let a = self.values[k.min(self.values.len() - 1)];
                let b = self.values[(k + 1).min(self.values.len() - 1)];
                a + (b - a) * f
            })
            .collect();
        Signal {
            start_s: self.start_s,
            rate,
            values,
        }
    }

    /// Moving average over `seconds`: keeps the turns, drops vibration.
    pub fn smoothed(&self, seconds: f64) -> Signal {
        let h = ((seconds * self.rate) as usize / 2).max(1);
        let n = self.values.len();
        let mut prefix = vec![0.0; n + 1];
        for (i, v) in self.values.iter().enumerate() {
            prefix[i + 1] = prefix[i] + v;
        }
        let values = (0..n)
            .map(|i| {
                let (lo, hi) = (i.saturating_sub(h), (i + h + 1).min(n));
                (prefix[hi] - prefix[lo]) / (hi - lo) as f64
            })
            .collect();
        Signal {
            values,
            ..self.clone()
        }
    }
}

/// Turning rate from a three-axis gyro mounted any which way: smoothed over
/// half a second (engine vibration shakes a gyro far harder than turning
/// does), then projected on the axis that varies most. On a kart that's the
/// yaw axis, because turning swamps pitch and roll. Its sign is arbitrary.
pub fn main_axis(points: &[(f64, [f64; 3])], rate: f64) -> Option<Signal> {
    let axes: Vec<Signal> = (0..3)
        .map(|k| {
            Signal::from_points(points.iter().map(|(t, v)| (*t, v[k])), rate)
                .map(|s| s.smoothed(0.5))
        })
        .collect::<Option<_>>()?;
    let n = axes.iter().map(|a| a.values.len()).min()?;
    let mean: Vec<f64> = axes
        .iter()
        .map(|a| a.values[..n].iter().sum::<f64>() / n as f64)
        .collect();
    let mut cov = [[0.0; 3]; 3];
    let rows = axes[0].values[..n]
        .iter()
        .zip(&axes[1].values[..n])
        .zip(&axes[2].values[..n]);
    for ((x, y), z) in rows.clone() {
        let d = [x - mean[0], y - mean[1], z - mean[2]];
        for r in 0..3 {
            for c in 0..3 {
                cov[r][c] += d[r] * d[c];
            }
        }
    }
    // Power iteration for the largest eigenvector of a 3×3 covariance.
    let mut v = [1.0, 1.0, 1.0];
    for _ in 0..100 {
        let w: Vec<f64> = (0..3)
            .map(|r| (0..3).map(|c| cov[r][c] * v[c]).sum())
            .collect();
        let norm = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm == 0.0 {
            return None;
        }
        v = [w[0] / norm, w[1] / norm, w[2] / norm];
    }
    Some(Signal {
        start_s: axes[0].start_s,
        rate,
        values: rows
            .map(|((x, y), z)| (x - mean[0]) * v[0] + (y - mean[1]) * v[1] + (z - mean[2]) * v[2])
            .collect(),
    })
}

/// Pearson correlation of `a` (the video) against `b` (the session) at every
/// shift where they overlap by at least `min_overlap` samples: `(shift, r,
/// overlap)`, where `a[i]` lines up with `b[i + shift]`. One FFT
/// cross-correlation plus running sums, so a whole session costs milliseconds.
pub fn correlate(a: &[f64], b: &[f64], min_overlap: usize) -> Vec<(isize, f64, usize)> {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 || min_overlap == 0 {
        return vec![];
    }
    let size = (n + m).next_power_of_two();
    let fft: Arc<dyn Fft<f64>> = FftPlanner::new().plan_fft_forward(size);
    let ifft: Arc<dyn Fft<f64>> = FftPlanner::new().plan_fft_inverse(size);
    let mut fa = vec![Complex::new(0.0, 0.0); size];
    let mut fb = vec![Complex::new(0.0, 0.0); size];
    for (i, &x) in a.iter().enumerate() {
        fa[i].re = x;
    }
    for (i, &x) in b.iter().enumerate() {
        fb[i].re = x;
    }
    fft.process(&mut fa);
    fft.process(&mut fb);
    let mut cross: Vec<Complex<f64>> = fa.iter().zip(&fb).map(|(x, y)| x.conj() * y).collect();
    ifft.process(&mut cross);
    let prefix = |v: &[f64]| {
        let mut s = vec![(0.0, 0.0)];
        for &x in v {
            let (a, b) = *s.last().unwrap();
            s.push((a + x, b + x * x));
        }
        s
    };
    let (pa, pb) = (prefix(a), prefix(b));
    let mut out = vec![];
    for shift in -(n as isize) + 1..m as isize {
        let lo = (-shift).max(0) as usize;
        let hi = n.min((m as isize - shift) as usize);
        if hi <= lo || hi - lo < min_overlap {
            continue;
        }
        let k = (hi - lo) as f64;
        let (sa, saa) = (pa[hi].0 - pa[lo].0, pa[hi].1 - pa[lo].1);
        let (bl, bh) = (
            (lo as isize + shift) as usize,
            (hi as isize + shift) as usize,
        );
        let (sb, sbb) = (pb[bh].0 - pb[bl].0, pb[bh].1 - pb[bl].1);
        let sab = cross[shift.rem_euclid(size as isize) as usize].re / size as f64;
        let den = ((saa - sa * sa / k) * (sbb - sb * sb / k)).sqrt();
        let r = if den > 1e-12 {
            (sab - sa * sb / k) / den
        } else {
            0.0
        };
        out.push((shift, r, hi - lo));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlate_finds_a_known_shift() {
        // Pseudo-random (xorshift), so no other shift lines up.
        let mut x = 88_172_645_463_325_252u64;
        let b: Vec<f64> = (0..500)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x % 1000) as f64
            })
            .collect();
        let a: Vec<f64> = b[120..320].iter().map(|x| x * 2.0 + 5.0).collect();
        let best = correlate(&a, &b, 100)
            .into_iter()
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        assert_eq!(best.0, 120);
        assert!((best.1 - 1.0).abs() < 1e-9);
        assert_eq!(best.2, 200);
    }

    #[test]
    fn from_points_fills_gaps_between_readings() {
        let s = Signal::from_points([(0.0, 0.0), (0.4, 4.0)], 10.0).unwrap();
        assert_eq!(s.values.len(), 5);
        assert!((s.values[2] - 2.0).abs() < 1e-9);
    }
}
