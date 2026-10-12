//! Track files come from the logger and from other people: junk must give
//! errors, never panics.

fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0..len).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; (x >> 24) as u8 }).collect()
}

#[test]
fn track_files_survive_junk() {
    for seed in 0..3000 {
        let b = bytes(seed, (seed % 700) as usize);
        let _ = aim_track::parse_ria(&b);
        let _ = aim_track::parse_tkk(&b);
        // Behind the start of a real .tkk record.
        let mut t = b"<hPtkk".to_vec();
        t.extend_from_slice(&b);
        let _ = aim_track::parse_tkk(&t);
    }
}
