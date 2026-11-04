//! What a channel measures, whatever the logger calls it: a MyChron 6 records
//! "Internal Batt" where a MyChron 4 records "Battery", and without GPS the
//! speed is Mychronix's own, worked out from RPM. Code that wants "the engine
//! speed" asks for `Role::Rpm` rather than a name.

use aim_xrk::{Channel, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Engine speed (a MyChron 4's "Engine" is named RPM when opened, see `jobs::open_session`).
    Rpm,
    /// Speed: GPS, else worked out from RPM and the gearing.
    Speed,
    ExhaustTemp,
    Battery,
    LoggerTemp,
    /// Speeding up and slowing down, g: the MyChron 6's IMU (see
    /// `aim_xrk::imu`), else the GPS's.
    ForwardG,
    /// Cornering g, from the GPS (the IMU's falls short as the chassis rolls).
    CorneringG,
}

impl Role {
    /// The names a logger records it under, the preferred first.
    pub const fn names(self) -> &'static [&'static str] {
        match self {
            Role::Rpm => &["RPM", "Engine", "Engine RPM"],
            Role::Speed => &["GPS Speed", crate::setup::SPEED_FROM_RPM],
            Role::ExhaustTemp => &["Exhaust Temp", "EGT"],
            Role::Battery => &["Internal Batt", "Battery"],
            Role::LoggerTemp => &["Logger Temperature", "Datalogger_Temp"],
            Role::ForwardG => &["Forward g", "GPS LonAcc"],
            Role::CorneringG => &["GPS LatAcc"],
        }
    }

    /// The role a channel name plays, if any.
    pub fn of(name: &str) -> Option<Role> {
        [
            Role::Rpm,
            Role::Speed,
            Role::ExhaustTemp,
            Role::Battery,
            Role::LoggerTemp,
            Role::ForwardG,
            Role::CorneringG,
        ]
        .into_iter()
        .find(|r| r.names().iter().any(|n| n.eq_ignore_ascii_case(name)))
    }
}

/// Whether a channel is the logger's own lap timing (Predictive Time, Prev
/// Lap Diff, splits…): written about once a lap against its own timing.
pub fn is_lap_timing(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("predictive") || n.contains("diff") || n.contains("lap time") || n.contains("split")
}

/// `session`'s channel for `role` that recorded something: by the role's
/// names in order (so GPS speed before speed from RPM).
pub fn find(session: &Session, role: Role) -> Option<&Channel> {
    role.names().iter().find_map(|n| {
        session
            .channels
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(n) && c.values.len() > 1)
    })
}

/// The steering angle, when the session recorded one: a sensor named for it
/// ("Steering", "Steer Angle"…), or the MyChron 6's own from its gyro.
pub fn steering(session: &Session) -> Option<&Channel> {
    session
        .channels
        .iter()
        .find(|c| c.name.to_lowercase().contains("steer") && c.values.len() > 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(name: &str) -> Channel {
        Channel {
            name: name.into(),
            group: None,
            units: String::new(),
            decimals: 0,
            interpolate: true,
            times_ms: vec![0, 1, 2],
            values: vec![1.0, 2.0, 3.0],
        }
    }

    #[test]
    fn finds_a_channel_by_what_it_measures() {
        let s = |names: &[&str]| Session {
            info: Default::default(),
            channels: names.iter().map(|n| channel(n)).collect(),
            laps: vec![],
            gps: None,
        };
        assert_eq!(
            find(&s(&["Battery", "RPM"]), Role::Battery).map(|c| c.name.as_str()),
            Some("Battery")
        );
        // GPS speed is preferred over speed worked out from RPM.
        let both = s(&[crate::setup::SPEED_FROM_RPM, "GPS Speed"]);
        assert_eq!(
            find(&both, Role::Speed).map(|c| c.name.as_str()),
            Some("GPS Speed")
        );
        assert!(find(&s(&["RPM"]), Role::ExhaustTemp).is_none());
        assert_eq!(Role::of("egt"), Some(Role::ExhaustTemp));
        assert_eq!(Role::of("Luminosity"), None);
    }
}
