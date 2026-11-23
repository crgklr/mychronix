//! Synced videos, kept beside the session as `<session>.video.json` (e.g.
//! `a_0108.video.json` next to `a_0108.xrz`). This file is what other tools,
//! such as a video editor plugin, read:
//!
//! ```json
//! { "version": 1,
//!   "videos": [ { "file": "/path/GX010238.MP4", "bytes": 3971281275,
//!                 "duration_s": 704.337, "offset_s": -61.84, ... } ] }
//! ```
//!
//! Session time (the session's own clock, as its channels are timed) =
//! video time + `offset_s`. One entry per video file; syncing a file again
//! replaces its entry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Match, VideoSignals};

/// The sidecar's format version; bump it when a field changes meaning.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Videos {
    pub version: u32,
    pub videos: Vec<SyncedVideo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncedVideo {
    /// Where the video was when it was synced.
    pub file: PathBuf,
    /// Its size then, to recognize it if it moves.
    pub bytes: u64,
    pub duration_s: f64,
    #[serde(flatten)]
    pub sync: Match,
    /// Set by hand rather than found.
    #[serde(default)]
    pub manual: bool,
}

/// The sidecar for a session file.
pub fn path_for(session: &Path) -> PathBuf {
    session.with_extension("video.json")
}

pub fn load(session: &Path) -> Result<Videos, Error> {
    match std::fs::read(path_for(session)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Videos::default()),
        Err(e) => Err(e.into()),
    }
}

/// Records `video`'s sync with `session`, replacing an earlier one for the same file.
pub fn save(session: &Path, video: &VideoSignals, sync: &Match) -> Result<(), Error> {
    let mut all = load(session)?;
    all.version = VERSION;
    all.videos.retain(|v| v.file != video.path);
    all.videos.push(SyncedVideo {
        file: video.path.clone(),
        bytes: video.bytes,
        duration_s: video.duration_s,
        sync: sync.clone(),
        manual: false,
    });
    let path = path_for(session);
    let part = path.with_extension("json.part");
    std::fs::write(&part, serde_json::to_vec_pretty(&all)?)?;
    std::fs::rename(&part, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_sits_beside_the_session() {
        assert_eq!(
            path_for(Path::new("/x/a_0108.xrz")),
            Path::new("/x/a_0108.video.json")
        );
        assert_eq!(
            path_for(Path::new("/x/a_0001-2.xrz")),
            Path::new("/x/a_0001-2.video.json")
        );
    }
}
