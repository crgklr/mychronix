//! Sensor checks: channels that should have recorded something and didn't.
//!
//! A logger keeps writing a channel whether or not its sensor is working, so a
//! loose RPM wire shows up as a session of zeros rather than a missing
//! channel. Only real sensor inputs are checked (RPM, speed sensors,
//! temperatures, pedals, steering, pressures), never the logger's own or
//! computed channels, and only on sessions where the kart actually moved.

use crate::Session;

/// What's wrong with a channel in one session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IssueKind {
    /// Every sample was the logger's "nothing connected" value.
    NotConnected,
    /// The reading never changed while the kart was on track.
    NoSignal,
    /// RPM read zero for this share of the time the kart was moving.
    DropOuts { percent: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub channel: String,
    pub kind: IssueKind,
}

/// The kind of sensor behind a channel, for advice; None for channels not checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensor {
    Rpm,
    SpeedSensor,
    Temperature,
    Driver,
    Pressure,
}

pub fn sensor_of(channel: &str) -> Option<Sensor> {
    let n = channel.to_lowercase();
    let not = ["gps", "logger", "alarm", "predictive", "diff", "internal", "batt", "luminosity", "backlight", "wifi", "calculated", "voltage", "led", "btn", "button", "err", "warn", "_alm"];
    // Warning flags ("Oil Pressure Low") sit at zero until something's wrong.
    let flag = n.ends_with(" low") || n.ends_with(" high") || n.ends_with("_low") || n.ends_with("_high");
    if flag || not.iter().any(|w| n.contains(w)) {
        return None;
    }
    if n.contains("rpm") {
        Some(Sensor::Rpm)
    } else if n.contains("jackshaft") || n.contains("wheelspeed") || n.contains("wheel speed") || n.starts_with("vwheel") {
        Some(Sensor::SpeedSensor)
    } else if n.contains("temp") || n.contains("egt") || n.contains("exhaust") || n.contains("water") || n.contains("cht") || n.contains("head") {
        Some(Sensor::Temperature)
    } else if n.contains("throttle") || n == "tps" || n.contains("pedal") || n.contains("brake") || n.contains("steer") {
        Some(Sensor::Driver)
    } else if n.contains("press") {
        Some(Sensor::Pressure)
    } else {
        None
    }
}

/// What to check, in plain words.
pub fn advice(channel: &str) -> &'static str {
    match sensor_of(channel) {
        Some(Sensor::Rpm) => {
            "Check the RPM wire: it should be wrapped snugly around the spark plug lead (or plugged into the ignition's RPM output), with its plug fully seated. A wire that works loose shows up as RPM dropping to zero while on track."
        }
        Some(Sensor::SpeedSensor) => {
            "Check the speed sensor's gap to its magnet or teeth, and its plug. If this kart has no such sensor fitted, switch the channel off in the MyChron's settings, or tell Mychronix not to warn about it."
        }
        Some(Sensor::Temperature) => {
            "Check the temperature sensor's plug and wire. A broken thermocouple usually reads nothing at all. If nothing should be connected to this input, switch it off in the MyChron's settings, or tell Mychronix not to warn about it."
        }
        Some(Sensor::Driver) | Some(Sensor::Pressure) => {
            "Check the sensor's plug and its cable to the logger or expansion box."
        }
        None => "Check the sensor and its wiring.",
    }
}

/// Problems with the sensor channels of one session.
pub fn sensor_issues(session: &Session) -> Vec<Issue> {
    // Driven: at least 30 s above 20 km/h by GPS (a short roll or push doesn't
    // count), or (without GPS) a complete lap timed by the logger.
    // No laps are needed (practice in a parking lot has no lap beacon). A
    // session that ends far from where it started was travelling, in a car or
    // trailer, not driven on a track, so it isn't checked.
    let moved = match session.channel("GPS Speed") {
        Some(c) => {
            let fast_ms: i64 = c.times_ms.windows(2).zip(&c.values).filter(|(_, v)| **v > 20.0).map(|(w, _)| w[1] - w[0]).sum();
            fast_ms >= 30_000 && !travelled(session)
        }
        None => !session.complete_laps().is_empty(),
    };
    let mut issues = Vec::new();
    for c in &session.channels {
        if sensor_of(&c.name).is_none() {
            continue;
        }
        if c.values.is_empty() {
            issues.push(Issue { channel: c.name.clone(), kind: IssueKind::NotConnected });
            continue;
        }
        if !moved {
            continue;
        }
        if c.values.iter().all(|v| (v - c.values[0]).abs() < 1e-9) {
            issues.push(Issue { channel: c.name.clone(), kind: IssueKind::NoSignal });
            continue;
        }
        if sensor_of(&c.name) == Some(Sensor::Rpm) {
            if let Some(percent) = rpm_dropouts(session, c) {
                issues.push(Issue { channel: c.name.clone(), kind: IssueKind::DropOuts { percent } });
            }
        }
    }
    issues
}

/// The sensor channels that recorded real readings in this session: with
/// these from a logger's other sessions, a dead channel can be told apart from
/// one that was never fitted.
pub fn working_sensors(session: &Session) -> Vec<String> {
    session
        .channels
        .iter()
        .filter(|c| sensor_of(&c.name).is_some() && c.values.len() > 1 && c.values.iter().any(|v| (v - c.values[0]).abs() > 1e-9))
        .map(|c| c.name.clone())
        .collect()
}

/// Whether an issue is worth telling the driver about: RPM always (every
/// engine has an RPM wire), anything else only if it has worked before on
/// this logger. A sensor that has never recorded anything just isn't fitted.
pub fn worth_reporting(issue: &Issue, worked_before: impl Fn(&str) -> bool) -> bool {
    sensor_of(&issue.channel) == Some(Sensor::Rpm) || worked_before(&issue.channel)
}

/// Whether the session ends more than 300 m from where it started: kart
/// sessions start and finish in the pits, even on big tracks.
fn travelled(session: &Session) -> bool {
    let Some(g) = session.gps.as_ref().filter(|g| g.len() > 1) else { return false };
    let n = g.len() - 1;
    let k = 111_320.0 * g.latitude[0].to_radians().cos();
    let (dx, dy) = ((g.longitude[n] - g.longitude[0]) * k, (g.latitude[n] - g.latitude[0]) * 110_540.0);
    (dx * dx + dy * dy).sqrt() > 300.0
}

/// Share of the time above 20 km/h (GPS) with RPM under 100, when over 10%.
fn rpm_dropouts(session: &Session, rpm: &crate::Channel) -> Option<u32> {
    let speed = session.channel("GPS Speed")?;
    let (mut zero, mut n) = (0usize, 0usize);
    for (t, v) in speed.times_ms.iter().zip(&speed.values) {
        if *v > 20.0 {
            n += 1;
            let i = rpm.times_ms.partition_point(|x| x <= t).saturating_sub(1);
            if rpm.values.get(i).is_some_and(|r| *r < 100.0) {
                zero += 1;
            }
        }
    }
    (n > 50 && zero * 10 > n).then(|| (zero * 100 / n) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Channel, Lap, SessionInfo};

    fn channel(name: &str, values: Vec<f64>) -> Channel {
        let n = values.len() as i64;
        Channel { name: name.into(), group: None, units: String::new(), decimals: 1, interpolate: true, times_ms: (0..n).map(|k| k * 100).collect(), values }
    }

    fn session(channels: Vec<Channel>) -> Session {
        Session { info: SessionInfo::default(), channels, laps: Vec::<Lap>::new(), gps: None }
    }

    #[test]
    fn flags_dead_sensors_only_when_moving() {
        let speed = channel("GPS Speed", (0..400).map(|k| if k < 10 { 0.0 } else { 40.0 }).collect());
        let half = channel("RPM", (0..400).map(|k| if k % 2 == 0 { 0.0 } else { 9000.0 }).collect());
        let s = session(vec![
            speed.clone(),
            channel("RPM", vec![0.0; 400]),
            channel("WheelSpeed", vec![0.0; 400]),
            Channel { values: vec![], times_ms: vec![], ..channel("Temp TC 1", vec![]) },
            channel("Exhaust Temp", (0..400).map(|k| 400.0 + k as f64).collect()),
            channel("Luminosity", vec![100.0; 400]),
            channel("Predictive Time", vec![-12290.0; 400]),
        ]);
        let got: Vec<(String, IssueKind)> = sensor_issues(&s).into_iter().map(|i| (i.channel, i.kind)).collect();
        assert_eq!(
            got,
            [("RPM".to_string(), IssueKind::NoSignal), ("WheelSpeed".into(), IssueKind::NoSignal), ("Temp TC 1".into(), IssueKind::NotConnected)]
        );
        // RPM half zero while moving.
        let s = session(vec![speed, half]);
        assert!(matches!(sensor_issues(&s)[0].kind, IssueKind::DropOuts { percent } if (45..=55).contains(&percent)));
        // A warm-up on the stand, or a 10 s roll: a flat RPM isn't flagged.
        let stand = session(vec![channel("GPS Speed", vec![0.5; 400]), channel("RPM", vec![0.0; 400])]);
        assert!(sensor_issues(&stand).is_empty());
        let roll = session(vec![channel("GPS Speed", (0..400).map(|k| if k < 100 { 30.0 } else { 0.0 }).collect()), channel("RPM", vec![0.0; 400])]);
        assert!(sensor_issues(&roll).is_empty());
    }

    #[test]
    fn only_sensor_channels_are_checked() {
        assert_eq!(sensor_of("RPM"), Some(Sensor::Rpm));
        assert_eq!(sensor_of("Jackshaft"), Some(Sensor::SpeedSensor));
        assert_eq!(sensor_of("Temp TC 1"), Some(Sensor::Temperature));
        assert_eq!(sensor_of("Steering Angle"), Some(Sensor::Driver));
        for n in ["Logger Temperature", "GPS Speed", "EGT Alarm_1", "Calculated_Gear", "External Voltage", "Luminosity", "Best Run Diff", "Oil Pressure Low", "ECU_ERR_FUEL_PRESS"] {
            assert_eq!(sensor_of(n), None, "{n}");
        }
    }
}
