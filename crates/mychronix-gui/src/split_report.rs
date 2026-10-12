//! The split times report, as RaceStudio's: each lap's time through each
//! split, the values shown absolute or against the best split or best lap,
//! fractional or cumulative, with statistics including the best theoretical
//! and best rolling laps.

/// What each cell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Values {
    #[default]
    Times,
    /// Difference to the best time for that split in the session.
    VsBestSplit,
    /// Difference to the same split of the best lap.
    VsBestLap,
}

/// One lap's row: split times in ms, or None when the lap doesn't pass every split.
#[derive(Debug, Clone)]
pub struct Row {
    pub lap_number: i32,
    pub lap_ms: i64,
    pub splits: Option<Vec<i64>>,
}

/// Best time per split over the included rows.
pub fn best_splits(rows: &[&Row], n: usize) -> Vec<Option<i64>> {
    (0..n).map(|k| rows.iter().filter_map(|r| r.splits.as_ref().map(|s| s[k])).min()).collect()
}

/// Sum of the best splits: the lap you'd do putting your best of each together.
pub fn best_theoretical(best: &[Option<i64>]) -> Option<i64> {
    best.iter().copied().sum()
}

/// The fastest run of `n` consecutive splits actually driven, possibly spanning
/// two laps (lap k's later splits then lap k+1's earlier ones). Returns the
/// time and where it starts: (index into `rows`, split index).
pub fn best_rolling(rows: &[Row], included: &[bool], n: usize) -> Option<(i64, usize, usize)> {
    // Every split driven, in order, with breaks where a lap is missing or excluded.
    let mut runs: Vec<Vec<(usize, usize, i64)>> = vec![Vec::new()];
    for (ri, r) in rows.iter().enumerate() {
        let consecutive = ri == 0 || rows[ri - 1].lap_number + 1 == r.lap_number;
        match (&r.splits, included[ri]) {
            (Some(s), true) => {
                if !consecutive {
                    runs.push(Vec::new());
                }
                runs.last_mut().unwrap().extend(s.iter().enumerate().map(|(k, &t)| (ri, k, t)));
            }
            _ => runs.push(Vec::new()),
        }
    }
    runs.iter()
        .filter(|run| run.len() >= n)
        .flat_map(|run| run.windows(n).map(|w| (w.iter().map(|x| x.2).sum::<i64>(), w[0].0, w[0].1)))
        .min_by_key(|x| x.0)
}

/// The value a cell shows, in ms.
pub fn cell(row: &[i64], k: usize, values: Values, cumulative: bool, best: &[Option<i64>], best_lap: Option<&[i64]>) -> Option<i64> {
    let reference: Option<Vec<i64>> = match values {
        Values::Times => None,
        Values::VsBestSplit => best.iter().copied().collect(),
        Values::VsBestLap => Some(best_lap?.to_vec()),
    };
    let upto = if cumulative { 0..=k } else { k..=k };
    let own: i64 = row[upto.clone()].iter().sum();
    match (values, reference) {
        (Values::Times, _) => Some(own),
        (_, Some(r)) => Some(own - r[upto].iter().sum::<i64>()),
        (_, None) => None,
    }
}

/// Average, median, slowest and standard deviation of a column.
pub fn stats(values: &[i64]) -> Option<(f64, f64, i64, f64)> {
    if values.is_empty() {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<i64>() as f64 / n;
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    let median = if sorted.len() % 2 == 0 { (sorted[mid - 1] + sorted[mid]) as f64 / 2.0 } else { sorted[mid] as f64 };
    let sd = (values.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n).sqrt();
    Some((mean, median, *sorted.last().unwrap(), sd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: i32, s: &[i64]) -> Row {
        Row { lap_number: n, lap_ms: s.iter().sum(), splits: Some(s.to_vec()) }
    }

    #[test]
    fn theoretical_and_rolling() {
        let rows = vec![row(2, &[10_000, 12_000, 9_000]), row(3, &[11_000, 10_500, 9_500]), row(4, &[10_200, 11_000, 8_800])];
        let refs: Vec<&Row> = rows.iter().collect();
        let best = best_splits(&refs, 3);
        assert_eq!(best, [Some(10_000), Some(10_500), Some(8_800)]);
        assert_eq!(best_theoretical(&best), Some(29_300));
        // Best three splits in a row: lap 3's 10.5 + 9.5 then lap 4's 10.2 = 30.2,
        // vs lap 2 (31.0), lap 3 (31.0), lap 4 (30.0) — lap 4 itself wins.
        assert_eq!(best_rolling(&rows, &[true, true, true], 3), Some((30_000, 2, 0)));
        // Leave lap 4 out: the best run is lap 2's last split into lap 3: 9.0 + 11.0 + 10.5.
        assert_eq!(best_rolling(&rows, &[true, true, false], 3), Some((30_500, 0, 2)));
    }

    #[test]
    fn rolling_does_not_jump_a_gap() {
        // Laps 2 and 5 aren't consecutive, so no run spans them.
        let rows = vec![row(2, &[10_000, 10_000]), row(5, &[1_000, 1_000])];
        assert_eq!(best_rolling(&rows, &[true, true], 2), Some((2_000, 1, 0)));
        let rows = vec![row(2, &[10_000, 5_000]), row(5, &[1_000, 10_000])];
        assert_eq!(best_rolling(&rows, &[true, true], 2), Some((11_000, 1, 0)));
    }

    #[test]
    fn cell_modes() {
        let best = [Some(10_000), Some(10_500), Some(8_800)];
        let best_lap = [10_200, 11_000, 8_800];
        let r = [11_000, 10_500, 9_500];
        assert_eq!(cell(&r, 1, Values::Times, false, &best, Some(&best_lap)), Some(10_500));
        assert_eq!(cell(&r, 1, Values::Times, true, &best, Some(&best_lap)), Some(21_500));
        assert_eq!(cell(&r, 0, Values::VsBestSplit, false, &best, Some(&best_lap)), Some(1_000));
        assert_eq!(cell(&r, 2, Values::VsBestSplit, true, &best, Some(&best_lap)), Some(1_700));
        assert_eq!(cell(&r, 1, Values::VsBestLap, false, &best, Some(&best_lap)), Some(-500));
        assert_eq!(cell(&r, 2, Values::VsBestLap, true, &best, Some(&best_lap)), Some(1_000));
    }

    #[test]
    fn column_stats() {
        let (mean, median, max, sd) = stats(&[10, 20, 30, 40]).unwrap();
        assert_eq!((mean, median, max), (25.0, 25.0, 40));
        assert!((sd - 11.180).abs() < 0.01);
    }
}
