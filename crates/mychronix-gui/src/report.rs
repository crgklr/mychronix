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
}

impl Stat {
    pub const ALL: [Stat; 4] = [Stat::Max, Stat::Min, Stat::Average, Stat::Peak];

    pub fn label(self) -> &'static str {
        match self {
            Stat::Max => "max",
            Stat::Min => "min",
            Stat::Average => "avg",
            Stat::Peak => "peak",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub channel: String,
    pub stat: Stat,
}

impl Column {
    fn new(channel: &str, stat: Stat) -> Self {
        Self { channel: channel.into(), stat }
    }

    /// "RPM max", "EGT avg", "Lateral g peak".
    pub fn title(&self) -> String {
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
        other => other,
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReportConfig {
    pub columns: Vec<Column>,
}

impl ReportConfig {
    fn path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("report.json"))
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
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Default, Preset::Engine, Preset::Driving];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Default => "Overview",
            Preset::Engine => "Engine",
            Preset::Driving => "Driving",
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
        };
        wanted
            .iter()
            .filter(|(name, _)| session.channels.iter().any(|c| c.name == *name))
            .map(|(name, stat)| Column::new(name, *stat))
            .collect()
    }
}

/// `stat` of `channel` over [start_ms, end_ms].
pub fn value(channel: &Channel, start_ms: i64, end_ms: i64, stat: Stat) -> Option<f64> {
    let lo = channel.times_ms.partition_point(|&t| t < start_ms);
    let hi = channel.times_ms.partition_point(|&t| t <= end_ms);
    let values = channel.values.get(lo..hi)?.iter().copied().filter(|v| v.is_finite());
    match stat {
        Stat::Max => values.reduce(f64::max),
        Stat::Min => values.reduce(f64::min),
        Stat::Peak => values.map(f64::abs).reduce(f64::max),
        Stat::Average => {
            let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
            (n > 0).then(|| sum / n as f64)
        }
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
    Some([values.iter().copied().fold(f64::MIN, f64::max), values.iter().copied().fold(f64::MAX, f64::min), mean, sd])
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
    fn column_summary() {
        let [max, min, mean, sd] = summary(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert_eq!((max, min, mean, sd), (9.0, 2.0, 5.0, 2.0));
    }
}
