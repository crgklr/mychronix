//! Is a .gpk/.rrk time in logger ticks or milliseconds? Compares the speed
//! match at the start and end of the session under both readings.
use aim_xrk::{drk, rs2gps};
fn main() {
    let p = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let (s, mpt) = drk::read_with_clock(&std::fs::read(&p).unwrap()).unwrap();
    let raw = rs2gps::read_rrk(&std::fs::read(p.with_extension("rrk")).unwrap());
    let sp = s.channel("GPS Speed").unwrap();
    let half = raw.len() / 2;
    for (name, scale) in [("ticks", mpt), ("ms", 1.0)] {
        let early = rs2gps::best_offset(&raw[..half], scale, sp);
        let late = rs2gps::best_offset(&raw[half..], scale, sp);
        println!("{} ms/tick {mpt:.5}: as {name}: first half {early:?}, second half {late:?}", p.file_name().unwrap().to_string_lossy());
    }
}
