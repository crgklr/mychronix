//! Talking to AiM loggers (MyChron 5/6) over WiFi.
//!
//! - [`discovery`]: find a logger with UDP `aim-ka` on port 36002.
//! - [`client`]: connect on TCP port 2000, list folders and download files.
//! - [`frame`]: the `<h…>…<…>` record framing both use.
//!
//! The protocol was worked out from captures of RaceStudio 3 talking to a
//! MyChron 6; see `docs/protocol-notes.md`. Only reads are implemented.

pub mod datakey;
pub mod client;
pub mod discovery;
pub mod frame;

pub use client::{DirEntry, Logger, MAX_FILE_BYTES, SESSIONS_DIR, TRACKS_DIR, is_safe_file_name};
pub use discovery::{AP_ADDRESS, Beacon, probe};

/// TCP port for the logger's data protocol.
pub const DATA_PORT: u16 = 2000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("checksum mismatch: frame says {expected:#06x}, payload sums to {actual:#06x}")]
    Checksum { expected: u16, actual: u16 },
    #[error("no logger answered at {0}")]
    NotFound(std::net::Ipv4Addr),
}
