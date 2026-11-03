//! Screenshot tests: each page of the app, opened on the made-up library
//! (`support::demo`), compared with its reference image in `tests/screens/`.
//!
//! They need a display and the same renderer the references were made with,
//! so they run only with `MYCHRONIX_SCREEN_TESTS=1`, in CI and through
//! `scripts/screens/run.sh` (Ubuntu 22.04 in a container, as CI is). There,
//! `scripts/screens/run.sh --update` rewrites the references. A page that
//! differs leaves its screenshot and a picture of the differences in
//! `target/tmp/screens/`.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

/// A page: its name, the hooks that open it (see the app's `hooks`), and
/// whether it's a session's.
type Screen = (&'static str, &'static [(&'static str, &'static str)], bool);

const SCREENS: &[Screen] = &[
    ("garage", &[("MYCHRONIX_PAGE", "garage")], false),
    ("sessions", &[("MYCHRONIX_PAGE", "sessions")], false),
    ("events", &[("MYCHRONIX_PAGE", "events")], false),
    ("tracks", &[("MYCHRONIX_PAGE", "tracks")], false),
    ("settings", &[("MYCHRONIX_PAGE", "settings")], false),
    ("graphs", &[], true),
    ("laps", &[("MYCHRONIX_REPORT", "1")], true),
    ("splits", &[("MYCHRONIX_SPLITS", "1")], true),
    ("map", &[("MYCHRONIX_VIEW", "map")], true),
    ("scatter", &[("MYCHRONIX_VIEW", "scatter")], true),
    ("histogram", &[("MYCHRONIX_VIEW", "histogram")], true),
    ("compare", &[("MYCHRONIX_COMPARE", "1")], true),
];

/// The session the session pages open: Alex's final on the last race day.
const SESSION: &str = "Mychronix/sessions/1001/Pinecrest Kart Park 01-06-30_009.drk";

/// A pixel counts as different when a channel is off by more than this
/// (renderers round antialiased edges differently)…
const CHANNEL_TOLERANCE: u8 = 24;
/// …and a page differs when more pixels than this do. The same build draws
/// the same pixels every run (screenshots wait for background work), so this
/// is small: a one-word label change is about 50.
const MAX_PIXELS: usize = 10;

fn screenshot(name: &str, hooks: &[(&str, &str)], session: bool, home: &Path, out: &Path) {
    support::demo::write_library(home);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mychronix"));
    cmd.env_clear()
        .env("HOME", home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("DISPLAY", std::env::var_os("DISPLAY").unwrap_or_default())
        .env("MYCHRONIX_WINDOWED", "1")
        .env("MYCHRONIX_TODAY", "2030-06-02")
        .env("MYCHRONIX_NO_TILES", "1")
        .env("MYCHRONIX_SCREENSHOT_FRAME", "200")
        .env("MYCHRONIX_SCREENSHOT", out)
        .envs(hooks.iter().copied());
    if let Some(v) = std::env::var_os("XAUTHORITY") {
        cmd.env("XAUTHORITY", v);
    }
    if session {
        cmd.arg(home.join(SESSION));
    }
    let status = cmd.status().expect("the app runs");
    assert!(
        status.success() && out.exists(),
        "{name}: no screenshot ({status})"
    );
}

/// How many pixels differ between two images of the same size, and a
/// picture of where (the reference, dimmed, with differing pixels red).
fn compare(a: &image::RgbaImage, b: &image::RgbaImage) -> (usize, image::RgbaImage) {
    let mut marked = a.clone();
    let mut n = 0;
    for (x, y, p) in a.enumerate_pixels() {
        let q = b.get_pixel(x, y);
        let off =
            p.0.iter()
                .zip(q.0)
                .take(3)
                .any(|(&u, v)| u.abs_diff(v) > CHANNEL_TOLERANCE);
        marked.put_pixel(
            x,
            y,
            if off {
                n += 1;
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([p[0] / 3, p[1] / 3, p[2] / 3, 255])
            },
        );
    }
    (n, marked)
}

#[test]
fn every_page_looks_as_it_did() {
    if std::env::var_os("MYCHRONIX_SCREEN_TESTS").is_none() {
        eprintln!("MYCHRONIX_SCREEN_TESTS not set; skipping (see scripts/screens/run.sh)");
        return;
    }
    let update = std::env::var_os("MYCHRONIX_SCREEN_UPDATE").is_some();
    let refs = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/screens");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("screens");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(&refs).unwrap();
    let mut failed = Vec::new();
    for &(name, hooks, session) in SCREENS {
        let home = out.join(format!("home-{name}"));
        let shot = out.join(format!("{name}.png"));
        screenshot(name, hooks, session, &home, &shot);
        let _ = std::fs::remove_dir_all(&home);
        let reference = refs.join(format!("{name}.png"));
        if update {
            std::fs::copy(&shot, &reference).unwrap();
            continue;
        }
        let Ok(want) = image::open(&reference) else {
            failed.push(format!(
                "{name}: no reference image (run scripts/screens/run.sh --update)"
            ));
            continue;
        };
        let (want, got) = (want.to_rgba8(), image::open(&shot).unwrap().to_rgba8());
        if want.dimensions() != got.dimensions() {
            failed.push(format!(
                "{name}: {:?}, the reference is {:?}",
                got.dimensions(),
                want.dimensions()
            ));
            continue;
        }
        let (n, marked) = compare(&want, &got);
        eprintln!("{name}: {n} pixels differ");
        if n > MAX_PIXELS {
            marked.save(out.join(format!("{name}.diff.png"))).unwrap();
            failed.push(format!("{name}: {n} pixels differ"));
        } else {
            let _ = std::fs::remove_file(&shot);
        }
    }
    assert!(
        failed.is_empty(),
        "pages that changed (screenshots and differences in {}):\n  {}",
        out.display(),
        failed.join("\n  ")
    );
}
