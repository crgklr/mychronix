//! Talking to AiM loggers (MyChron 5/6) over WiFi or USB.
//!
//! - [`discovery`]: find a logger with UDP `aim-ka` on port 36002.
//! - [`client`]: connect on TCP port 2000, list folders and download files.
//! - [`usb`]: the same requests over a MyChron 6's USB cable.
//! - [`Link`]: either connection, for code that doesn't care which.
//! - [`frame`]: the `<h…>…<…>` record framing both use.
//!
//! The protocol was worked out from captures of RaceStudio 3 talking to a
//! MyChron 6; see `docs/protocol-notes.md`. Only reads are implemented.

pub mod datakey;
pub mod client;
pub mod discovery;
pub mod frame;
pub mod usb;

pub use client::{DirEntry, Logger, MAX_FILE_BYTES, SESSIONS_DIR, TRACKS_DIR, is_safe_file_name};
pub use discovery::{AP_ADDRESS, Beacon, probe};

/// An open connection to a logger, by WiFi or USB: the same reads either way.
pub enum Link {
    Wifi(Logger),
    Usb(usb::UsbLogger),
}

impl Link {
    /// USB when a MyChron is plugged in, else the logger's WiFi.
    pub fn connect() -> Result<Self, Error> {
        if usb::is_present() {
            return usb::UsbLogger::open().map(Link::Usb);
        }
        Logger::connect_ap().map(Link::Wifi)
    }

    pub fn list(&mut self, dir: &str) -> Result<Vec<DirEntry>, Error> {
        match self {
            Link::Wifi(l) => l.list(dir),
            Link::Usb(l) => l.list(dir),
        }
    }

    pub fn read_file(&mut self, path: &str, progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, Error> {
        match self {
            Link::Wifi(l) => l.read_file(path, progress),
            Link::Usb(l) => l.read_file(path, progress),
        }
    }
}

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
