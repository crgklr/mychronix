//! Channels report: one row per lap, one column per channel statistic, as
//! RaceStudio's Channels Report. The chosen columns are remembered in
//! `~/Mychronix/report.json`.

use std::path::PathBuf;

use aim_xrk::{Channel, Session};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stat {
    Max,
    Min,
    Average,
    /// Largest size either way, e.g. cornering g in left and right turns.
    Peak,
    /// Share of the lap (%) spent braking, on the throttle, cornering or
    /// coasting, as RaceStudio's GPS math channels: from "GPS LonAcc"
    /// (braking below −0.15 g, throttle above +0.05 g) and "GPS LatAcc"
    /// (cornering beyond ±0.2 g), coasting when none of those.
    Braking,
    OnThrottle,
    Cornering,
    Coasting,
    /// Times the wheel changes direction by 5° or more: how often the driver
    /// corrects. Fewer is smoother.
    Corrections,
}

impl Stat {
    pub const ALL: [Stat; 4] = [Stat::Max, Stat::Min, Stat::Average, Stat::Peak];

    pub fn label(self) -> &'static str {
        match self {
            Stat::Max => "max",
            Stat::Min => "min",
            Stat::Average => "avg",
            Stat::Peak => "peak",
            Stat::Braking => "braking",
            Stat::OnThrottle => "on throttle",
            Stat::Cornering => "cornering",
            Stat::Coasting => "coasting",
            Stat::Corrections => "corrections",
        }
    }

    /// A share of the lap in %, rather than a value of the channel.
    pub fn is_share(self) -> bool {
        matches!(
            self,
            Stat::Braking | Stat::OnThrottle | Stat::Cornering | Stat::Coasting
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub channel: String,
    pub stat: Stat,
}

impl Column {
    fn new(channel: &str, stat: Stat) -> Self {
        Self {
            channel: channel.into(),
            stat,
        }
    }

    /// "RPM max", "EGT avg", "Lateral g peak", "Braking", "Steering corrections".
    pub fn title(&self) -> String {
        if self.stat == Stat::Corrections {
            return format!("{} corrections", short_name(&self.channel));
        }
        if self.stat.is_share() {
            let l = self.stat.label();
            return format!("{}{}", l[..1].to_uppercase(), &l[1..]);
        }
        format!("{} {}", short_name(&self.channel), self.stat.label())
    }
}

/// Friendlier names for the channels a MyChron records.
pub fn short_name(channel: &str) -> &str {
    match channel {
        "GPS Speed" => "Speed",
        "Exhaust Temp" => "EGT",
        // On a MyChron 6 the Y axis is lateral; its peak matches the logger's own max lateral g.
        "AccelerometerY" => "Lateral g",
        "GPS LonAcc" => "Long. g",
        "GPS LatAcc" => "Lat. g",
        "Steering Angle" => "Steering",
        other => other,
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReportConfig {
    pub columns: Vec<Column>,
}

impl ReportConfig {
    fn path() -> Option<PathBuf> {
        crate::paths::data("report.json")
    }

    pub fn load() -> Self {
        crate::storage::load_or_default(Self::path())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| std::io::Error::other("no home folder"))?;
        crate::storage::save(&path, self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Default,
    Engine,
    Driving,
    Driver,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::Default,
        Preset::Engine,
        Preset::Driving,
        Preset::Driver,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Default => "Overview",
            Preset::Engine => "Engine",
            Preset::Driving => "Driving",
            Preset::Driver => "Driver",
        }
    }

    /// The preset's columns that this session has channels for.
    pub fn columns(self, session: &Session) -> Vec<Column> {
        use Stat::*;
        let wanted: &[(&str, Stat)] = match self {
            Preset::Default => &[
                ("RPM", Max),
                ("RPM", Min),
                ("GPS Speed", Max),
                ("GPS Speed", Min),
                ("Exhaust Temp", Max),
                ("Exhaust Temp", Average),
                ("AccelerometerY", Peak),
            ],
            Preset::Engine => &[
                ("RPM", Max),
                ("RPM", Min),
                ("RPM", Average),
                ("Exhaust Temp", Max),
                ("Exhaust Temp", Min),
                ("Exhaust Temp", Average),
                ("Water Temp", Max),
                ("Internal Batt", Min),
            ],
            Preset::Driving => &[
                ("GPS Speed", Max),
                ("GPS Speed", Min),
                ("GPS Speed", Average),
                ("AccelerometerY", Peak),
                ("GPS LonAcc", Min),
                ("GPS LonAcc", Max),
            ],
            // RaceStudio's "racer performance": how each lap splits into braking,
            // throttle, cornering and coasting (from GPS and the lateral accelerometer).
            Preset::Driver => &[
                ("GPS LonAcc", OnThrottle),
                ("GPS LonAcc", Braking),
                ("GPS LatAcc", Cornering),
                ("GPS LonAcc", Coasting),
                ("GPS Speed", Min),
                ("GPS LonAcc", Min),
                ("Steering Angle", Corrections),
                ("Steering", Corrections),
                ("Steering Angle", Peak),
                ("Steering", Peak),
            ],
        };
        wanted
            .iter()
            .filter(|(name, _)| channel(session, name).is_some())
            .map(|(name, stat)| Column::new(name, *stat))
            .collect()
    }
}

/// `session`'s channel `name`, or what plays its role under another name
/// (see `channels::Role`: a MyChron 4's Battery for Internal Batt, speed from
/// RPM for GPS Speed without GPS).
pub fn channel<'a>(session: &'a Session, name: &str) -> Option<&'a Channel> {
    session
        .channels
        .iter()
        .find(|c| c.name == name && c.values.len() > 1)
        .or_else(|| crate::channels::find(session, crate::channels::Role::of(name)?))
}

/// Lateral g for cornering and coasting: from GPS as RaceStudio does, else
/// the MyChron 6's lateral accelerometer (which also picks up vibration).
const LATERAL: [&str; 2] = ["GPS LatAcc", "AccelerometerY"];

/// A report cell: `column` over [start_ms, end_ms].
/// The columns a session's Laps table shows from a saved layout: those whose
/// channel it has, never the logger's own lap timing (no number per lap to
/// show). A layout that hardly fits the session (GPS g-forces on a MyChron 4
/// without GPS) gives the overview instead; the saved one stays for the
/// sessions it suits.
pub fn shown_columns(session: &Session, saved: &[Column]) -> Vec<Column> {
    let shown: Vec<Column> = saved
        .iter()
        .filter(|c| {
            channel(session, &c.channel).is_some() && !crate::channels::is_lap_timing(&c.channel)
        })
        .cloned()
        .collect();
    if shown.len() < 2 && saved.len() >= 2 {
        Preset::Default.columns(session)
    } else {
        shown
    }
}

/// The Laps table's values, `[lap][column]`, the lap time (s) first.
pub fn table(session: &Session, columns: &[Column]) -> Vec<Vec<Option<f64>>> {
    session
        .laps
        .iter()
        .map(|lap| {
            std::iter::once(Some(lap.duration_ms() as f64 / 1000.0))
                .chain(
                    columns
                        .iter()
                        .map(|c| cell(session, c, lap.start_ms, lap.end_ms)),
                )
                .collect()
        })
        .collect()
}

/// The Laps table's summary rows, for each of its `n_cols` columns (the lap
/// time first): [`summary`] over the laps `included` (indexes into its rows).
pub fn summaries(
    table: &[Vec<Option<f64>>],
    n_cols: usize,
    included: &[usize],
) -> Vec<Option<[f64; 4]>> {
    (0..n_cols)
        .map(|k| {
            summary(
                &included
                    .iter()
                    .filter_map(|&i| table[i][k])
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Decimals to show per column of [`table`]: the lap time to the
/// millisecond, shares and counts whole, a channel at its own (at most 2).
pub fn decimals(session: &Session, columns: &[Column]) -> Vec<usize> {
    std::iter::once(3)
        .chain(columns.iter().map(|c| {
            if c.stat.is_share() || c.stat == Stat::Corrections {
                0
            } else {
                channel(session, &c.channel)
                    .map(|ch| ch.decimals.min(2) as usize)
                    .unwrap_or(1)
            }
        }))
        .collect()
}

pub fn cell(session: &Session, column: &Column, start_ms: i64, end_ms: i64) -> Option<f64> {
    let channel = self::channel(session, &column.channel)?;
    if column.stat == Stat::Corrections {
        let lo = channel.times_ms.partition_point(|&t| t < start_ms);
        let hi = channel.times_ms.partition_point(|&t| t <= end_ms);
        return (hi > lo + 1).then(|| reversals(&channel.values[lo..hi], 5.0) as f64);
    }
    if !column.stat.is_share() {
        return value(channel, start_ms, end_ms, column.stat);
    }
    let lateral = LATERAL
        .iter()
        .find_map(|name| session.channels.iter().find(|c| c.name == *name));
    let condition: Box<dyn Fn(i64, f64) -> bool> = match column.stat {
        Stat::Braking => Box::new(|_, v| v < -0.15),
        Stat::OnThrottle => Box::new(|_, v| v > 0.05),
        Stat::Cornering => Box::new(|_, v| v.abs() > 0.2),
        Stat::Coasting => {
            let lateral = lateral?;
            Box::new(move |t, v| {
                (-0.15..=0.05).contains(&v) && sample_at(lateral, t).is_some_and(|g| g.abs() <= 0.2)
            })
        }
        _ => return None,
    };
    share(channel, start_ms, end_ms, condition)
}

/// % of [start_ms, end_ms] in which `condition(time, value)` holds, weighting
/// each sample by the time until the next.
fn share(
    channel: &Channel,
    start_ms: i64,
    end_ms: i64,
    condition: impl Fn(i64, f64) -> bool,
) -> Option<f64> {
    let lo = channel.times_ms.partition_point(|&t| t < start_ms);
    let hi = channel.times_ms.partition_point(|&t| t <= end_ms);
    if hi <= lo + 1 {
        return None;
    }
    let (mut on, mut total) = (0i64, 0i64);
    for i in lo..hi {
        let t = channel.times_ms[i];
        let next = channel
            .times_ms
            .get(i + 1)
            .copied()
            .unwrap_or(end_ms)
            .min(end_ms);
        let dt = (next - t).max(0);
        let v = channel.values[i];
        if !v.is_finite() {
            continue;
        }
        total += dt;
        if condition(t, v) {
            on += dt;
        }
    }
    (total > 0).then(|| on as f64 * 100.0 / total as f64)
}

/// Direction changes of at least `threshold` (hysteresis), e.g. steering
/// corrections of 5° or more. Counts changes in direction rather than the
/// angle itself, so a drifting zero doesn't affect it.
pub fn reversals(values: &[f64], threshold: f64) -> usize {
    let (mut n, mut extreme, mut dir) = (0, values.first().copied().unwrap_or(0.0), 0i8);
    for &x in values {
        match dir {
            1 if x > extreme => extreme = x,
            -1 if x < extreme => extreme = x,
            1 if x < extreme - threshold => {
                n += 1;
                dir = -1;
                extreme = x;
            }
            -1 if x > extreme + threshold => {
                n += 1;
                dir = 1;
                extreme = x;
            }
            0 if (x - extreme).abs() >= threshold => {
                dir = if x > extreme { 1 } else { -1 };
                extreme = x;
            }
            _ => {}
        }
    }
    n
}

/// The latest sample at or before `t_ms`.
fn sample_at(channel: &Channel, t_ms: i64) -> Option<f64> {
    let i = channel.times_ms.partition_point(|&t| t <= t_ms);
    channel.values.get(i.checked_sub(1)?).copied()
}

/// `stat` of `channel` over [start_ms, end_ms].
pub fn value(channel: &Channel, start_ms: i64, end_ms: i64, stat: Stat) -> Option<f64> {
    let lo = channel.times_ms.partition_point(|&t| t < start_ms);
    let hi = channel.times_ms.partition_point(|&t| t <= end_ms);
    let values = channel
        .values
        .get(lo..hi)?
        .iter()
        .copied()
        .filter(|v| v.is_finite());
    match stat {
        Stat::Max => values.reduce(f64::max),
        Stat::Min => values.reduce(f64::min),
        Stat::Peak => values.map(f64::abs).reduce(f64::max),
        Stat::Average => {
            let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
            (n > 0).then(|| sum / n as f64)
        }
        _ => None,
    }
}

/// Max, min, average and standard deviation of a column over the included laps.
pub fn summary(values: &[f64]) -> Option<[f64; 4]> {
    if values.is_empty() {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let sd = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    Some([
        values.iter().copied().fold(f64::MIN, f64::max),
        values.iter().copied().fold(f64::MAX, f64::min),
        mean,
        sd,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(values: &[f64]) -> Channel {
        Channel {
            name: "X".into(),
            group: None,
            units: String::new(),
            decimals: 1,
            interpolate: true,
            times_ms: (0..values.len() as i64).map(|k| k * 100).collect(),
            values: values.to_vec(),
        }
    }

    #[test]
    fn stats_over_a_lap_window() {
        let c = channel(&[1.0, 5.0, -7.0, 3.0, 100.0]);
        // 0..=300 ms covers the first four samples.
        assert_eq!(value(&c, 0, 300, Stat::Max), Some(5.0));
        assert_eq!(value(&c, 0, 300, Stat::Min), Some(-7.0));
        assert_eq!(value(&c, 0, 300, Stat::Peak), Some(7.0));
        assert_eq!(value(&c, 0, 300, Stat::Average), Some(0.5));
        assert_eq!(value(&c, 1000, 2000, Stat::Max), None);
    }

    #[test]
    fn shares_of_a_lap() {
        // Longitudinal g at 100 ms steps: brake, brake, coast, throttle, throttle, throttle, (end).
        let lon = Channel {
            name: "GPS LonAcc".into(),
            ..channel(&[-0.5, -0.2, 0.0, 0.3, 0.3, 0.3, 0.0])
        };
        let lat = Channel {
            name: "GPS LatAcc".into(),
            ..channel(&[0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0])
        };
        let session = Session {
            info: Default::default(),
            channels: vec![lon, lat],
            laps: Vec::new(),
            gps: None,
        };
        let col = |name: &str, stat| Column::new(name, stat);
        let pct = |c: Column| cell(&session, &c, 0, 600).unwrap().round();
        assert_eq!(pct(col("GPS LonAcc", Stat::Braking)), 33.0);
        assert_eq!(pct(col("GPS LonAcc", Stat::OnThrottle)), 50.0);
        assert_eq!(pct(col("GPS LatAcc", Stat::Cornering)), 17.0);
        assert_eq!(pct(col("GPS LonAcc", Stat::Coasting)), 17.0);
        assert_eq!(col("GPS LonAcc", Stat::Braking).title(), "Braking");
    }

    #[test]
    fn steering_corrections() {
        // Turn in, unwind, a 2° wobble (not counted), then a correction back and forth.
        let v = [0.0, 10.0, 20.0, 10.0, 0.0, 2.0, 0.0, 8.0, 1.0, 9.0];
        assert_eq!(reversals(&v, 5.0), 4);
        assert_eq!(reversals(&[0.0, 1.0, 0.0, 1.0], 5.0), 0);
        assert_eq!(
            Column::new("Steering Angle", Stat::Corrections).title(),
            "Steering corrections"
        );
    }

    #[test]
    fn column_summary() {
        let [max, min, mean, sd] = summary(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert_eq!((max, min, mean, sd), (9.0, 2.0, 5.0, 2.0));
    }

    #[test]
    fn the_laps_table_shows_its_columns_and_a_value_per_lap() {
        let named = |name: &str, values: &[f64]| Channel {
            name: name.into(),
            ..channel(values)
        };
        let session = Session {
            info: Default::default(),
            channels: vec![
                named("RPM", &[8000.0, 9000.0, 8500.0, 9500.0]),
                named("Prev Lap Diff", &[-12.29; 4]),
            ],
            laps: vec![
                aim_xrk::Lap {
                    number: 1,
                    start_ms: 0,
                    end_ms: 150,
                },
                aim_xrk::Lap {
                    number: 2,
                    start_ms: 150,
                    end_ms: 300,
                },
            ],
            gps: None,
        };
        let col = |c: &str, stat| Column {
            channel: c.into(),
            stat,
        };
        // The logger's lap timing is never a column; a channel the session lacks isn't either.
        let saved = [
            col("RPM", Stat::Max),
            col("RPM", Stat::Min),
            col("Prev Lap Diff", Stat::Max),
            col("Exhaust Temp", Stat::Max),
        ];
        let shown = shown_columns(&session, &saved);
        assert_eq!(shown, [col("RPM", Stat::Max), col("RPM", Stat::Min)]);
        let rows = table(&session, &shown);
        assert_eq!(rows[0], [Some(0.15), Some(9000.0), Some(8000.0)]);
        assert_eq!(rows[1], [Some(0.15), Some(9500.0), Some(8500.0)]);
        // Summaries over the laps ticked: lap 2 alone, then both.
        let only_2 = summaries(&rows, 3, &[1]);
        assert_eq!(only_2[1].map(|s| s[0]), Some(9500.0));
        assert_eq!(
            summaries(&rows, 3, &[0, 1])[1].map(|s| (s[0], s[1])),
            Some((9500.0, 9000.0))
        );
        assert_eq!(decimals(&session, &shown), [3, 1, 1]);
        // No laps: still a (blank) summary per column.
        assert_eq!(summaries(&[], 3, &[]), [None, None, None]);
    }
}
