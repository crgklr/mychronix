//! Talking to a logger over TCP port 2000.
//!
//! Every exchange observed so far follows one shape:
//!
//! 1. On connect, both sides send an 8-byte `STCP` hello.
//! 2. The PC sends a 64-byte `STNC` request: command, sub-command, path.
//! 3. The logger acknowledges with a 64-byte `STCP` (phase `0x0a09`), then
//!    announces the reply size in another (phase `0x0a11`, size at bytes 16..20).
//! 4. The PC pulls the reply by byte offset: it sends a 4-byte `STCP` holding
//!    the offset, and the logger answers with that offset followed by up to
//!    65,472 bytes. Repeat until the size is reached.
//!
//! Only read-only commands are implemented. Anything that makes the logger ask
//! the PC for data (phase `0x0a01`, used by writes) is refused.

use std::io::{BufReader, ErrorKind, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::frame::{self, Frame};
use crate::{DATA_PORT, Error, discovery::AP_ADDRESS};

const STCP: &[u8; 4] = b"STCP";
const STNC: &[u8; 4] = b"STNC";
const HELLO: [u8; 8] = [0, 0, 0, 0, 6, 8, 0, 0];
const PHASE_ACK: u16 = 0x0a09;
const PHASE_READY: u16 = 0x0a11;
const PHASE_SEND_TO_LOGGER: u16 = 0x0a01;
/// Largest frame the logger sends: 4-byte offset + 65,472 bytes of data.
const MAX_FRAME: usize = 1 << 20;
const CHUNK_RETRIES: usize = 3;

/// Read-only commands, as (command, sub-command).
pub mod cmd {
    pub const READ_FILE: (u16, u16) = (0x02, 0x04);
    pub const LIST_DIR: (u16, u16) = (0x05, 0x04);
}

/// Folder that holds recorded sessions (`recorded=1:/mem` in the device's path table).
pub const SESSIONS_DIR: &str = "1:/mem";
/// Folder that holds the track database (`tkk=0:/tkk`).
pub const TRACKS_DIR: &str = "0:/tkk";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub size: u64,
    /// As the logger formats it, e.g. `10/10/2026`.
    pub date: String,
    /// As the logger formats it, e.g. `14:39:36`.
    pub time: String,
}

impl DirEntry {
    pub fn is_session(&self) -> bool {
        let lower = self.name.to_ascii_lowercase();
        // MyChron 6s also store some sessions as .hrz (format not known yet;
        // kept as downloaded).
        lower.ends_with(".xrz") || lower.ends_with(".hrz") || lower.ends_with(".xrk") || lower.ends_with(".drk")
    }
}

/// Parses a directory listing: one `FLE` record per file, each holding
/// `TMD` (date), `TMT` (time), `SZ` (decimal size) and `N` (name).
/// Largest file Mychronix will download (a long session is a few MB).
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// True for a plain file name that's safe to save under: letters, digits,
/// `.`, `_` and `-`, no leading dot (so no `..`, no paths, no hidden files).
/// Names come from the logger over an open WiFi network, so they're checked
/// before any is used to name a file on disk.
pub fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn parse_listing(data: &[u8]) -> Result<Vec<DirEntry>, Error> {
    let mut entries = Vec::new();
    for file in frame::parse_all(data)? {
        if file.tag_str() != "FLE" {
            continue;
        }
        let mut entry = DirEntry { name: String::new(), size: 0, date: String::new(), time: String::new() };
        for field in frame::parse_all(&file.payload)? {
            let text = String::from_utf8_lossy(&field.payload).trim_end_matches('\0').to_string();
            match field.tag_str() {
                "N" => entry.name = text,
                "SZ" => entry.size = text.trim().parse().unwrap_or(0),
                "TMD" => entry.date = text,
                "TMT" => entry.time = text,
                _ => {}
            }
        }
        if !entry.name.is_empty() {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// An open connection to a logger.
pub struct Logger {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    /// Stops the keepalive thread when the connection is dropped.
    keepalive: Arc<AtomicBool>,
}

impl Drop for Logger {
    fn drop(&mut self) {
        self.keepalive.store(false, Ordering::Relaxed);
    }
}

/// Sends `aim-ka` to the logger about once a second while the connection is
/// open, as RS3 does: loggers are reported to drop or stall a TCP connection
/// that goes a second or so without one.
fn keep_alive(addr: SocketAddr, running: Arc<AtomicBool>) {
    let target = SocketAddr::new(addr.ip(), crate::discovery::DISCOVERY_PORT);
    let socket = match UdpSocket::bind(SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), 0)) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("keepalive: couldn't open a UDP socket: {e}");
            return;
        }
    };
    let _ = socket.set_read_timeout(Some(Duration::from_millis(900)));
    let mut buf = [0u8; 512];
    let mut quiet = 0;
    while running.load(Ordering::Relaxed) {
        let _ = socket.send_to(crate::discovery::PING, target);
        // Drain the reply; a run of missing replies is the earliest sign of a hung logger.
        match socket.recv_from(&mut buf) {
            Ok(_) => quiet = 0,
            Err(_) => {
                quiet += 1;
                if quiet == 5 {
                    log::warn!("keepalive: no reply from {} for ~5 s", addr.ip());
                }
            }
        }
    }
}

impl Logger {
    /// Connects to a MyChron in access-point mode (`10.0.0.1:2000`).
    pub fn connect_ap() -> Result<Self, Error> {
        Self::connect(SocketAddr::V4(SocketAddrV4::new(AP_ADDRESS, DATA_PORT)))
    }

    pub fn connect(addr: SocketAddr) -> Result<Self, Error> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let keepalive = Arc::new(AtomicBool::new(true));
        {
            let running = keepalive.clone();
            std::thread::Builder::new().name("aim-ka".into()).spawn(move || keep_alive(addr, running))?;
        }
        let mut logger = Self { reader: BufReader::new(stream.try_clone()?), writer: stream, keepalive };
        logger.send(STCP, HELLO.to_vec())?;
        let reply = logger.recv()?;
        if reply.tag != *STCP || reply.payload.len() != 8 {
            return Err(Error::Protocol(format!("unexpected hello reply {:?}", reply.tag_str())));
        }
        Ok(logger)
    }

    /// Lists a folder on the logger, e.g. [`SESSIONS_DIR`].
    pub fn list(&mut self, dir: &str) -> Result<Vec<DirEntry>, Error> {
        let data = self.request(cmd::LIST_DIR, dir, |_, _| {})?;
        parse_listing(&data)
    }

    /// The recorded sessions, oldest first by name.
    pub fn sessions(&mut self) -> Result<Vec<DirEntry>, Error> {
        let mut entries: Vec<_> = self.list(SESSIONS_DIR)?.into_iter().filter(DirEntry::is_session).collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Downloads a file. `progress(done, total)` is called after every chunk.
    pub fn read_file(&mut self, path: &str, progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, Error> {
        self.request(cmd::READ_FILE, path, progress)
    }

    /// Sends a read-only request and pulls the whole reply.
    pub fn request(
        &mut self,
        (command, sub): (u16, u16),
        path: &str,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Vec<u8>, Error> {
        self.send(STNC, request_body(command, sub, path)?)?;

        let mut size = None;
        let mut stale = 0;
        while size.is_none() {
            let reply = self.recv()?;
            let (phase, len) = reply_phase(&reply)?;
            match phase {
                PHASE_ACK => {}
                // A payload staged for another command (another app's
                // download cut short, or newer firmware's identify bundle):
                // read it out so the logger moves on, then wait for ours.
                PHASE_READY if reply_command(&reply).is_some_and(|c| c != (command, sub)) && stale < 3 => {
                    stale += 1;
                    log::warn!("{path}: the logger had {len} bytes staged for command {:?}; clearing them", reply_command(&reply));
                    let mut got = 0u64;
                    while got < len as u64 {
                        let chunk = self.pull(got as u32)?;
                        if chunk.is_empty() {
                            break;
                        }
                        got += chunk.len() as u64;
                    }
                }
                PHASE_READY => size = Some(len as u64),
                PHASE_SEND_TO_LOGGER => {
                    return Err(Error::Protocol(format!("{path}: the logger asked for data; writes aren't supported")));
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "{path}: unexpected reply phase {other:#06x}: {:02x?}",
                        &reply.payload[..reply.payload.len().min(32)]
                    )));
                }
            }
        }
        let size = size.unwrap_or(0);
        log::debug!("{path}: {size} bytes to read");
        if size > MAX_FILE_BYTES {
            return Err(Error::Protocol(format!("{path}: the logger reports {size} bytes, more than any session file should be")));
        }

        // Grow as data arrives rather than trusting the reported size up front.
        let mut data = Vec::with_capacity((size as usize).min(4 * 1024 * 1024));
        while (data.len() as u64) < size {
            let offset = data.len() as u32;
            let chunk = self.pull(offset)?;
            if chunk.is_empty() {
                return Err(Error::Protocol(format!("{path}: empty chunk at offset {offset}")));
            }
            data.extend_from_slice(&chunk);
            progress(data.len() as u64, size);
        }
        data.truncate(size as usize);
        Ok(data)
    }

    /// Asks for the chunk at `offset`, re-asking if the logger goes quiet and
    /// skipping stale chunks left over from an earlier ask.
    fn pull(&mut self, offset: u32) -> Result<Vec<u8>, Error> {
        for attempt in 0..=CHUNK_RETRIES {
            if attempt > 0 {
                log::warn!("no reply for offset {offset}; asking again ({attempt} of {CHUNK_RETRIES})");
            }
            self.send(STCP, offset.to_le_bytes().to_vec())?;
            loop {
                match self.recv() {
                    Ok(f) if f.tag == *STCP && f.payload.len() >= 4 => {
                        let at = u32::from_le_bytes(f.payload[..4].try_into().unwrap());
                        if at == offset {
                            return Ok(f.payload[4..].to_vec());
                        }
                        // A duplicate of an earlier chunk: keep reading.
                    }
                    Ok(f) => return Err(Error::Protocol(format!("unexpected {} frame while reading", f.tag_str()))),
                    Err(Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
                    Err(e) => return Err(e),
                }
            }
        }
        Err(Error::Protocol(format!("no reply for offset {offset} after {CHUNK_RETRIES} retries")))
    }

    fn send(&mut self, tag: &[u8; 4], payload: Vec<u8>) -> Result<(), Error> {
        log::trace!("send {} {} bytes: {:02x?}", String::from_utf8_lossy(tag), payload.len(), &payload[..payload.len().min(64)]);
        Frame::new(tag, payload).write_to(&mut self.writer)?;
        self.writer.flush()?;
        Ok(())
    }

    fn recv(&mut self) -> Result<Frame, Error> {
        let frame = Frame::read_from(&mut self.reader, MAX_FRAME);
        match &frame {
            Ok(f) => log::trace!("recv {} {} bytes: {:02x?}", f.tag_str(), f.payload.len(), &f.payload[..f.payload.len().min(64)]),
            Err(e) => log::debug!("recv failed: {e}"),
        }
        frame
    }
}

/// The 64-byte request body, laid out as RS3 sends it.
/// Commands that erase or reset something on the logger, as (command, group):
/// delete file, remove folder, empty folder, format media, format data, erase
/// firmware, reset WiFi. Mychronix only reads, and refuses to build these.
const DESTRUCTIVE: [(u16, u16); 7] = [(0x06, 0x04), (0x07, 0x04), (0x08, 0x04), (0x09, 0x04), (0x0d, 0x04), (0x1000, 0x05), (0x05, 0x0c)];

pub(crate) fn request_body(command: u16, sub: u16, path: &str) -> Result<Vec<u8>, Error> {
    if DESTRUCTIVE.contains(&(command, sub)) {
        return Err(Error::Protocol(format!("refusing destructive command {command:#x}/{sub:#x}")));
    }
    if path.len() > 31 || !path.is_ascii() {
        return Err(Error::Protocol(format!("path {path:?} must be ASCII and at most 31 bytes")));
    }
    let mut b = vec![0u8; 64];
    b[8..10].copy_from_slice(&command.to_le_bytes());
    b[10..12].copy_from_slice(&sub.to_le_bytes());
    b[24..28].copy_from_slice(&1u32.to_le_bytes());
    b[32..32 + path.len()].copy_from_slice(path.as_bytes());
    Ok(b)
}

/// (phase, size) from a 64-byte `STCP` reply header.
fn reply_phase(reply: &Frame) -> Result<(u16, u32), Error> {
    if reply.tag != *STCP || reply.payload.len() != 64 {
        return Err(Error::Protocol(format!(
            "expected a 64-byte STCP reply, got {} of {} bytes",
            reply.tag_str(),
            reply.payload.len()
        )));
    }
    let p = &reply.payload;
    Ok((u16::from_le_bytes([p[24], p[25]]), u32::from_le_bytes(p[16..20].try_into().unwrap())))
}

/// The command a reply answers (bytes 8–11, as in the request), if it says.
fn reply_command(reply: &Frame) -> Option<(u16, u16)> {
    let p = reply.payload.get(8..12)?;
    let c = (u16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]]));
    (c != (0, 0)).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_commands_are_refused() {
        for (c, g) in DESTRUCTIVE {
            assert!(request_body(c, g, "1:/mem/a_0001.xrz").is_err());
        }
        assert!(request_body(cmd::READ_FILE.0, cmd::READ_FILE.1, "1:/mem/a_0001.xrz").is_ok());
    }

    #[test]
    fn request_matches_capture() {
        let body = request_body(0x02, 0x04, "1:/mem/a_0034.xrz").unwrap();
        let mut expected = vec![0u8; 64];
        expected[8..12].copy_from_slice(&[2, 0, 4, 0]);
        expected[24] = 1;
        expected[32..49].copy_from_slice(b"1:/mem/a_0034.xrz");
        assert_eq!(body, expected);
    }

    #[test]
    fn rejects_long_paths() {
        assert!(request_body(2, 4, "0:/a/very/long/path/that/does/not/fit.xrz").is_err());
    }

    #[test]
    fn names_from_the_logger_are_checked() {
        for ok in ["a_0035.xrz", "a_0035.xri", "nl2amuco.tkk", "summary1.csv"] {
            assert!(is_safe_file_name(ok), "{ok}");
        }
        for bad in ["", "..", "../x.xrz", "a/b.xrz", "..\\x", ".bashrc", "x y.xrz", "/etc/passwd", "a_0035.xrz\0"] {
            assert!(!is_safe_file_name(bad), "{bad}");
        }
    }
}
