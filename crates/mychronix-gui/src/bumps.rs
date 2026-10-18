//! Bumps and curbs the kart hits on most laps, from a MyChron's
//! accelerometers. A hit is a sharp jolt: the size of the acceleration
//! (whichever way the logger is mounted, so no mount needs working out)
//! jumping more than 0.3 g from its average over the quarter second around
//! it. One-off jolts are left out; only places hit on most racing laps count,
//! which is what a curb the driver uses, or a bump on the line, looks like.
//!
//! On a smooth track there are none. On a circuit, a kart with a MyChron 6
//! hit two places on 12 of 22 laps (a public sample, Oct 2026).

use aim_xrk::{Lap, Session};

use crate::compare::DistanceProfile;

/// A jolt this big (g) counts as a hit.
const HIT_G: f64 = 0.3;
/// Hits closer together than this (ms) are one.
const DEBOUNCE_MS: i64 = 300;
/// Hits within this share of the lap of each other are the same place.
const SAME_PLACE: f64 = 0.02;

/// A place hit on most laps.
#[derive(Debug, Clone, PartialEq)]
pub struct Bump {
    /// Where, as a share of the lap's distance (0 = the start line).
    pub at: f64,
    /// Laps that hit it, out of how many.
    pub laps: usize,
    pub of: usize,
    /// The hardest jolt there, g.
    pub peak_g: f64,
}

/// The places `session`'s racing laps (no slower than 15% off `best`) hit most often.
pub fn recurring(session: &Session, best: &Lap) -> Vec<Bump> {
    let ch = |n: &str| session.channels.iter().find(|c| c.name == n && c.values.len() > 100);
    let (Some(x), Some(y), Some(z)) = (ch("AccelerometerX"), ch("AccelerometerY"), ch("AccelerometerZ")) else { return Vec::new() };
    let n = x.values.len().min(y.values.len()).min(z.values.len());
    let size: Vec<f64> = (0..n).map(|i| (x.values[i].powi(2) + y.values[i].powi(2) + z.values[i].powi(2)).sqrt()).collect();
    // Jolt: the size against its average over about a quarter second around it.
    let jolt: Vec<f64> = (0..n)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(6), (i + 7).min(n));
            size[i] - size[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect();
    let laps: Vec<&Lap> = session.complete_laps().iter().filter(|l| l.duration_ms() as f64 <= best.duration_ms() as f64 * 1.15).collect();
    let bumps = hits_to_bumps(laps.len(), laps.iter().filter_map(|lap| {
        let profile = DistanceProfile::from_gps(session, lap)?;
        let (lo, hi) = (x.times_ms.partition_point(|&t| t < lap.start_ms), x.times_ms.partition_point(|&t| t <= lap.end_ms).min(n));
        let mut hits = Vec::new();
        let mut last: Option<i64> = None;
        for (&t, &j) in x.times_ms[lo..hi].iter().zip(&jolt[lo..hi]) {
            if j.abs() > HIT_G && last.is_none_or(|l| t - l > DEBOUNCE_MS) {
                hits.push((profile.dist_at(t as f64) / profile.length(), j.abs()));
                last = Some(t);
            }
        }
        Some(hits)
    }));
    for b in &bumps {
        log::debug!("bump at {:.0}% of the lap: {} of {} laps, up to {:.2} g", b.at * 100.0, b.laps, b.of, b.peak_g);
    }
    bumps
}

/// Groups each lap's hits (share of the lap, g) by place; keeps the places hit
/// on at least 40% of `laps` (and at least 3).
fn hits_to_bumps(laps: usize, per_lap: impl Iterator<Item = Vec<(f64, f64)>>) -> Vec<Bump> {
    let mut all: Vec<(f64, f64, usize)> = per_lap.enumerate().flat_map(|(k, hits)| hits.into_iter().map(move |(at, g)| (at, g, k))).collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let need = 3.max((laps as f64 * 0.4).ceil() as usize);
    let mut out = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() && all[j].0 - all[j - 1].0 <= SAME_PLACE {
            j += 1;
        }
        let group = &all[i..j];
        let mut which: Vec<usize> = group.iter().map(|h| h.2).collect();
        which.sort_unstable();
        which.dedup();
        if which.len() >= need {
            out.push(Bump {
                at: group.iter().map(|h| h.0).sum::<f64>() / group.len() as f64,
                laps: which.len(),
                of: laps,
                peak_g: group.iter().map(|h| h.1).fold(0.0, f64::max),
            });
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_places_hit_on_most_laps_count() {
        // Ten laps: a curb at 16% hit on 7, a bump at 73% on only 3, and a one-off at 40%.
        let laps: Vec<Vec<(f64, f64)>> = (0..10)
            .map(|k| {
                let mut h = Vec::new();
                if k < 7 {
                    h.push((0.16 + 0.002 * k as f64, 0.35));
                }
                if k % 4 == 0 {
                    h.push((0.73, 0.4));
                }
                if k == 5 {
                    h.push((0.40, 0.6));
                }
                h
            })
            .collect();
        let bumps = hits_to_bumps(10, laps.into_iter());
        assert_eq!(bumps.len(), 1);
        assert_eq!((bumps[0].laps, bumps[0].of), (7, 10));
        assert!((bumps[0].at - 0.166).abs() < 0.01);
    }
}
