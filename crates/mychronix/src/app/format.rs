//! Formatting values for the page (durations, deltas, units, thousands) and
//! small helpers about channels and laps shared by the pages.

use super::*;

pub(super) fn file_name(path: &std::path::Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// "1 lap", "3 laps".
pub(super) fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

pub(super) fn group_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(super) fn format_duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s >= 3600 { format!("{}h {:02}m", s / 3600, (s / 60) % 60) } else { format!("{}m {:02}s", s / 60, s % 60) }
}

pub(super) fn format_delta(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "+" };
    let a = ms.unsigned_abs();
    if a >= 60_000 { format!("{sign}{}", format_lap_time(a as i64)) } else { format!("{sign}{:.3}", a as f64 / 1000.0) }
}

pub(super) fn format_value(v: f64, decimals: u8) -> String {
    format!("{v:.prec$}", prec = decimals.min(4) as usize)
}

/// Units as the logger stores them, made presentable.
pub(super) fn pretty_units(units: &str) -> &str {
    match units {
        "C" => "°C",
        "F" => "°F",
        "deg" => "°",
        other => other,
    }
}

/// Laps that aren't timed laps: the out-lap (first) and in-lap (last), which
/// run to or from the ends of the recording, plus any lap far off the pace
/// (cool-downs, cautions): shorter than `MIN_LAP_MS` or 15% slower than the
/// median complete lap.
pub(crate) fn slow_laps(session: &Session) -> Vec<bool> {
    let n = session.laps.len();
    let mut times: Vec<i64> = session.complete_laps().iter().map(|l| l.duration_ms()).filter(|&d| d >= MIN_LAP_MS).collect();
    times.sort_unstable();
    let median = times.get(times.len() / 2).copied().unwrap_or(i64::MAX);
    session
        .laps
        .iter()
        .enumerate()
        .map(|(i, l)| {
            i == 0 || i + 1 == n || l.duration_ms() < MIN_LAP_MS || l.duration_ms() as f64 > median as f64 * 1.15
        })
        .collect()
}

/// Lap-timing channels are stored in milliseconds; show them in seconds.
/// Speeds and temperatures are converted to the display units too.
pub(super) fn seconds_not_ms(session: &mut Session) {
    // Speeds and temperatures in the chosen units (mph and °F, or metric).
    for c in session.channels.iter_mut() {
        crate::units::convert_channel(c);
    }
    for c in session.channels.iter_mut().filter(|c| c.units.trim() == "ms") {
        for v in &mut c.values {
            *v /= 1000.0;
        }
        c.units = "s".into();
        c.decimals = 3;
    }
}

/// A channel that never changes (e.g. lap comparisons the MyChron isn't set to calculate).
pub(super) fn is_dead(c: &Channel) -> bool {
    c.values.len() < 2 || c.values.iter().all(|v| (v - c.values[0]).abs() < 1e-9)
}

/// Sidebar groups, in order.
pub(super) const CHANNEL_GROUPS: [&str; 5] = ["Engine", "Movement", "Lap timing", "Logger", "More channels"];

/// Which group a channel is listed under. `imu`: the session has the IMU's
/// corrected channels, which stand in for the GPS's forward acceleration.
pub(super) fn channel_group(c: &Channel, imu: bool) -> &'static str {
    let n = c.name.to_lowercase();
    // The IMU's own axes (the logger's, not the kart's) and channels the
    // corrected ones replace: still there, but tucked away.
    if n.starts_with("accelerometer") || n.starts_with("gyro") || (imu && n == "gps lonacc") {
        "More channels"
    } else if n == "forward g" || n == "vertical g" || n == "grip used" {
        "Movement"
    } else if n.contains("predictive") || n.contains("diff") || n.contains("lap time") || n.contains("split") {
        "Lap timing"
    } else if n.contains("rpm") || n.contains("exhaust") || n.contains("egt") || n.contains("water") || n.contains("cht") || n.contains("throttle") || n.contains("lambda") {
        "Engine"
    } else if n.contains("speed") || n.contains("acc") || n.contains("gyro") || n.contains("gps") || n.contains("steer") || n.contains("brake") {
        "Movement"
    } else {
        "Logger"
    }
}

/// Channels shown when a session first opens: ones every MyChron records,
/// falling back to the first few channels with real data.
pub(super) fn default_channels(session: &Session) -> Vec<usize> {
    const PREFERRED: [&str; 9] = ["RPM", "Engine", "GPS Speed", crate::setup::SPEED_FROM_RPM, "Exhaust Temp", "Speed", "WheelSpeed", "Water Temp", "WT"];
    let mut picks: Vec<usize> = PREFERRED
        .iter()
        .filter_map(|name| session.channels.iter().position(|c| c.name == *name && !is_dead(c)))
        .take(3)
        .collect();
    if picks.is_empty() {
        picks = session
            .channels
            .iter()
            .enumerate()
            .filter(|(_, c)| c.values.len() > 1000)
            .map(|(i, _)| i)
            .take(3)
            .collect();
    }
    picks
}

/// The sample at or just before `t_ms` (no interpolation; see
/// `charts::value_at` for that).
pub(super) fn sample_at(c: &Channel, t_ms: i64) -> Option<f64> {
    let i = c.times_ms.partition_point(|&t| t <= t_ms);
    i.checked_sub(1).map(|i| c.values[i])
}

/// Samples within [start_ms, end_ms] as (seconds from start, value), reduced to
/// at most ~`max` points by keeping each bucket's min and max so peaks survive.
pub(super) fn decimate(c: &Channel, start_ms: i64, end_ms: i64, max: usize) -> Vec<[f64; 2]> {
    let lo = c.times_ms.partition_point(|&t| t < start_ms);
    let hi = c.times_ms.partition_point(|&t| t <= end_ms);
    let point = |i: usize| [(c.times_ms[i] - start_ms) as f64 / 1000.0, c.values[i]];
    let n = hi.saturating_sub(lo);
    if n <= max {
        return (lo..hi).map(point).collect();
    }
    let buckets = max / 2;
    let mut out = Vec::with_capacity(max);
    for b in 0..buckets {
        let a = lo + n * b / buckets;
        let z = lo + n * (b + 1) / buckets;
        let (mut min_i, mut max_i) = (a, a);
        for i in a..z {
            if c.values[i] < c.values[min_i] {
                min_i = i;
            }
            if c.values[i] > c.values[max_i] {
                max_i = i;
            }
        }
        let (first, second) = if min_i <= max_i { (min_i, max_i) } else { (max_i, min_i) };
        out.push(point(first));
        if second != first {
            out.push(point(second));
        }
    }
    out
}
