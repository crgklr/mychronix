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

/// Session summaries and session files: junk must give errors, never panics.
#[test]
fn summaries_and_sessions_survive_junk() {
    fn bytes(seed: u64, len: usize) -> Vec<u8> {
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (0..len).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; (x >> 24) as u8 }).collect()
    }
    let xri = b"<hIMEM0000000099a><hTINF0000000048a>sess_num=35|date=10/10/2026|hour=14:19:23|<TINF003379><hLAPS0000000030a>nlap=15|best=30480|<LAPS001234><IMEM000000>";
    for n in 0..xri.len() {
        let _ = aim_xrk::parse_xri(&xri[..n]);
    }
    for seed in 0..2000 {
        let b = bytes(seed, (seed % 500) as usize);
        let _ = aim_xrk::parse_xri(&b);
        let _ = aim_xrk::parse_summary_csv(&String::from_utf8_lossy(&b));
    }
    // Session files go through a third-party reader; a panic there must come back as an error.
    for seed in 0..200 {
        let mut b = bytes(seed, 64 + (seed % 2000) as usize);
        b.splice(0..0, *b"<hCNF");
        let _ = aim_xrk::Session::from_bytes(&b);
    }
}
