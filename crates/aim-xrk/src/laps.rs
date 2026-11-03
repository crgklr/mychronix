//! Which laps count: one set of rules for every reader, the app and its
//! assistant server, so a session's best lap is the same in the list, in
//! Analysis and in the assistant's answers.
//!
//! The out-lap and in-lap (first and last) run to or from the ends of the
//! recording, so they're never timed laps. Of the rest:
//! - shorter than [`MIN_LAP_MS`], or under 70% of the median lap, it was cut
//!   short (a second beacon, or the pit lane crossing the line): not a lap;
//! - over 115% of the median, it was a slow lap (a cool-down, a caution, a
//!   spin): a lap, and it can't be the best, but it isn't raced.

use crate::{Lap, Session};

/// No kart lap is shorter.
pub const MIN_LAP_MS: i64 = 10_000;

/// Under this share of the median lap: cut short.
const SHORT: f64 = 0.7;
/// Over this share of the median lap: a slow lap.
const SLOW: f64 = 1.15;

/// The median of the complete laps no shorter than `min_ms`.
fn median_ms(session: &Session, min_ms: i64) -> Option<i64> {
    let mut d: Vec<i64> = session.complete_laps().iter().map(Lap::duration_ms).filter(|&d| d >= min_ms).collect();
    d.sort_unstable();
    d.get(d.len() / 2).copied()
}

/// The shortest a complete lap can be and still be a real lap: `min_ms`, or
/// 70% of the median lap when that's longer.
pub fn shortest(session: &Session, min_ms: i64) -> i64 {
    median_ms(session, min_ms).map_or(min_ms, |m| min_ms.max((m as f64 * SHORT) as i64))
}

/// For each of `session.laps`, whether it's a racing lap: the laps that
/// count toward bests, consistency and pace.
pub fn racing(session: &Session) -> Vec<bool> {
    let n = session.laps.len();
    let (short, median) = (shortest(session, MIN_LAP_MS), median_ms(session, MIN_LAP_MS));
    session
        .laps
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let d = l.duration_ms();
            n >= 3 && i > 0 && i + 1 < n && d >= short && median.is_some_and(|m| d as f64 <= m as f64 * SLOW)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionInfo;

    fn session(times_s: &[f64]) -> Session {
        let mut t = 0;
        let laps = times_s
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let start = t;
                t += (s * 1000.0) as i64;
                Lap { number: i as i32, start_ms: start, end_ms: t }
            })
            .collect();
        Session { info: SessionInfo::default(), channels: vec![], laps, gps: None }
    }

    #[test]
    fn a_lap_cut_short_is_never_the_best() {
        // Out-lap, laps of about 30 s, one cut short by a second beacon (15.5 s), a slow lap, in-lap.
        let s = session(&[50.0, 30.4, 30.1, 15.5, 30.6, 41.0, 20.0]);
        assert_eq!(s.best_lap(MIN_LAP_MS).map(|l| l.number), Some(2));
        assert_eq!(racing(&s), [false, true, true, false, true, false, false]);
    }

    #[test]
    fn too_few_laps_have_none_that_count() {
        assert_eq!(racing(&session(&[40.0, 30.0])), [false, false]);
        assert!(session(&[40.0, 30.0]).best_lap(MIN_LAP_MS).is_none());
    }
}
