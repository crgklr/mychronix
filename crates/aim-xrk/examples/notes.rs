//! Prints a session's notes. `cargo run -p aim-xrk --example notes -- <files…>`
fn main() {
    for p in std::env::args().skip(1) {
        let s = aim_xrk::Session::open(std::path::Path::new(&p)).unwrap();
        println!("{} racing laps {:?}", p.rsplit('/').next().unwrap(), aim_xrk::notes::racing_laps(&s).iter().map(|l| l.number).collect::<Vec<_>>());
        for n in aim_xrk::notes::session_notes(&s) {
            println!("  {n:?}");
        }
    }
}
