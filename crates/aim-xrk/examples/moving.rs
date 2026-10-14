//! Seconds above 10 / 20 km/h and the RPM range while moving.
fn main() {
    for p in std::env::args().skip(1) {
        let s = aim_xrk::Session::open(std::path::Path::new(&p)).unwrap();
        let sp = s.channel("GPS Speed").unwrap();
        let dt = |th: f64| sp.times_ms.windows(2).zip(&sp.values).filter(|(_, v)| **v > th).map(|(w, _)| (w[1] - w[0]) as f64).sum::<f64>() / 1000.0;
        let rpm = s.channel("RPM").map(|c| c.values.iter().copied().fold(0.0, f64::max)).unwrap_or(-1.0);
        let dur = sp.times_ms.last().unwrap() - sp.times_ms[0];
        println!("{}  length {:.0}s  >10km/h {:.1}s  >20km/h {:.1}s  max RPM {rpm}", p.rsplit('/').next().unwrap(), dur as f64 / 1000.0, dt(10.0), dt(20.0));
    }
}
