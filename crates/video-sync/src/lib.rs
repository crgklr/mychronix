//! Lines up an onboard video with a MyChron session, so a video editor (or a
//! plugin for one) can show the session's data in step with the picture.
//!
//! Two signals are compared, whichever both sides have:
//! - **Engine sound vs. RPM.** The pitch of the engine in the video's sound
//!   follows its RPM. Works with any camera that records sound, any MyChron, and
//!   clips that were trimmed or re-encoded (which lose the camera's own data).
//! - **Gyro vs. gyro.** A GoPro records its own gyro; through the corners it
//!   turns with the kart, like the MyChron's gyro (or its GPS heading). Best at
//!   telling one lap from the next; the sound sets the exact offset (see [`find`]).
//!
//! The answer is one number, `offset_s`: session time = video time + offset_s.
//! It's saved beside the session as `<session>.video.json` (see [`store`]).

mod audio;
mod engine;
mod gpmf;
mod mp4;
pub mod signal;
pub mod store;

use std::fs::File;
use std::path::{Path, PathBuf};

use aim_xrk::Session;
use serde::{Deserialize, Serialize};

use signal::Signal;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not an MP4 or MOV video")]
    NotMp4,
    #[error("couldn't read the video's sound: {0}")]
    Audio(String),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

/// Rate (per second) for finding roughly where a video sits in a session.
const COARSE_HZ: f64 = 5.0;
/// Rate of the signals used for the final, frame-level answer.
const FINE_HZ: f64 = 50.0;
/// Frames a second of the sound's note strengths.
const NOTES_HZ: f64 = 25.0;

/// The signals in one video that a session can be matched against.
#[derive(Debug, Clone)]
pub struct VideoSignals {
    pub path: PathBuf,
    pub bytes: u64,
    pub duration_s: f64,
    /// The camera's clock at the start (seconds since 1970), if it wrote one. A
    /// hint only: cameras whose clock was never set write a date years off.
    pub created_unix: Option<i64>,
    /// The sound, as how strongly each engine note stands out moment by moment.
    pub engine: Option<engine::NoteStrength>,
    /// The camera's turning rate (GoPro gyro), sign arbitrary.
    pub turning: Option<Signal>,
}

/// The signals in one session that a video can be matched against.
#[derive(Debug, Clone, Default)]
pub struct SessionSignals {
    pub rpm: Option<Signal>,
    /// The kart's turning rate: the MyChron's gyro if it has one, else its GPS heading.
    pub turning: Option<Signal>,
}

/// Which signal a match came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Engine sound in the video vs. the session's RPM.
    EngineSound,
    /// The camera's gyro vs. the session's gyro or GPS heading.
    Gyro,
}

/// How well one signal lined up.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SignalFit {
    pub method: Method,
    /// Session time = video time + offset_s, by this signal alone.
    pub offset_s: f64,
    /// How well it fits there. Engine sound: how far the notes the RPM
    /// predicts stand out in the sound, in standard deviations (a clear match
    /// is 1.5 and up; a wrong one is under 1). Gyro: correlation, 0–1.
    pub fit: f64,
}

/// Where a video sits in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    /// Session time = video time + offset_s. Negative: the camera started first.
    pub offset_s: f64,
    /// Seconds of video that overlap the session.
    pub overlap_s: f64,
    /// How far this match stands out from the best one somewhere else in the
    /// session (0–1). Laps look alike, so a low margin means "maybe the wrong lap".
    pub margin: f64,
    /// Each signal's own answer.
    pub signals: Vec<SignalFit>,
}

impl Match {
    /// Good enough to trust without anyone checking it.
    pub fn is_confident(&self) -> bool {
        self.margin >= 0.25
            && self.signals.iter().any(|f| match f.method {
                Method::EngineSound => f.fit >= 1.5,
                Method::Gyro => f.fit >= 0.6,
            })
    }

    /// For ranking one video's matches across sessions: the signals' fits
    /// averaged (sound scaled to about 0–1), so one that disagrees pulls a
    /// match down, weighted toward matches that cover more of the video.
    pub fn score(&self, video_s: f64) -> f64 {
        let fits: Vec<f64> = self
            .signals
            .iter()
            .map(|f| match f.method {
                Method::EngineSound => (f.fit / 3.0).clamp(0.0, 1.0),
                Method::Gyro => f.fit,
            })
            .collect();
        let fit = fits.iter().sum::<f64>() / fits.len().max(1) as f64;
        fit * (self.overlap_s / video_s.max(1.0)).min(1.0).sqrt()
    }
}

/// Reads a video's sound and (GoPro) gyro. Takes a few seconds for a long clip.
pub fn video_signals(path: &Path) -> Result<VideoSignals, Error> {
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    let movie = mp4::read(&mut file)?;
    let turning = match movie.tracks.iter().find(|t| &t.format == b"gpmd") {
        Some(track) => signal::main_axis(&gpmf::gyro(&mut file, track)?, FINE_HZ),
        None => None,
    };
    let engine = audio::decode(path)?.and_then(|sound| engine::note_strength(&sound, NOTES_HZ));
    Ok(VideoSignals {
        path: path.to_path_buf(),
        bytes,
        duration_s: movie.duration_s,
        created_unix: movie.created_unix,
        engine,
        turning,
    })
}

/// The session's RPM and turning rate.
pub fn session_signals(session: &Session) -> SessionSignals {
    let points = |name: &str| {
        session.channel(name).map(|c| {
            c.times_ms
                .iter()
                .zip(&c.values)
                .map(|(&t, &v)| (t as f64 / 1000.0, v))
                .collect::<Vec<_>>()
        })
    };
    // A MyChron 4's RPM channel is called "Engine" until the app renames it.
    let rpm = points("RPM")
        .or_else(|| points("Engine"))
        .and_then(|p| Signal::from_points(p, FINE_HZ));
    let gyro = ["GyroX", "GyroY", "GyroZ"].map(points);
    let turning = match gyro {
        [Some(x), Some(y), Some(z)] if x.len() == y.len() && y.len() == z.len() => {
            let joined: Vec<(f64, [f64; 3])> = (0..x.len())
                .map(|i| (x[i].0, [x[i].1, y[i].1, z[i].1]))
                .collect();
            signal::main_axis(&joined, FINE_HZ)
        }
        _ => None,
    }
    .or_else(|| {
        let g = session.gps.as_ref()?;
        Signal::from_points(
            g.times_ms
                .iter()
                .zip(&g.yaw_rate_dps)
                .map(|(&t, &v)| (t as f64 / 1000.0, v)),
            FINE_HZ,
        )
        .map(|s| s.smoothed(0.5))
    });
    SessionSignals { rpm, turning }
}

/// Finds where `video` sits in `session`, or `None` if they share no signal or
/// overlap too little to tell (a minute, or most of a shorter clip).
pub fn find(video: &VideoSignals, session: &SessionSignals) -> Option<Match> {
    let min_overlap_s = 60f64.min(video.duration_s * 0.8);
    // 1. Coarse: each signal scores every offset. Each curve is scaled to its
    //    own peak and they're added, so both signals have a say.
    let mut curves: Vec<(f64, f64, Vec<f64>, Vec<f64>)> = vec![]; // (first, step, score, overlap s)
    let mut order = 1.0;
    if let (Some(notes), Some(rpm)) = (&video.engine, &session.rpm)
        && let Some(scan) = engine::scan(notes, rpm, COARSE_HZ, min_overlap_s)
    {
        order = scan.order;
        let video_s = notes.frames() as f64 / notes.rate;
        // Weighted toward offsets that compare more of the video, so a
        // sliver at the session's edge can't win.
        let score = scan
            .scores
            .iter()
            .zip(&scan.used_s)
            .map(|(s, u)| {
                if s.is_finite() {
                    s * (u / video_s).sqrt()
                } else {
                    f64::NAN
                }
            })
            .collect();
        curves.push((scan.first, scan.step, score, scan.used_s));
    }
    if let (Some(v), Some(s)) = (&video.turning, &session.turning) {
        let (v, s) = (v.resampled(COARSE_HZ), s.resampled(COARSE_HZ));
        let n = v.values.len();
        let min = (min_overlap_s * COARSE_HZ) as usize;
        let first = s.start_s - v.start_s - (n as f64 - 1.0) / COARSE_HZ;
        let mut score = vec![f64::NAN; n + s.values.len() - 1];
        let mut overlap = vec![0.0; score.len()];
        for (shift, r, ov) in signal::correlate(&v.values, &s.values, min) {
            let i = (shift + n as isize - 1) as usize;
            // The gyro's sign depends on how each was mounted: either will do.
            score[i] = r.abs() * (ov as f64 / n as f64).sqrt();
            overlap[i] = ov as f64 / COARSE_HZ;
        }
        curves.push((first, 1.0 / COARSE_HZ, score, overlap));
    }
    let g0 = curves.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
    let mut total: Vec<f64> = vec![];
    let mut overlap: Vec<f64> = vec![];
    for (first, step, score, ov) in &curves {
        let peak = score
            .iter()
            .copied()
            .filter(|s| s.is_finite())
            .fold(0.0, f64::max);
        if peak <= 0.0 {
            continue;
        }
        for (k, (s, o)) in score.iter().zip(ov).enumerate() {
            if !s.is_finite() {
                continue;
            }
            let i = ((first + k as f64 * step - g0) * COARSE_HZ).round() as usize;
            if i >= total.len() {
                total.resize(i + 1, 0.0);
                overlap.resize(i + 1, 0.0);
            }
            total[i] += s.max(0.0) / peak;
            overlap[i] = overlap[i].max(*o);
        }
    }
    let best = (0..total.len()).max_by(|&a, &b| total[a].total_cmp(&total[b]))?;
    if total[best] <= 0.0 {
        return None;
    }
    let coarse = g0 + best as f64 / COARSE_HZ;
    // The runner-up: the best score more than 5 s away (another lap, usually).
    let away = (5.0 * COARSE_HZ) as usize;
    let second = (0..total.len())
        .filter(|&i| i.abs_diff(best) > away)
        .map(|i| total[i])
        .fold(0.0, f64::max);

    // 2. Fine: each signal on its own, within a second of the coarse answer.
    let mut signals = vec![];
    if let (Some(notes), Some(rpm)) = (&video.engine, &session.rpm)
        && let Some((offset_s, fit)) = engine::refine(notes, rpm, order, coarse, 1.0)
    {
        signals.push(SignalFit {
            method: Method::EngineSound,
            offset_s,
            fit,
        });
    }
    if let (Some(v), Some(s)) = (&video.turning, &session.turning)
        && let Some((offset_s, fit)) = refine_gyro(v, s, coarse, 1.0)
    {
        signals.push(SignalFit {
            method: Method::Gyro,
            offset_s,
            fit,
        });
    }
    // The engine sound sets the offset when it fits. The gyro finds the lap
    // well, but most MyChrons sit on the steering wheel, so their gyro turns
    // with the steering, a few tenths before the kart does (measured on a
    // MyChron 6: the gyro answer lands ~0.37 s early). The sound and the RPM
    // are both the engine itself, with no such lead.
    let offset_s = match signals
        .iter()
        .find(|f| f.method == Method::EngineSound && f.fit >= 1.0)
    {
        Some(f) => f.offset_s,
        None => signals.iter().find(|f| f.method == Method::Gyro)?.offset_s,
    };
    Some(Match {
        offset_s,
        overlap_s: overlap[best],
        margin: ((total[best] - second) / total[best]).clamp(0.0, 1.0),
        signals,
    })
}

/// The offset within `around ± reach` s where the turning rates correlate best.
fn refine_gyro(v: &Signal, s: &Signal, around: f64, reach: f64) -> Option<(f64, f64)> {
    let step = 1.0 / FINE_HZ;
    let n = (reach / step) as isize;
    let pts: Vec<(f64, f64)> = (-n..=n)
        .filter_map(|k| {
            let offset = around + k as f64 * step;
            // v's sample i lines up with s's sample i + shift.
            let shift = ((v.start_s + offset - s.start_s) * FINE_HZ).round() as isize;
            let lo = (-shift).max(0) as usize;
            let hi = v
                .values
                .len()
                .min((s.values.len() as isize - shift).max(0) as usize);
            if hi <= lo + (10.0 * FINE_HZ) as usize {
                return None;
            }
            let b = &s.values[(lo as isize + shift) as usize..(hi as isize + shift) as usize];
            Some((offset, pearson(&v.values[lo..hi], b).abs()))
        })
        .collect();
    peak(&pts, step)
}

/// The highest of evenly spaced (x, y) points, placed between samples by a
/// parabola through it and its neighbors.
fn peak(pts: &[(f64, f64)], step: f64) -> Option<(f64, f64)> {
    let i = (0..pts.len()).max_by(|&a, &b| pts[a].1.total_cmp(&pts[b].1))?;
    let (x, y) = pts[i];
    if i == 0 || i + 1 == pts.len() {
        return Some((x, y));
    }
    let (y0, y2) = (pts[i - 1].1, pts[i + 1].1);
    let den = y0 - 2.0 * y + y2;
    let dx = if den.abs() > 1e-12 {
        0.5 * (y0 - y2) / den
    } else {
        0.0
    };
    Some((x + dx.clamp(-0.5, 0.5) * step, y))
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (dx, dy) = (x - ma, y - mb);
        sab += dx * dy;
        saa += dx * dx;
        sbb += dy * dy;
    }
    if saa > 0.0 && sbb > 0.0 {
        sab / (saa * sbb).sqrt()
    } else {
        0.0
    }
}
