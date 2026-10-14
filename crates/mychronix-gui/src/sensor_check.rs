//! Sensor checks after a download or when a session opens (see
//! `aim_xrk::checks`). A sensor is only reported when it has recorded real
//! readings in another session from the same MyChron (or it's RPM), so
//! sensors that were never fitted stay quiet. Which sensors have worked comes
//! from the session digests (`crate::digest`); the channels you've asked not
//! to hear about again are in `~/Mychronix/sensor-check.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use aim_xrk::checks::{Issue, IssueKind};
use serde::{Deserialize, Serialize};

/// Channels not to warn about, by logger id.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ignored {
    #[serde(default)]
    pub loggers: BTreeMap<String, BTreeSet<String>>,
}

impl Ignored {
    fn path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Mychronix").join("sensor-check.json"))
    }

    pub fn load() -> Self {
        crate::storage::load_or_default(Self::path())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| std::io::Error::other("no home folder"))?;
        crate::storage::save(&path, self)
    }

    pub fn contains(&self, logger: &str, channel: &str) -> bool {
        self.loggers.get(logger).is_some_and(|c| c.contains(channel))
    }

    pub fn set(&mut self, logger: &str, channel: &str, ignore: bool) {
        let set = self.loggers.entry(logger.to_string()).or_default();
        if ignore {
            set.insert(channel.to_string());
        } else {
            set.remove(channel);
        }
    }
}

/// One session's problems.
#[derive(Debug, Clone)]
pub struct Checked {
    pub logger: String,
    /// Session number, e.g. 35.
    pub number: Option<u32>,
    pub issues: Vec<Issue>,
}

/// One problem channel across sessions, for the report.
#[derive(Debug, Clone)]
pub struct Problem {
    pub logger: String,
    pub channel: String,
    /// (session number, what was wrong) in session order.
    pub sessions: Vec<(Option<u32>, IssueKind)>,
}

/// Groups sessions' issues by logger and channel, leaving out ignored ones.
pub fn problems(checked: &[Checked], ignored: &Ignored) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    for c in checked {
        for i in &c.issues {
            if ignored.contains(&c.logger, &i.channel) {
                continue;
            }
            match out.iter_mut().find(|p| p.logger == c.logger && p.channel == i.channel) {
                Some(p) => p.sessions.push((c.number, i.kind)),
                None => out.push(Problem { logger: c.logger.clone(), channel: i.channel.clone(), sessions: vec![(c.number, i.kind)] }),
            }
        }
    }
    for p in &mut out {
        p.sessions.sort_by_key(|s| s.0);
    }
    out
}

/// What went wrong, in a few words: "no signal in sessions 8, 9 and 25".
pub fn summary(p: &Problem) -> String {
    let list = |ns: Vec<String>| match ns.len() {
        0 => String::new(),
        1 => format!("session {}", ns[0]),
        _ => format!("sessions {} and {}", ns[..ns.len() - 1].join(", "), ns[ns.len() - 1]),
    };
    let numbers = |f: &dyn Fn(&IssueKind) -> bool| p.sessions.iter().filter(|s| f(&s.1)).map(|s| s.0.map(|n| n.to_string()).unwrap_or_else(|| "?".into())).collect::<Vec<_>>();
    let mut parts = Vec::new();
    let nc = numbers(&|k| *k == IssueKind::NotConnected);
    if !nc.is_empty() {
        parts.push(format!("nothing connected in {}", list(nc)));
    }
    let ns = numbers(&|k| *k == IssueKind::NoSignal);
    if !ns.is_empty() {
        parts.push(format!("no signal on track in {}", list(ns)));
    }
    let drops: Vec<String> = p
        .sessions
        .iter()
        .filter_map(|s| match s.1 {
            IssueKind::DropOuts { percent } => Some(format!("{}% of session {}", percent, s.0.map(|n| n.to_string()).unwrap_or_else(|| "?".into()))),
            _ => None,
        })
        .collect();
    if !drops.is_empty() {
        parts.push(format!("dropped to zero on track for {}", drops.join(", ")));
    }
    let s = parts.join("; ");
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Checks `paths` in the background, learning first which sensors have worked
/// in `known` (the same loggers' other sessions; digests already cached aren't
/// worked out again). The result arrives once, with the sessions that have an
/// issue worth reporting.
pub fn spawn(paths: Vec<PathBuf>, known: Vec<PathBuf>, ctx: eframe::egui::Context) -> std::sync::mpsc::Receiver<Vec<Checked>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut digests = crate::digest::Digests::load();
        for path in known.iter().chain(&paths) {
            digests.ensure(path);
        }
        digests.save();
        let mut out = Vec::new();
        for path in paths {
            let Some(d) = digests.get(&path) else { continue };
            let issues = digests.reportable(d);
            if issues.is_empty() {
                continue;
            }
            let number = path.file_name().and_then(|n| aim_xrk::summary::session_number(&n.to_string_lossy()));
            log::info!("sensor check {}: {}", path.display(), issues.iter().map(|i| format!("{} {:?}", i.channel, i.kind)).collect::<Vec<_>>().join(", "));
            out.push(Checked { logger: d.logger.clone(), number, issues });
        }
        let _ = tx.send(out);
        ctx.request_repaint();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(channel: &str, kind: IssueKind) -> Issue {
        Issue { channel: channel.into(), kind }
    }

    #[test]
    fn grouped_by_channel_and_ignored_left_out() {
        let checked = vec![
            Checked { logger: "1".into(), number: Some(9), issues: vec![issue("RPM", IssueKind::NoSignal), issue("WheelSpeed", IssueKind::NoSignal)] },
            Checked { logger: "1".into(), number: Some(8), issues: vec![issue("RPM", IssueKind::NoSignal)] },
            Checked { logger: "1".into(), number: Some(25), issues: vec![issue("RPM", IssueKind::DropOuts { percent: 30 })] },
        ];
        let mut ignored = Ignored::default();
        ignored.set("1", "WheelSpeed", true);
        let p = problems(&checked, &ignored);
        assert_eq!(p.len(), 1);
        assert_eq!(summary(&p[0]), "No signal on track in sessions 8 and 9; dropped to zero on track for 30% of session 25");
    }
}
