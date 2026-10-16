//! Lists each channel of the given sessions: samples, units and median value.
fn main() {
    for p in std::env::args().skip(1) {
        match aim_xrk::Session::open(&p) {
            Ok(s) => {
                println!("{}  ({} laps)", p.rsplit('/').next().unwrap(), s.laps.len());
        for c in &s.channels {
                    let mut v = c.values.clone();
                    v.sort_by(|a, b| a.total_cmp(b));
                    let med = v.get(v.len() / 2).copied().unwrap_or(f64::NAN);
                    let rate = if c.times_ms.len() > 1 { c.times_ms.len() as f64 * 1000.0 / (c.times_ms[c.times_ms.len() - 1] - c.times_ms[0]).max(1) as f64 } else { 0.0 };
                    println!("  {:<16} {:>7} samples ~{:>4.0} Hz  median {:>9.2} {}", c.name, c.values.len(), rate, med, c.units);
                }
            }
            Err(e) => println!("{}: {e}", p.rsplit('/').next().unwrap()),
        }
    }
}
