//! The MyChron 6 over USB (`11cc:0110`): the same read-only requests as
//! over WiFi, in a different envelope.
//!
//! - A request is the same 64-byte body as over TCP, sent as a vendor control
//!   OUT (request 1); the reply comes back on a vendor control IN (request 1).
//! - The data then arrives in chunks of up to 64,512 bytes on bulk IN `0x81`.
//!   A vendor control IN (request 2) polls `[state, length]`: state 0 means a
//!   chunk of `length` bytes is ready. After each chunk an empty control OUT
//!   (request 2) asks for the next one; after the last it ends the transfer.
//! - The state never says "finished": the file's size comes from the folder
//!   listing. A transfer left unfinished carries on at the next connection,
//!   even after a new request, so connecting reads out anything left over.
//!
//! Verified on a MyChron 6 on Oct 22, 2026 (listing, `.xri`, a 2.2 MB
//! session; byte-identical to the WiFi download). Wire facts after
//! rodrigobrim/myoverlay's notes (GPL-3.0; facts only), see
//! docs/protocol-notes.md.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use nusb::MaybeFuture;
use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Recipient};

use crate::Error;
use crate::client::{DirEntry, MAX_FILE_BYTES, cmd, parse_listing, request_body};

const VID: u16 = 0x11cc;
const PID: u16 = 0x0110;
const BULK_IN: u8 = 0x81;
const TIMEOUT: Duration = Duration::from_secs(5);
/// State: a chunk is ready to read.
const READY: u32 = 0;
/// Where the logger's settings are, which carry its identity (`SRC`/`idn`).
const SETTINGS: &str = "1:/mem/prm_cfg.cff";

/// Whether a MyChron is plugged in by USB (cheap; doesn't open it).
pub fn is_present() -> bool {
    nusb::list_devices().wait().map(|mut d| d.any(|d| d.vendor_id() == VID && d.product_id() == PID)).unwrap_or(false)
}

/// An open USB connection to a MyChron.
pub struct UsbLogger {
    interface: nusb::Interface,
    bulk_in: nusb::Endpoint<Bulk, In>,
    /// Sizes from the listings so far, by full path: the state only gives a chunk's.
    sizes: HashMap<String, u64>,
    sequence: u16,
}

fn usb_error(e: impl std::fmt::Display) -> Error {
    Error::Protocol(format!("USB: {e}"))
}

impl UsbLogger {
    pub fn open() -> Result<Self, Error> {
        let info = nusb::list_devices()
            .wait()
            .map_err(usb_error)?
            .find(|d| d.vendor_id() == VID && d.product_id() == PID)
            .ok_or_else(|| Error::Protocol("no MyChron is plugged in by USB".into()))?;
        let device = info.open().wait().map_err(usb_error)?;
        if device.active_configuration().is_err() {
            let value = device.configurations().next().map(|c| c.configuration_value()).unwrap_or(1);
            device.set_configuration(value).wait().map_err(usb_error)?;
        }
        let interface = device.detach_and_claim_interface(0).wait().map_err(usb_error)?;
        let bulk_in = interface.endpoint::<Bulk, In>(BULK_IN).map_err(usb_error)?;
        let mut logger = Self { interface, bulk_in, sizes: HashMap::new(), sequence: 0x30 };
        let leftover = logger.drain()?;
        if leftover > 0 {
            log::info!("usb: read out {leftover} bytes an earlier connection left unfinished");
        }
        Ok(logger)
    }

    fn control_in(&self, request: u8, length: u16) -> Result<Vec<u8>, Error> {
        self.interface
            .control_in(ControlIn { control_type: ControlType::Vendor, recipient: Recipient::Device, request, value: 0, index: 0, length }, TIMEOUT)
            .wait()
            .map_err(usb_error)
    }

    fn control_out(&self, request: u8, data: &[u8]) -> Result<(), Error> {
        self.interface
            .control_out(ControlOut { control_type: ControlType::Vendor, recipient: Recipient::Device, request, value: 0, index: 0, data }, TIMEOUT)
            .wait()
            .map_err(usb_error)
    }

    /// `(state, length)`.
    fn state(&self) -> Result<(u32, u32), Error> {
        let s = self.control_in(2, 8)?;
        let word = |o: usize| s.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        Ok((word(0).ok_or_else(|| usb_error("short state reply"))?, word(4).unwrap_or(0)))
    }

    /// The next chunk's length, waiting up to `wait` for it.
    fn wait_ready(&self, wait: Duration) -> Result<Option<usize>, Error> {
        let start = Instant::now();
        while start.elapsed() < wait {
            if let (READY, len) = self.state()? && len > 0 {
                return Ok(Some(len as usize));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(None)
    }

    /// Reads the ready chunk of `len` bytes, then asks for the next.
    fn chunk(&mut self, len: usize) -> Result<Vec<u8>, Error> {
        let done = self.bulk_in.transfer_blocking(Buffer::new(len.div_ceil(64) * 64), TIMEOUT);
        done.status.map_err(usb_error)?;
        let data = done.buffer[..done.actual_len.min(len)].to_vec();
        self.control_out(2, &[])?;
        Ok(data)
    }

    /// Reads out whatever an unfinished transfer left; the bytes discarded.
    fn drain(&mut self) -> Result<usize, Error> {
        let mut drained = 0;
        while let Some(len) = self.wait_ready(Duration::from_millis(300))? {
            drained += self.chunk(len)?.len();
            if drained as u64 > MAX_FILE_BYTES {
                return Err(usb_error("the MyChron keeps sending leftover data; turn it off and on"));
            }
        }
        Ok(drained)
    }

    /// Sends a read-only request; its data in full. `size` is the file's
    /// size from a listing; without one the reply is a single chunk (a listing).
    fn request(&mut self, (command, sub): (u16, u16), path: &str, size: Option<u64>, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, Error> {
        let mut body = request_body(command, sub, path)?;
        self.sequence = self.sequence.wrapping_add(1);
        body[4..6].copy_from_slice(&0x0038u16.to_le_bytes());
        body[6..8].copy_from_slice(&self.sequence.to_le_bytes());
        body[24..28].copy_from_slice(&0x0a01u32.to_le_bytes());
        self.control_out(1, &body)?;
        let reply = self.control_in(1, 64)?;
        if reply.get(8..12) != body.get(8..12) {
            return Err(Error::Protocol(format!("{path}: the MyChron answered a different request: {:02x?}", &reply[..reply.len().min(32)])));
        }
        if size.is_some_and(|s| s > MAX_FILE_BYTES) {
            return Err(Error::Protocol(format!("{path}: the logger lists {} bytes, more than any session file should be", size.unwrap_or(0))));
        }
        let mut data = Vec::new();
        loop {
            let first = data.is_empty();
            let Some(len) = self.wait_ready(if first { TIMEOUT } else { Duration::from_secs(2) })? else {
                return match size {
                    Some(s) if (data.len() as u64) < s => Err(Error::Protocol(format!("{path}: the MyChron stopped after {} of {s} bytes", data.len()))),
                    _ if first => Err(Error::Protocol(format!("{path}: no reply from the MyChron"))),
                    _ => Ok(data),
                };
            };
            data.extend(self.chunk(len)?);
            let total = size.unwrap_or(data.len() as u64);
            progress((data.len() as u64).min(total), total);
            if size.is_none_or(|s| data.len() as u64 >= s) {
                break;
            }
        }
        if let Some(s) = size {
            data.truncate(s as usize);
        }
        Ok(data)
    }

    /// Lists a folder on the logger, e.g. [`crate::SESSIONS_DIR`].
    pub fn list(&mut self, dir: &str) -> Result<Vec<DirEntry>, Error> {
        let data = self.request(cmd::LIST_DIR, dir, None, |_, _| {})?;
        let entries = parse_listing(&data)?;
        for e in &entries {
            self.sizes.insert(format!("{}/{}", dir.trim_end_matches('/'), e.name), e.size);
        }
        Ok(entries)
    }

    /// Downloads a file. Its size comes from its folder's listing (read now
    /// if it hasn't been). `progress(done, total)` is called after every chunk.
    pub fn read_file(&mut self, path: &str, progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, Error> {
        if !self.sizes.contains_key(path) && let Some((dir, _)) = path.rsplit_once('/') {
            self.list(dir)?;
        }
        let size = *self.sizes.get(path).ok_or_else(|| Error::Protocol(format!("{path}: not on the MyChron")))?;
        if size == 0 {
            return Ok(Vec::new());
        }
        self.request(cmd::READ_FILE, path, Some(size), progress)
    }

    /// The logger's serial number, from the identity record in its settings.
    pub fn logger_id(&mut self) -> Result<Option<u32>, Error> {
        Ok(identity_serial(&self.read_file(SETTINGS, |_, _| {})?))
    }
}

/// The serial in a settings file's `SRC` record, which holds the same `idn`
/// record the logger announces over WiFi (serial 12 bytes in).
fn identity_serial(settings: &[u8]) -> Option<u32> {
    let at = settings.windows(5).position(|w| w == b"<hSRC")?;
    let payload = settings.get(at + crate::frame::HEADER_LEN..)?;
    if !payload.starts_with(b"idn") {
        return None;
    }
    payload.get(12..16).map(|b| u32::from_le_bytes(b.try_into().unwrap())).filter(|&s| s != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_from_the_identity_record() {
        // As found in a MyChron 6's prm_cfg.cff (serial changed).
        let mut cff = b"<hCNF\0xxxx\x01>padding".to_vec();
        cff.extend_from_slice(b"<hSRC\0\x80\0\0\0\x01>idn\x018\0\xd0\x02\xb5\x01\0\0");
        cff.extend_from_slice(&12345678u32.to_le_bytes());
        cff.extend_from_slice(&[0; 20]);
        assert_eq!(identity_serial(&cff), Some(12345678));
        assert_eq!(identity_serial(b"<hCNF\0 no identity here"), None);
    }
}
