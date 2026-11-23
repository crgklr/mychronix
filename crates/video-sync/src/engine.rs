//! Matching a video's sound to a session's RPM.
//!
//! The engine fires once per revolution (a two-stroke) or every other one (a
//! four-stroke), so the sound has a note at RPM/60 Hz (or half that) and its
//! harmonics. Rather than guess that note from the sound alone, which wind
//! and other karts make unreliable, the session's RPM says where the note
//! should be at each moment, and the match is the offset where the sound is
//! loudest at those predicted notes.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::audio::{RATE as SOUND_RATE, Sound};
use crate::signal::Signal;

const WINDOW: usize = 1600; // 0.2 s
const NFFT: usize = 4096;
const BIN_HZ: f64 = SOUND_RATE / NFFT as f64;
/// Candidate notes: 20–320 Hz in 1 Hz steps (1,200–19,200 firings a minute).
const LOW_HZ: f64 = 20.0;
const NOTES: usize = 300;
const HARMONICS: usize = 4;
/// Below this the engine is idling or off and the clutch is out: no signal.
const MIN_RPM: f64 = 1500.0;
/// Firings per revolution to try: two-stroke, four-stroke.
const ORDERS: [f64; 2] = [1.0, 0.5];

/// For each moment of the sound, how strongly each candidate note (with its
/// harmonics) stands out, in standard deviations from that moment's average.
#[derive(Debug, Clone)]
pub struct NoteStrength {
    pub start_s: f64,
    pub rate: f64,
    /// `frames × NOTES`, row by row.
    rows: Vec<f32>,
}

impl NoteStrength {
    pub fn frames(&self) -> usize {
        self.rows.len() / NOTES
    }

    /// The strength of `rpm × order` at frame `f`.
    fn at(&self, f: usize, rpm: f64, order: f64) -> f32 {
        let note = (rpm / 60.0 * order - LOW_HZ).round();
        let note = (note.max(0.0) as usize).min(NOTES - 1);
        self.rows[f * NOTES + note]
    }

    /// The same frames averaged in groups of `k` (for a fast first pass).
    fn coarser(&self, k: usize) -> NoteStrength {
        let frames = self.frames() / k;
        let mut rows = vec![0f32; frames * NOTES];
        for f in 0..frames * k {
            let (dst, src) = ((f / k) * NOTES, f * NOTES);
            for n in 0..NOTES {
                rows[dst + n] += self.rows[src + n] / k as f32;
            }
        }
        NoteStrength {
            start_s: self.start_s + (k as f64 - 1.0) / 2.0 / self.rate,
            rate: self.rate / k as f64,
            rows,
        }
    }
}

/// Reads the sound into note strengths, `rate` frames a second.
pub fn note_strength(sound: &Sound, rate: f64) -> Option<NoteStrength> {
    let hop = (SOUND_RATE / rate) as usize;
    if sound.samples.len() < WINDOW + hop * 10 {
        return None;
    }
    let frames = (sound.samples.len() - WINDOW) / hop + 1;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(NFFT);
    let hann: Vec<f32> = (0..WINDOW)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos())
        .collect();
    let top_bin = ((LOW_HZ + NOTES as f64) * HARMONICS as f64 / BIN_HZ) as usize + 2;
    let mut buf = vec![Complex::new(0f32, 0.0); NFFT];
    let mut spectrum = |f: usize, out: &mut [f32]| {
        let at = f * hop;
        for (i, c) in buf.iter_mut().enumerate() {
            let x = if i < WINDOW {
                sound.samples[at + i] * hann[i]
            } else {
                0.0
            };
            *c = Complex::new(x, 0.0);
        }
        fft.process(&mut buf);
        for (o, c) in out.iter_mut().zip(&buf) {
            *o = c.norm().ln_1p();
        }
    };

    // Steady sounds (wind roar, a rattle) are loud at the same pitch all
    // session: take each frequency's typical level (median of a sample of
    // frames) away, so only what changes, the engine, is left.
    let mut line = vec![0f32; top_bin];
    let sampled: Vec<usize> = (0..frames).step_by((frames / 400).max(1)).collect();
    let mut levels = vec![vec![0f32; sampled.len()]; top_bin];
    for (j, &f) in sampled.iter().enumerate() {
        spectrum(f, &mut line);
        for (b, level) in levels.iter_mut().enumerate() {
            level[j] = line[b];
        }
    }
    let floor: Vec<f32> = levels
        .iter_mut()
        .map(|l| {
            let mid = l.len() / 2;
            *l.select_nth_unstable_by(mid, f32::total_cmp).1
        })
        .collect();

    let bins: Vec<[usize; HARMONICS]> = (0..NOTES)
        .map(|n| {
            std::array::from_fn(|h| {
                ((LOW_HZ + n as f64) * (h + 1) as f64 / BIN_HZ).round() as usize
            })
        })
        .collect();
    let mut rows = vec![0f32; frames * NOTES];
    for f in 0..frames {
        spectrum(f, &mut line);
        for (b, v) in line.iter_mut().enumerate() {
            *v = (*v - floor[b]).max(0.0);
        }
        let row = &mut rows[f * NOTES..(f + 1) * NOTES];
        for (n, r) in row.iter_mut().enumerate() {
            *r = bins[n].iter().map(|&b| line[b]).sum();
        }
        let mean = row.iter().sum::<f32>() / NOTES as f32;
        let sd = (row.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / NOTES as f32).sqrt();
        for r in row.iter_mut() {
            *r = if sd > 0.0 { (*r - mean) / sd } else { 0.0 };
        }
    }
    Some(NoteStrength {
        start_s: sound.start_s + WINDOW as f64 / 2.0 / SOUND_RATE,
        rate,
        rows,
    })
}

/// The average strength of the notes the RPM predicts, with the video placed
/// at `offset_s` (session time = video time + offset), and how many frames
/// counted. `None` if fewer than `min_frames` had the engine running.
fn strength_at(
    notes: &NoteStrength,
    rpm: &Signal,
    offset_s: f64,
    order: f64,
    min_frames: usize,
) -> Option<(f64, usize)> {
    let (mut sum, mut used) = (0.0, 0usize);
    for f in 0..notes.frames() {
        let t = notes.start_s + f as f64 / notes.rate + offset_s;
        let x = (t - rpm.start_s) * rpm.rate;
        if x < 0.0 || x >= (rpm.values.len() - 1) as f64 {
            continue;
        }
        let (k, frac) = (x as usize, x.fract());
        let r = rpm.values[k] * (1.0 - frac) + rpm.values[k + 1] * frac;
        if r < MIN_RPM {
            continue;
        }
        sum += notes.at(f, r, order) as f64;
        used += 1;
    }
    (used >= min_frames.max(1)).then(|| (sum / used as f64, used))
}

/// Strength at every offset `step_s` apart over the whole session, for the
/// order (two- or four-stroke) that fits best: (first offset, step, scores, frames used).
pub fn scan(
    notes: &NoteStrength,
    rpm: &Signal,
    coarse_hz: f64,
    min_overlap_s: f64,
) -> Option<Scan> {
    let k = ((notes.rate / coarse_hz).round() as usize).max(1);
    let notes = notes.coarser(k);
    let rpm = rpm.resampled(notes.rate);
    let step = 1.0 / notes.rate;
    let video_len = notes.frames() as f64 * step;
    let first = rpm.start_s - notes.start_s - video_len;
    let count = ((video_len + rpm.values.len() as f64 * step) / step) as usize;
    let min_frames = (min_overlap_s * notes.rate) as usize;
    let mut best: Option<Scan> = None;
    for order in ORDERS {
        let mut scores = vec![f64::NAN; count];
        let mut used = vec![0usize; count];
        for i in 0..count {
            if let Some((s, u)) =
                strength_at(&notes, &rpm, first + i as f64 * step, order, min_frames)
            {
                scores[i] = s;
                used[i] = u;
            }
        }
        let peak = scores
            .iter()
            .copied()
            .filter(|s| s.is_finite())
            .fold(f64::MIN, f64::max);
        if best.as_ref().is_none_or(|b| peak > b.peak) {
            best = Some(Scan {
                first,
                step,
                order,
                peak,
                scores,
                used_s: used.iter().map(|&u| u as f64 * step).collect(),
            });
        }
    }
    best.filter(|b| b.peak.is_finite())
}

/// One pass over every offset.
pub struct Scan {
    pub first: f64,
    pub step: f64,
    /// Firings per revolution that fit (1 two-stroke, 0.5 four-stroke).
    pub order: f64,
    pub peak: f64,
    /// Mean predicted-note strength at each offset (NaN: too little overlap).
    pub scores: Vec<f64>,
    /// Seconds of running engine compared at each offset.
    pub used_s: Vec<f64>,
}

/// The best offset within `around ± reach` s, in 10 ms steps, with a parabola
/// through the peak: (offset, strength).
pub fn refine(
    notes: &NoteStrength,
    rpm: &Signal,
    order: f64,
    around: f64,
    reach: f64,
) -> Option<(f64, f64)> {
    let step = 0.01;
    let n = (reach / step) as isize;
    let min_frames = (10.0 * notes.rate) as usize;
    let pts: Vec<(f64, f64)> = (-n..=n)
        .filter_map(|i| {
            let o = around + i as f64 * step;
            strength_at(notes, rpm, o, order, min_frames).map(|(s, _)| (o, s))
        })
        .collect();
    crate::peak(&pts, step)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up session: RPM wandering between 6,000 and 11,000 over 3 minutes.
    fn rpm_at(t: f64) -> f64 {
        8500.0 + 2500.0 * (t * 0.21).sin() * (t * 0.047).cos() + 600.0 * (t * 0.9).sin()
    }

    #[test]
    fn finds_where_a_clip_sits_by_its_engine_note() {
        // The session: 180 s of RPM at 20 Hz, its clock starting at 0.4 s.
        let rpm = Signal::from_points(
            (0..3600).map(|i| (0.4 + i as f64 / 20.0, rpm_at(0.4 + i as f64 / 20.0))),
            50.0,
        )
        .unwrap();
        // The clip: 90 s of a two-stroke note (with harmonics) that started 47.3 s into the
        // session, under a loud steady 60 Hz hum and noise.
        let start = 47.3;
        let mut phase = 0.0f64;
        let mut seed = 1u32;
        let samples: Vec<f32> = (0..(90.0 * SOUND_RATE) as usize)
            .map(|i| {
                let t = start + i as f64 / SOUND_RATE;
                phase += rpm_at(t) / 60.0 / SOUND_RATE * std::f64::consts::TAU;
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f64 / (1u32 << 24) as f64 - 0.5;
                let engine = phase.sin() + 0.6 * (2.0 * phase).sin() + 0.4 * (3.0 * phase).sin();
                let hum = 3.0 * (i as f64 / SOUND_RATE * 60.0 * std::f64::consts::TAU).sin();
                (0.3 * engine + hum + noise) as f32
            })
            .collect();
        let notes = note_strength(
            &Sound {
                start_s: 0.0,
                samples,
            },
            25.0,
        )
        .unwrap();
        let scan = scan(&notes, &rpm, 5.0, 60.0).unwrap();
        assert_eq!(scan.order, 1.0);
        let best = (0..scan.scores.len())
            .filter(|&i| scan.scores[i].is_finite())
            .max_by(|&a, &b| scan.scores[a].total_cmp(&scan.scores[b]))
            .unwrap();
        let coarse = scan.first + best as f64 * scan.step;
        assert!((coarse - start).abs() < 0.5, "coarse {coarse}");
        let (fine, strength) = refine(&notes, &rpm, scan.order, coarse, 1.0).unwrap();
        assert!((fine - start).abs() < 0.03, "fine {fine}");
        assert!(strength > 1.5, "strength {strength}");
    }
}
