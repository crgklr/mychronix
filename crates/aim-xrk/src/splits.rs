//! The logger's own split times. When the track the MyChron is using has
//! split lines set up, it records each lap's time to every line: `LAP`
//! records with segment 1, 2, … beside the whole lap's (segment 0). libxrk
//! keeps only the whole laps, so these are read here.
//!
//! A split record holds the time from the lap's start to its line and the
//! logger's time at the line, so the two give the lap's start. Its lap
//! number is the previous lap's, so records are matched to laps by that
//! start time, never by number (checked on two sample files, Nov 2026).

use libxrk::payloads::lap::LapPayload;

use crate::Lap;

/// One lap's split lines, as the logger timed them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LapSplits {
    /// The lap's start, ms, as in [`Lap::start_ms`].
    pub lap_start_ms: i64,
    /// From the lap's start to each split line, in order, ms.
    pub cumulative_ms: Vec<i64>,
}

/// Records whose starts are this close are the same lap's (ms).
pub(crate) const SAME_START_MS: i64 = 5;

/// The split records among `payloads` (every `LAP` record's payload), with
/// `time_offset` (libxrk's) taken off their times as libxrk does for laps.
pub fn from_lap_records<'a>(
    payloads: impl IntoIterator<Item = &'a [u8]>,
    time_offset: i64,
) -> Vec<LapSplits> {
    // (start, segment, cumulative ms)
    let mut records: Vec<(i64, u8, i64)> = payloads
        .into_iter()
        .filter_map(LapPayload::parse)
        .filter(|p| p.segment != 0)
        .map(|p| {
            let cumulative = p.duration as i64;
            (
                p.end_time as i64 - time_offset - cumulative,
                p.segment,
                cumulative,
            )
        })
        .collect();
    records.sort_by_key(|r| (r.0, r.1));
    let mut out: Vec<(i64, Vec<(u8, i64)>)> = Vec::new();
    for (start, segment, cumulative) in records {
        match out.last_mut() {
            Some((s, segs)) if start - *s <= SAME_START_MS => {
                // The same record written twice.
                if !segs.iter().any(|&(k, _)| k == segment) {
                    segs.push((segment, cumulative));
                }
            }
            _ => out.push((start, vec![(segment, cumulative)])),
        }
    }
    out.into_iter()
        .filter_map(|(start, mut segs)| {
            segs.sort_by_key(|s| s.0);
            let cumulative_ms: Vec<i64> = segs.into_iter().map(|s| s.1).collect();
            // Lines in order around the lap, or it isn't trusted.
            let ordered = cumulative_ms[0] > 0 && cumulative_ms.windows(2).all(|w| w[1] > w[0]);
            ordered.then_some(LapSplits {
                lap_start_ms: start,
                cumulative_ms,
            })
        })
        .collect()
}

/// `splits` kept only where they belong to one of `laps` and every line
/// falls inside it.
pub fn matched(splits: Vec<LapSplits>, laps: &[Lap]) -> Vec<LapSplits> {
    splits
        .into_iter()
        .filter(|s| {
            laps.iter().any(|l| {
                (l.start_ms - s.lap_start_ms).abs() <= SAME_START_MS
                    && s.cumulative_ms.last().is_some_and(|&c| c < l.duration_ms())
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `LAP` payload: 20 bytes (older loggers) or 32 (newer, end time at [28:32]).
    fn payload(segment: u8, lap_num: u16, duration: u32, end: u32, long: bool) -> Vec<u8> {
        let mut p = vec![0u8; if long { 32 } else { 20 }];
        p[1] = segment;
        p[2..4].copy_from_slice(&lap_num.to_le_bytes());
        p[4..8].copy_from_slice(&duration.to_le_bytes());
        if long {
            p[16..20].copy_from_slice(&duration.to_le_bytes());
            p[28..32].copy_from_slice(&end.to_le_bytes());
        } else {
            p[16..20].copy_from_slice(&end.to_le_bytes());
        }
        p
    }

    /// The TCR sample's second lap: 134.936 s ending at 678547, splits at
    /// 33.743 and 76.232 s. Split records carry the previous lap's number.
    fn tcr(long: bool) -> Vec<Vec<u8>> {
        let (start, end) = (678_547 - 134_936, 678_547);
        vec![
            payload(0, 1, 100_000, start as u32, long),
            payload(1, 1, 33_743, (start + 33_743) as u32, long),
            payload(2, 1, 76_232, (start + 76_232) as u32, long),
            payload(0, 2, 134_936, end as u32, long),
        ]
    }

    #[test]
    fn reads_cumulative_splits_by_start_time() {
        for long in [false, true] {
            let records = tcr(long);
            let got = from_lap_records(records.iter().map(|p| p.as_slice()), 0);
            assert_eq!(
                got,
                vec![LapSplits {
                    lap_start_ms: 678_547 - 134_936,
                    cumulative_ms: vec![33_743, 76_232],
                }],
                "long payload: {long}"
            );
        }
    }

    #[test]
    fn takes_off_the_time_offset_and_matches_laps() {
        let records = tcr(true);
        let got = from_lap_records(records.iter().map(|p| p.as_slice()), 1_000);
        let start = 678_547 - 134_936 - 1_000;
        assert_eq!(got[0].lap_start_ms, start);
        let lap = |s: i64, d: i64| Lap {
            number: 3,
            start_ms: s,
            end_ms: s + d,
        };
        // Its own lap; another lap; a lap too short to hold the lines.
        assert_eq!(matched(got.clone(), &[lap(start + 2, 134_936)]).len(), 1);
        assert!(matched(got.clone(), &[lap(start + 50_000, 134_936)]).is_empty());
        assert!(matched(got, &[lap(start, 70_000)]).is_empty());
    }

    #[test]
    fn duplicates_kept_once_and_disorder_dropped() {
        let mut records = tcr(false);
        records.push(records[1].clone());
        let got = from_lap_records(records.iter().map(|p| p.as_slice()), 0);
        assert_eq!(got[0].cumulative_ms, vec![33_743, 76_232]);
        // A second line timed before the first isn't trusted.
        let bad = [
            payload(1, 1, 50_000, 150_000, false),
            payload(2, 1, 40_000, 140_000, false),
        ];
        assert!(from_lap_records(bad.iter().map(|p| p.as_slice()), 0).is_empty());
    }
}
