//! AiM record framing, shared by the TCP protocol and the records nested in
//! its replies (directory listings, device info):
//!
//! ```text
//! '<' 'h' TAG[4] LEN:u32le FLAG:u8 '>'  payload[LEN]  '<' TAG[4] SUM:u16le '>'
//! ```
//!
//! `SUM` is the 16-bit wrapping sum of the payload bytes. Short tags are
//! padded with NUL (`"SZ\0\0"`).

use std::io::{self, Read, Write};

use crate::Error;

pub const HEADER_LEN: usize = 12;
pub const TRAILER_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub tag: [u8; 4],
    pub flag: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(tag: &[u8; 4], payload: Vec<u8>) -> Self {
        Self { tag: *tag, flag: 0, payload }
    }

    /// The tag without its NUL padding, for display and matching.
    pub fn tag_str(&self) -> &str {
        let end = self.tag.iter().position(|&b| b == 0).unwrap_or(4);
        std::str::from_utf8(&self.tag[..end]).unwrap_or("?")
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len() + TRAILER_LEN);
        out.extend_from_slice(b"<h");
        out.extend_from_slice(&self.tag);
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.push(self.flag);
        out.push(b'>');
        out.extend_from_slice(&self.payload);
        out.push(b'<');
        out.extend_from_slice(&self.tag);
        out.extend_from_slice(&checksum(&self.payload).to_le_bytes());
        out.push(b'>');
        out
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(&self.encode())
    }

    /// Reads one frame, verifying its delimiters and checksum.
    pub fn read_from(r: &mut impl Read, max_len: usize) -> Result<Self, Error> {
        let mut header = [0u8; HEADER_LEN];
        r.read_exact(&mut header)?;
        if &header[..2] != b"<h" || header[11] != b'>' {
            return Err(Error::Protocol(format!("bad frame header {:02x?}", header)));
        }
        let tag: [u8; 4] = header[2..6].try_into().unwrap();
        let len = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
        if len > max_len {
            return Err(Error::Protocol(format!("frame of {len} bytes exceeds limit {max_len}")));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        let mut trailer = [0u8; TRAILER_LEN];
        r.read_exact(&mut trailer)?;
        check_trailer(&tag, &payload, &trailer)?;
        Ok(Self { tag, flag: header[10], payload })
    }
}

/// Splits a buffer of back-to-back frames (as found inside replies).
pub fn parse_all(mut buf: &[u8]) -> Result<Vec<Frame>, Error> {
    let mut frames = Vec::new();
    while !buf.is_empty() {
        if buf.len() < HEADER_LEN + TRAILER_LEN || &buf[..2] != b"<h" || buf[11] != b'>' {
            return Err(Error::Protocol(format!("bad nested frame at {:02x?}", &buf[..buf.len().min(12)])));
        }
        let tag: [u8; 4] = buf[2..6].try_into().unwrap();
        let len = u32::from_le_bytes(buf[6..10].try_into().unwrap()) as usize;
        let end = HEADER_LEN + len + TRAILER_LEN;
        if buf.len() < end {
            return Err(Error::Protocol(format!("nested frame of {len} bytes is truncated")));
        }
        let payload = buf[HEADER_LEN..HEADER_LEN + len].to_vec();
        check_trailer(&tag, &payload, &buf[HEADER_LEN + len..end])?;
        frames.push(Frame { tag, flag: buf[10], payload });
        buf = &buf[end..];
    }
    Ok(frames)
}

pub fn checksum(payload: &[u8]) -> u16 {
    payload.iter().fold(0u16, |sum, &b| sum.wrapping_add(b as u16))
}

fn check_trailer(tag: &[u8; 4], payload: &[u8], trailer: &[u8]) -> Result<(), Error> {
    if trailer[0] != b'<' || &trailer[1..5] != tag || trailer[7] != b'>' {
        return Err(Error::Protocol(format!("bad frame trailer {:02x?}", trailer)));
    }
    let sum = u16::from_le_bytes([trailer[5], trailer[6]]);
    if sum != checksum(payload) {
        return Err(Error::Checksum { expected: sum, actual: checksum(payload) });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_matches_capture() {
        // The PC's opening message, byte for byte from the Oct 11 capture.
        let hello = Frame::new(b"STCP", vec![0, 0, 0, 0, 6, 8, 0, 0]);
        assert_eq!(
            hello.encode(),
            b"<hSTCP\x08\x00\x00\x00\x00>\x00\x00\x00\x00\x06\x08\x00\x00<STCP\x0e\x00>"
        );
    }

    #[test]
    fn round_trip_through_reader() {
        let f = Frame { tag: *b"N\0\0\0", flag: 1, payload: b"al.ria".to_vec() };
        let bytes = f.encode();
        assert_eq!(Frame::read_from(&mut &bytes[..], 1024).unwrap(), f);
        assert_eq!(f.tag_str(), "N");
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut bytes = Frame::new(b"STCP", vec![1, 2, 3]).encode();
        let n = bytes.len();
        bytes[n - 3] ^= 1;
        assert!(matches!(Frame::read_from(&mut &bytes[..], 1024), Err(Error::Checksum { .. })));
    }
}
