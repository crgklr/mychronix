//! The MyChron 4 data key (USB 11cc:0101): list and download the runs it
//! holds. Protocol in docs/protocol-notes.md; worked out from RaceStudio 2.
//!
//! Only the read-only commands RaceStudio 2 uses for a download are here:
//! identify, directory, run header, lap records and samples. The key's
//! firmware-loader, write and erase commands are deliberately not
//! implemented, so they can't be sent.

use std::time::Duration;

use nusb::MaybeFuture;
use nusb::transfer::{Buffer, Bulk, ControlOut, ControlType, In, Recipient};

pub const VID: u16 = 0x11CC;
pub const PID: u16 = 0x0101;
const DATA_IN: u8 = 0x82;
const TIMEOUT: Duration = Duration::from_secs(5);
/// Largest single sample read; longer runs are read in pieces.
const CHUNK: usize = 32 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("no MyChron 4 data key is plugged in")]
    NotFound,
    #[error("couldn't open the data key: {0}{hint}", hint = LINUX_HINT)]
    Open(String),
    #[error("the data key didn't answer ({0}); unplug it, wait a few seconds and plug it back in")]
    Transfer(String),
    #[error("the data key sent something unexpected: {0}")]
    Unexpected(String),
}

const LINUX_HINT: &str = if cfg!(target_os = "linux") { " (on Linux this needs the AiM USB rule; see the README)" } else { "" };

pub type Result<T> = std::result::Result<T, KeyError>;

/// A run listed in the key's directory, with its header.
#[derive(Debug, Clone)]
pub struct RunInfo {
    /// Position in the key's directory (what `run_header` takes).
    pub index: u16,
    /// The logger's own run number.
    pub number: u16,
    /// The logger's serial number and AiM model code (106 = MyChron 4).
    pub serial: u32,
    pub model: u16,
    pub laps_addr: u32,
    pub data_addr: u32,
    pub data_len: u32,
    /// Best lap in hundredths of a second, when the run has one.
    pub best_lap_cs: Option<u32>,
    /// The raw 256-byte header, kept with downloads.
    pub header: Vec<u8>,
}

impl RunInfo {
    pub fn laps(&self) -> usize {
        (self.data_addr.saturating_sub(self.laps_addr) / 64) as usize
    }

    /// When the run started, by the logger's clock: ("09/19/2026", "07:31:51").
    pub fn start(&self) -> Option<(String, String)> {
        let b = self.header.get(0x9A..0xA1)?;
        let d = |x: u8| (x >> 4) as u32 * 10 + (x & 0x0F) as u32;
        (d(b[5]) > 0 && d(b[3]) > 0).then(|| (format!("{:02}/{:02}/{}", d(b[5]), d(b[3]), 2000 + d(b[6])), format!("{:02}:{:02}:{:02}", d(b[2]), d(b[1]), d(b[0]))))
    }

    /// The file name a download is saved as: "20260919-073151_025.mc4".
    pub fn file_name(&self) -> String {
        match self.start() {
            Some((date, time)) => format!("{}{}{}-{}_{:03}.mc4", &date[6..10], &date[0..2], &date[3..5], time.replace(':', ""), self.number),
            None => format!("run_{:03}.mc4", self.number),
        }
    }
}

/// A downloaded run: header, lap records and samples, as the key sent them.
#[derive(Debug, Clone)]
pub struct Run {
    pub info: RunInfo,
    pub lap_records: Vec<u8>,
    pub data: Vec<u8>,
}

pub struct DataKey {
    interface: nusb::Interface,
    data_in: nusb::Endpoint<Bulk, In>,
}

/// Whether a data key is plugged in (cheap; doesn't open it).
pub fn is_present() -> bool {
    nusb::list_devices().wait().map(|mut d| d.any(|d| d.vendor_id() == VID && d.product_id() == PID)).unwrap_or(false)
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

impl DataKey {
    pub fn open() -> Result<Self> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| KeyError::Open(e.to_string()))?
            .find(|d| d.vendor_id() == VID && d.product_id() == PID)
            .ok_or(KeyError::NotFound)?;
        let device = info.open().wait().map_err(|e| KeyError::Open(e.to_string()))?;
        let interface = device.detach_and_claim_interface(0).wait().map_err(|e| KeyError::Open(e.to_string()))?;
        let data_in = interface.endpoint::<Bulk, In>(DATA_IN).map_err(|e| KeyError::Open(e.to_string()))?;
        let mut key = DataKey { interface, data_in };
        key.identify()?;
        Ok(key)
    }

    /// A vendor request with a data stage, as RaceStudio 2 sends them.
    fn vendor(&self, request: u8, index: u16, data: &[u8]) -> Result<()> {
        self.interface
            .control_out(
                ControlOut { control_type: ControlType::Vendor, recipient: Recipient::Endpoint, request, value: 0, index, data },
                TIMEOUT,
            )
            .wait()
            .map_err(|e| KeyError::Transfer(e.to_string()))
    }

    /// Reads exactly `len` bytes from the data endpoint.
    fn read(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            let want = (len - out.len()).div_ceil(64) * 64;
            let done = self.data_in.transfer_blocking(Buffer::new(want), TIMEOUT);
            done.status.map_err(|e| KeyError::Transfer(e.to_string()))?;
            if done.actual_len == 0 {
                return Err(KeyError::Transfer("no data".into()));
            }
            out.extend_from_slice(&done.buffer[..done.actual_len]);
        }
        out.truncate(len);
        Ok(out)
    }

    /// The identify step RaceStudio 2 starts with (memory request 7, then a
    /// 128-byte buffer read); returns the key's info block.
    fn identify(&mut self) -> Result<Vec<u8>> {
        self.vendor(0x0E, 0, &[0x07, 0, 0, 0, 0, 0])?;
        self.vendor(0x0C, 0x0471, &[0, 0, 0, 0x80, 0x00, 0x01])?;
        self.read(128)
    }

    /// A `1D` command: `[1D][sub][0][addr: 3 LE][0][len: u16 LE]`, then the reply.
    fn command(&mut self, sub: u8, addr: u32, len: usize) -> Result<Vec<u8>> {
        let a = addr.to_le_bytes();
        let l = (len as u16).to_le_bytes();
        self.vendor(0x0D, 0, &[0x1D, sub, 0, a[0], a[1], a[2], 0, l[0], l[1]])?;
        self.read(len)
    }

    /// The runs on the key, oldest first.
    pub fn runs(&mut self) -> Result<Vec<RunInfo>> {
        let dir = self.command(0x11, 0, 4096)?;
        let count = dir.chunks_exact(16).take_while(|e| u16_at(e, 0).is_some_and(|m| m != 0xFFFF && m != 0)).count();
        let mut runs = Vec::with_capacity(count);
        for index in 0..count as u16 {
            let h = self.command(0x12, index as u32, 256)?;
            let field = |o| u32_at(&h, o).ok_or_else(|| KeyError::Unexpected("short run header".into()));
            let (laps_addr, data_addr, data_len) = (field(136)?, field(140)?, field(150)?);
            if data_addr < laps_addr || data_len > 32 << 20 {
                return Err(KeyError::Unexpected(format!("run {index}'s header doesn't add up")));
            }
            let best = u32_at(&h, 8).filter(|&b| b > 0 && b != 0x7FFF_FFFF);
            let entry = &dir[index as usize * 16..index as usize * 16 + 16];
            runs.push(RunInfo {
                index,
                number: u16_at(&h, 0).unwrap_or(index),
                serial: u32_at(entry, 2).unwrap_or(0),
                model: u16_at(entry, 0).unwrap_or(0),
                laps_addr,
                data_addr,
                data_len,
                best_lap_cs: best,
                header: h,
            });
        }
        Ok(runs)
    }

    /// Downloads one run's lap records and samples.
    pub fn download(&mut self, info: &RunInfo, mut progress: impl FnMut(usize, usize)) -> Result<Run> {
        let lap_records = self.command(0x13, info.laps_addr, (info.data_addr - info.laps_addr) as usize)?;
        let total = (info.data_len as usize).div_ceil(64) * 64;
        let mut data = Vec::with_capacity(total);
        while data.len() < total {
            let n = (total - data.len()).min(CHUNK);
            data.extend(self.command(0x14, info.data_addr + data.len() as u32, n)?);
            progress(data.len(), total);
        }
        data.truncate(info.data_len as usize);
        Ok(Run { info: info.clone(), lap_records, data })
    }
}

impl Run {
    /// The run as one file (`.mc4`): "MC4R", version 1, then the 256-byte
    /// header, the lap records and the samples, each with a u32 length.
    pub fn to_file(&self) -> Vec<u8> {
        let mut f = b"MC4R".to_vec();
        f.extend(1u32.to_le_bytes());
        for part in [&self.info.header, &self.lap_records, &self.data] {
            f.extend((part.len() as u32).to_le_bytes());
            f.extend(part.iter());
        }
        f
    }
}
