//! Reads every .xrk/.xrz under $MYCHRONIX_SAMPLES (skipped when unset).

use std::path::{Path, PathBuf};

fn sample_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sample_files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
            Some("xrk" | "xrz")
        ) {
            out.push(path);
        }
    }
}

#[test]
fn reads_sample_sessions() {
    let Some(dir) = std::env::var_os("MYCHRONIX_SAMPLES") else {
        eprintln!("MYCHRONIX_SAMPLES not set; skipping");
        return;
    };
    let mut files = Vec::new();
    sample_files(Path::new(&dir), &mut files);
    assert!(!files.is_empty(), "no .xrk/.xrz files under {dir:?}");
    for path in files {
        let session = aim_xrk::Session::open(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert!(!session.channels.is_empty(), "{path:?}: no channels");
        for c in &session.channels {
            assert_eq!(c.times_ms.len(), c.values.len(), "{path:?}: channel {}", c.name);
        }
    }
}
