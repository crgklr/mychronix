//! Session files (.xrk / .xrz) recorded by AiM loggers.
//!
//! Parsing is delegated to `libxrk`; this crate converts its output into the
//! session model the rest of Mychronix works with: every channel as `f64`
//! samples against millisecond timecodes, plus laps and descriptive metadata.

use std::path::Path;

pub mod summary;

pub use libxrk;
pub use summary::{SessionSummary, parse_summary_csv, parse_xri};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("could not parse session file: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// One recorded session.
#[derive(Debug, Clone)]
pub struct Session {
    pub info: SessionInfo,
    pub channels: Vec<Channel>,
    pub laps: Vec<Lap>,
    /// Where the kart was, when the logger had a GPS fix.
    pub gps: Option<GpsTrack>,
}

/// GPS positions in the same timebase as channel samples. All vectors have
/// the same length; points without a usable fix are left out.
#[derive(Debug, Clone, Default)]
pub struct GpsTrack {
    pub times_ms: Vec<i64>,
    /// Degrees.
    pub latitude: Vec<f64>,
    /// Degrees.
    pub longitude: Vec<f64>,
    pub speed_kmh: Vec<f64>,
    /// Rate of change of the GPS heading (from the receiver's own velocity),
    /// degrees per second, clockwise positive: turning right.
    pub yaw_rate_dps: Vec<f64>,
}

impl GpsTrack {
    pub fn len(&self) -> usize {
        self.times_ms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.times_ms.is_empty()
    }

    /// Index of the last point at or before `t_ms`.
    pub fn index_at(&self, t_ms: i64) -> Option<usize> {
        self.times_ms.partition_point(|&t| t <= t_ms).checked_sub(1)
    }

    fn from_libxrk(g: &libxrk::GpsDecodeResult) -> Option<Self> {
        let mut track = GpsTrack::default();
        for i in 0..g.timecodes.len() {
            let (lat, lon) = (g.latitude[i], g.longitude[i]);
            let fixed = g.fix.get(i).is_none_or(|&f| f > 0.0);
            if fixed && lat.is_finite() && lon.is_finite() && (lat != 0.0 || lon != 0.0) {
                track.times_ms.push(g.timecodes[i]);
                track.latitude.push(lat);
                track.longitude.push(lon);
                track.speed_kmh.push(g.speed.get(i).copied().unwrap_or(0.0) * 3.6);
                track.yaw_rate_dps.push(g.yaw_rate.get(i).map(|&v| v as f64).filter(|v| v.is_finite()).unwrap_or(0.0));
            }
        }
        (!track.is_empty()).then_some(track)
    }
}

/// Descriptive fields the logger stores with a session.
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub driver: Option<String>,
    pub vehicle: Option<String>,
    pub venue: Option<String>,
    pub date: Option<String>,
    pub time: Option<String>,
    pub session: Option<String>,
    pub device: Option<String>,
    pub logger_id: Option<u32>,
}

/// A recorded channel. `times_ms` and `values` have the same length.
#[derive(Debug, Clone)]
pub struct Channel {
    pub name: String,
    pub group: Option<String>,
    pub units: String,
    pub decimals: u8,
    pub interpolate: bool,
    pub times_ms: Vec<i64>,
    pub values: Vec<f64>,
}

/// A lap, bounded in the same timebase as channel samples. Numbered as the
/// logger numbers them: the out-lap is lap 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lap {
    pub number: i32,
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Lap {
    pub fn duration_ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }
}

impl Session {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        // The reader is third-party code working on whatever file it's given; a
        // damaged file must come back as an error, never take the app down.
        let read = std::panic::catch_unwind(|| libxrk::read_xrk(data).map(Self::from));
        match read {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(e)) => Err(Error::Parse(e.to_string())),
            Err(_) => Err(Error::Parse("the file is damaged or in a format this version can't read".into())),
        }
    }

    pub fn channel(&self, name: &str) -> Option<&Channel> {
        self.channels.iter().find(|c| c.name == name)
    }

    /// Laps with a real start and finish. The first lap runs from the start of
    /// recording to the line (the out-lap) and the last from the line to the
    /// end of recording (the in-lap), so neither is a timed lap; the logger
    /// leaves both out of its own best lap too.
    pub fn complete_laps(&self) -> &[Lap] {
        if self.laps.len() < 3 { &[] } else { &self.laps[1..self.laps.len() - 1] }
    }

    /// The fastest complete lap no shorter than `min_ms`.
    pub fn best_lap(&self, min_ms: i64) -> Option<&Lap> {
        self.complete_laps()
            .iter()
            .filter(|l| l.duration_ms() >= min_ms)
            .min_by_key(|l| l.duration_ms())
    }
}

impl From<libxrk::XrkFile> for Session {
    fn from(file: libxrk::XrkFile) -> Self {
        let m = file.metadata;
        let info = SessionInfo {
            driver: m.driver,
            vehicle: m.vehicle,
            venue: m.venue,
            date: m.log_date,
            time: m.log_time,
            session: m.session,
            device: m.device_name,
            logger_id: m.logger_id,
        };
        let channels: Vec<Channel> = file
            .channels
            .into_iter()
            .map(|c| Channel {
                name: c.name,
                group: c.group,
                units: c.units,
                decimals: c.dec_pts,
                interpolate: c.interpolate,
                times_ms: c.timecodes,
                values: to_f64(c.values),
            })
            .collect();
        let gps = file.gps.as_ref().and_then(GpsTrack::from_libxrk);
        let mut channels = channels;
        if let Some(g) = &gps {
            channels.push(Channel {
                name: "GPS Speed".into(),
                group: Some("GPS".into()),
                units: "km/h".into(),
                decimals: 1,
                interpolate: true,
                times_ms: g.times_ms.clone(),
                values: g.speed_kmh.clone(),
            });
            channels.push(Channel {
                name: "GPS LonAcc".into(),
                group: Some("GPS".into()),
                units: "g".into(),
                decimals: 2,
                interpolate: true,
                times_ms: g.times_ms.clone(),
                values: longitudinal_g(&g.times_ms, &g.speed_kmh),
            });
            channels.push(Channel {
                name: "GPS LatAcc".into(),
                group: Some("GPS".into()),
                units: "g".into(),
                decimals: 2,
                interpolate: true,
                times_ms: g.times_ms.clone(),
                values: lateral_g(&g.times_ms, &g.speed_kmh, &g.yaw_rate_dps),
            });
        }
        let laps = file
            .laps
            .into_iter()
            // libxrk counts from 0; the logger and RaceStudio count the out-lap as lap 1.
            .map(|l| Lap { number: l.num + 1, start_ms: l.start_time, end_ms: l.end_time })
            .collect();
        Session { info, channels, laps, gps }
    }
}

fn to_f64(values: libxrk::ChannelValues) -> Vec<f64> {
    use libxrk::ChannelValues as V;
    match values {
        V::UInt8(v) => v.into_iter().map(f64::from).collect(),
        V::UInt16(v) => v.into_iter().map(f64::from).collect(),
        V::Int16(v) => v.into_iter().map(f64::from).collect(),
        V::Int32(v) => v.into_iter().map(f64::from).collect(),
        V::UInt32(v) => v.into_iter().map(f64::from).collect(),
        V::Float32(v) => v.into_iter().map(f64::from).collect(),
        V::Float64(v) => v,
    }
}

/// Formats a duration as `m:ss.mmm`, the way lap times are shown on the logger.
pub fn format_lap_time(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let ms = ms.unsigned_abs();
    format!("{sign}{}:{:02}.{:03}", ms / 60_000, (ms / 1000) % 60, ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lap_time_formatting() {
        assert_eq!(format_lap_time(0), "0:00.000");
        assert_eq!(format_lap_time(54_321), "0:54.321");
        assert_eq!(format_lap_time(61_005), "1:01.005");
    }

    #[test]
    fn best_lap_ignores_out_and_in_laps() {
        let lap = |number, start_ms, end_ms| Lap { number, start_ms, end_ms };
        let session = |laps| Session { info: SessionInfo::default(), channels: vec![], laps, gps: None };
        // Out-lap 25.7 s and in-lap 24.4 s are faster than any real lap, as in a_0031/a_0032.
        let s = session(vec![lap(0, 0, 25_700), lap(1, 25_700, 57_000), lap(2, 57_000, 87_600), lap(3, 87_600, 112_000)]);
        assert_eq!(s.best_lap(10_000).map(|l| l.number), Some(2));
        // With no lap that crosses the line twice there is no best lap.
        assert!(session(vec![lap(0, 0, 20_000), lap(1, 20_000, 75_000)]).best_lap(0).is_none());
    }
}

/// Smoothing for the GPS accelerations: ±200 ms matches RaceStudio's own
/// "GPS LonAcc" and "GPS LatAcc" most closely (compared against its CSV
/// export of a full race session: correlation 0.99, about 0.03 g apart).
const GPS_ACC_HALF_MS: i64 = 200;
/// Below this RaceStudio reports both GPS accelerations as 0, so a kart
/// standing still doesn't pick up GPS jitter.
const GPS_ACC_MIN_KMH: f64 = 1.0;

/// Longitudinal acceleration in g from GPS speed: the slope over
/// [`GPS_ACC_HALF_MS`] either side, as RaceStudio's "GPS LonAcc" (negative
/// when braking).
pub fn longitudinal_g(times_ms: &[i64], speed_kmh: &[f64]) -> Vec<f64> {
    const HALF_MS: i64 = GPS_ACC_HALF_MS;
    let n = times_ms.len();
    (0..n)
        .map(|i| {
            let a = times_ms[..i].partition_point(|&t| t < times_ms[i] - HALF_MS);
            let b = (i + times_ms[i..].partition_point(|&t| t <= times_ms[i] + HALF_MS)).saturating_sub(1).max(i);
            let dt = (times_ms[b] - times_ms[a]) as f64 / 1000.0;
            if dt <= 0.0 || speed_kmh[i] < GPS_ACC_MIN_KMH { 0.0 } else { (speed_kmh[b] - speed_kmh[a]) / 3.6 / dt / 9.81 }
        })
        .collect()
}

/// Lateral acceleration in g from GPS: speed × heading turn rate, the turn
/// rate averaged over [`GPS_ACC_HALF_MS`] either side as for
/// [`longitudinal_g`]. RaceStudio's "GPS LatAcc"; positive in right-hand turns.
pub fn lateral_g(times_ms: &[i64], speed_kmh: &[f64], yaw_rate_dps: &[f64]) -> Vec<f64> {
    const HALF_MS: i64 = GPS_ACC_HALF_MS;
    let n = times_ms.len().min(speed_kmh.len()).min(yaw_rate_dps.len());
    (0..n)
        .map(|i| {
            let a = times_ms[..i].partition_point(|&t| t < times_ms[i] - HALF_MS);
            let b = (i + times_ms[i..n].partition_point(|&t| t <= times_ms[i] + HALF_MS)).max(i + 1).min(n);
            let rates = &yaw_rate_dps[a..b];
            let yaw = rates.iter().sum::<f64>() / rates.len() as f64;
            if speed_kmh[i] < GPS_ACC_MIN_KMH { 0.0 } else { speed_kmh[i] / 3.6 * yaw.to_radians() / 9.81 }
        })
        .collect()
}

#[cfg(test)]
mod lon_g_tests {
    #[test]
    fn steady_braking_reads_minus_one_g() {
        // Losing 9.81 m/s (35.3 km/h) per second, sampled at 25 Hz.
        let times: Vec<i64> = (0..50).map(|k| k * 40).collect();
        let speeds: Vec<f64> = times.iter().map(|&t| 60.0 - 35.316 * t as f64 / 1000.0).collect();
        let g = super::longitudinal_g(&times, &speeds);
        assert!((g[25] + 1.0).abs() < 0.01, "{}", g[25]);
    }

    #[test]
    fn steady_circle_reads_v_squared_over_r() {
        // 20 m/s round a 40 m radius to the right: 10 m/s² ≈ 1.02 g, turning 28.6°/s.
        let times: Vec<i64> = (0..50).map(|k| k * 40).collect();
        let speeds = vec![72.0; 50];
        let yaw = vec![(20.0f64 / 40.0).to_degrees(); 50];
        let g = super::lateral_g(&times, &speeds, &yaw);
        assert!((g[25] - 1.019).abs() < 0.01, "{}", g[25]);
    }
}
