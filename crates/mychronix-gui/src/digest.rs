//! A small digest of each session file, for lists that shouldn't open every
//! session: its racing lap times (for sparklines), the sensors that recorded
//! real readings, and the sensor problems found (see `aim_xrk::checks`).
//! Worked out in the background once per file and cached in
//! `session-digests.json` in the cache folder, keyed by path and size.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aim_xrk::checks::{Issue, IssueKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Digest {
    pub logger: String,
    /// Racing laps in order: (lap number, ms).
    pub laps: Vec<(i32, i64)>,
    /// Sensor channels that recorded real readings.
    pub working: Vec<String>,
    /// Sensor problems: (channel, kind) with kind "not connected", "no signal"
    /// or "drop outs N%".
    pub issues: Vec<(String, String)>,
    /// The logger's model when the file says which, e.g. "MyChron 6".
    #[serde(default)]
    pub model: String,
    /// The laps were timed from GPS (the logger's weren't): then these stand
    /// in for the logger's summary, which still counts one long lap.
    #[serde(default)]
    pub laps_from_gps: bool,
    /// Every lap, out-lap and in-lap included, and the best (number, ms).
    #[serde(default)]
    pub lap_count: u32,
    #[serde(default)]
    pub best: Option<(u32, u64)>,
}

impl Digest {
    pub fn of(session: &aim_xrk::Session) -> Self {
        Digest {
            logger: session.info.logger_id.map(|i| i.to_string()).unwrap_or_default(),
            laps: aim_xrk::notes::racing_laps(session).iter().map(|l| (l.number, l.duration_ms())).collect(),
            working: aim_xrk::checks::working_sensors(session),
            issues: aim_xrk::checks::sensor_issues(session).iter().map(|i| (i.channel.clone(), kind_text(i.kind))).collect(),
            model: session.info.logger_model_id.and_then(aim_xrk::mychron_model).map(str::to_string).unwrap_or_default(),
            laps_from_gps: session.info.laps_from_gps,
            lap_count: session.laps.len() as u32,
            best: session.best_lap(crate::app::MIN_LAP_MS).map(|l| (l.number.max(0) as u32, l.duration_ms().max(0) as u64)),
        }
    }

    /// The issues back in `aim_xrk` form.
    pub fn issues(&self) -> Vec<Issue> {
        self.issues.iter().filter_map(|(c, k)| Some(Issue { channel: c.clone(), kind: parse_kind(k)? })).collect()
    }
}

fn kind_text(k: IssueKind) -> String {
    match k {
        IssueKind::NotConnected => "not connected".into(),
        IssueKind::NoSignal => "no signal".into(),
        IssueKind::DropOuts { percent } => format!("drop outs {percent}%"),
    }
}

fn parse_kind(s: &str) -> Option<IssueKind> {
    match s {
        "not connected" => Some(IssueKind::NotConnected),
        "no signal" => Some(IssueKind::NoSignal),
        _ => s.strip_prefix("drop outs ")?.trim_end_matches('%').parse().ok().map(|percent| IssueKind::DropOuts { percent }),
    }
}

/// All digests known so far, by session path.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Digests {
    #[serde(default)]
    by_key: BTreeMap<String, Digest>,
}

fn key(path: &Path) -> String {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    format!("{}|{size}", path.display())
}

impl Digests {
    fn file() -> Option<PathBuf> {
        // v3: laps timed from GPS (see `jobs::relap`).
        crate::paths::cache_dir().map(|d| d.join("session-digests-v3.json"))
    }

    pub fn load() -> Self {
        crate::storage::load_or_default(Self::file())
    }

    pub fn save(&self) {
        if let Some(p) = Self::file() {
            let _ = crate::storage::save(&p, self);
        }
    }

    pub fn get(&self, path: &Path) -> Option<&Digest> {
        self.by_key.get(&key(path))
    }

    /// The digest for `path`, reading the file if it isn't known yet.
    pub fn ensure(&mut self, path: &Path) -> Option<&Digest> {
        let k = key(path);
        if !self.by_key.contains_key(&k) {
            let session = crate::jobs::open_session(path).ok()?;
            self.by_key.insert(k.clone(), Digest::of(&session));
        }
        self.by_key.get(&k)
    }

    pub fn insert(&mut self, path: &Path, digest: Digest) {
        self.by_key.insert(key(path), digest);
    }

    /// The model of `logger`, when any of its sessions says.
    pub fn model(&self, logger: &str) -> Option<String> {
        self.by_key.values().find(|d| d.logger == logger && !d.model.is_empty()).map(|d| d.model.clone())
    }

    /// Whether `channel` recorded real readings in any session from `logger`.
    pub fn worked(&self, logger: &str, channel: &str) -> bool {
        self.by_key.values().any(|d| d.logger == logger && d.working.iter().any(|c| c == channel))
    }

    /// The issues in a session worth reporting (see `aim_xrk::checks::worth_reporting`).
    pub fn reportable(&self, digest: &Digest) -> Vec<Issue> {
        digest.issues().into_iter().filter(|i| aim_xrk::checks::worth_reporting(i, |c| self.worked(&digest.logger, c))).collect()
    }
}

/// Works out the digests of `paths` in the background (cached ones come back
/// at once), sending each as it's ready, then saves the cache.
pub fn spawn(paths: Vec<PathBuf>, ctx: eframe::egui::Context) -> std::sync::mpsc::Receiver<(PathBuf, Digest)> {
    let (tx, rx) = std::sync::mpsc::channel();
    crate::jobs::spawn("digests", move || {
        let mut digests = Digests::load();
        let mut changed = false;
        for (n, path) in paths.into_iter().enumerate() {
            let known = digests.get(&path).is_some();
            let Some(d) = digests.ensure(&path).cloned() else { continue };
            changed |= !known;
            if tx.send((path, d)).is_err() {
                break;
            }
            if n % 4 == 0 {
                ctx.request_repaint();
            }
        }
        if changed {
            digests.save();
        }
        ctx.request_repaint();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_kinds_round_trip() {
        for k in [IssueKind::NotConnected, IssueKind::NoSignal, IssueKind::DropOuts { percent: 30 }] {
            assert_eq!(parse_kind(&kind_text(k)), Some(k));
        }
    }
}
