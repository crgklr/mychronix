//! Lists LAP markers by segment for each session given.
fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).unwrap();
        let file = match aim_xrk::libxrk::read_xrk(&data) {
            Ok(f) => f,
            Err(e) => { println!("{path}: {e}"); continue; }
        };
        let token = aim_xrk::libxrk::messages::tokens::lap();
        let mut counts = std::collections::BTreeMap::new();
        let mut sample = Vec::new();
        for m in file.raw.header_messages.get(&token).map(Vec::as_slice).unwrap_or_default() {
            if m.payload.len() < 8 { continue; }
            let seg = m.payload[1];
            let lap = u16::from_le_bytes([m.payload[2], m.payload[3]]);
            let dur = u32::from_le_bytes(m.payload[4..8].try_into().unwrap());
            *counts.entry(seg).or_insert(0) += 1;
            if sample.len() < 12 { sample.push((lap, seg, dur)); }
        }
        println!("{}: segments {:?}  first (lap, seg, ms): {:?}", path.rsplit('/').next().unwrap(), counts, sample);
    }
}
