//! The command-line tool as someone runs it: the built binary, its output and
//! exit status, against files made here (no MyChron or data key needed).

use std::path::PathBuf;
use std::process::Command;

use aim_xrk::summary::{Date, SessionSummary, to_xri};

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mychronix-cli"))
        .args(args)
        .output()
        .expect("the binary runs")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mychronix-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn help_names_the_commands() {
    let out = cli(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for command in ["info", "sessions", "device", "datakey"] {
        assert!(
            text.contains(command),
            "--help doesn't mention {command}:\n{text}"
        );
    }
}

#[test]
fn a_missing_file_is_an_error_that_names_it() {
    let out = cli(&["info", "/no/such/session.xrz"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("/no/such/session.xrz"));
}

#[test]
fn sessions_lists_a_folder_from_its_summaries() {
    let dir = temp("sessions");
    let summary = SessionSummary {
        number: Some(7),
        date: Some(Date {
            year: 2026,
            month: 10,
            day: 31,
        }),
        start_time: Some("08:55:00".into()),
        track: Some("Club Ring".into()),
        laps: Some(11),
        best_lap_ms: Some(34_365),
        ..Default::default()
    };
    std::fs::write(dir.join("a_0007.xri"), to_xri(&summary)).unwrap();
    let out = cli(&["sessions", dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("(1 sessions)"), "{text}");
    for expected in ["2026-10-31", "Club Ring", "0:34.365"] {
        assert!(text.contains(expected), "no {expected} in:\n{text}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
