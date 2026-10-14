//! Lists a session's channels with units, sample count and range.
//! `cargo run -p aim-xrk --example channels -- <file.xrk>`
fn main() {
    for path in std::env::args().skip(1) {
        let s = aim_xrk::Session::open(std::path::Path::new(&path)).expect("open");
        println!("== {path}");
        for c in &s.channels {
            let (lo, hi) = c.values.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            let changes = c.values.windows(2).any(|w| w[0] != w[1]);
            println!("{:28} {:8} {:7} samples  {:>10.2} .. {:<10.2}{}", c.name, c.units, c.values.len(), lo, hi, if changes { "" } else { "  (constant)" });
        }
    }
}
