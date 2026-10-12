//! Junk from the network must give errors, never panics: the logger's WiFi is
//! open, so anything can answer.

/// A small deterministic generator, so failures reproduce.
fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0..len).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; (x >> 24) as u8 }).collect()
}

#[test]
fn listings_and_frames_survive_junk() {
    let real = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/tkk-listing.bin")).unwrap();
    // Every truncation of a real listing.
    for n in 0..real.len() {
        let _ = aim_link::client::parse_listing(&real[..n]);
        let _ = aim_link::frame::parse_all(&real[..n]);
    }
    // Random bytes, and random bytes behind a plausible frame start.
    for seed in 0..2000 {
        let mut b = bytes(seed, (seed % 300) as usize);
        let _ = aim_link::client::parse_listing(&b);
        b.splice(0..0, *b"<hFLE");
        let _ = aim_link::client::parse_listing(&b);
        let _ = aim_link::frame::Frame::read_from(&mut &b[..], 1 << 20);
    }
}

#[test]
fn beacons_survive_junk() {
    for seed in 0..2000 {
        let b = bytes(seed, (seed % 400) as usize);
        let _ = aim_link::Beacon::parse(aim_link::AP_ADDRESS, std::time::Duration::ZERO, b);
    }
}
