//! Finding a logger: UDP `aim-ka` on port 36002.
//!
//! RS3 sends the six ASCII bytes `aim-ka` from UDP 36002 to the logger every
//! two seconds; the logger answers each with a 236-byte status record. Only
//! the fields below are understood so far; the rest is kept raw.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use crate::Error;

pub const DISCOVERY_PORT: u16 = 36002;
pub const PING: &[u8] = b"aim-ka";
/// A MyChron in access-point mode is always here.
pub const AP_ADDRESS: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);

#[derive(Debug, Clone)]
pub struct Beacon {
    /// Where the reply came from.
    pub address: Ipv4Addr,
    /// The logger's serial number (e.g. `12345678`, whose WiFi is `AiM-MYC6-345678`).
    pub logger_id: Option<u32>,
    /// How long the logger took to answer.
    pub round_trip: Duration,
    pub raw: Vec<u8>,
}

impl Beacon {
    pub fn parse(address: Ipv4Addr, round_trip: Duration, raw: Vec<u8>) -> Result<Self, Error> {
        // u32 length at 0 (0xec = 236), the logger's IPv4 at 8 (big-endian),
        // and the logger id at 96 inside the record that starts "idn" at 84.
        if raw.len() < 100 || u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize != raw.len() {
            return Err(Error::Protocol(format!("unexpected aim-ka reply of {} bytes", raw.len())));
        }
        let logger_id = (&raw[84..87] == b"idn").then(|| u32::from_le_bytes(raw[96..100].try_into().unwrap()));
        Ok(Self { address, logger_id, round_trip, raw })
    }
}

/// Sends `aim-ka` to `target` and returns the first valid reply.
///
/// Tries to send from port 36002 like RS3 does; falls back to any free port
/// when something else (RS3 itself) already holds it. The logger answers
/// either way.
pub fn probe(target: Ipv4Addr, timeout: Duration) -> Result<Beacon, Error> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT))
        .or_else(|_| UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)))?;
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 2048];
    loop {
        let sent = Instant::now();
        socket.send_to(PING, SocketAddrV4::new(target, DISCOVERY_PORT))?;
        socket.set_read_timeout(Some(Duration::from_millis(500).min(deadline.saturating_duration_since(sent).max(Duration::from_millis(1)))))?;
        match socket.recv_from(&mut buf) {
            Ok((n, std::net::SocketAddr::V4(from))) if *from.ip() == target => {
                return Beacon::parse(*from.ip(), sent.elapsed(), buf[..n].to_vec());
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.into()),
        }
        if Instant::now() >= deadline {
            return Err(Error::NotFound(target));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply shaped like the real one, with a made-up logger id.
    pub fn sample_reply(logger_id: u32) -> Vec<u8> {
        let mut r = vec![0u8; 236];
        r[0] = 0xec;
        r[4] = 2;
        r[8..12].copy_from_slice(&[10, 0, 0, 1]);
        r[84..87].copy_from_slice(b"idn");
        r[96..100].copy_from_slice(&logger_id.to_le_bytes());
        r
    }

    #[test]
    fn parses_logger_id() {
        let b = Beacon::parse(AP_ADDRESS, Duration::from_millis(3), sample_reply(12_345_678)).unwrap();
        assert_eq!(b.logger_id, Some(12_345_678));
    }

    #[test]
    fn rejects_wrong_length() {
        let mut r = sample_reply(1);
        r[0] = 0x10;
        assert!(Beacon::parse(AP_ADDRESS, Duration::ZERO, r).is_err());
    }
}
