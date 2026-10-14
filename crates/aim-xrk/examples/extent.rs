//! How far a session's GPS track spreads, and where it starts and ends relative to the middle.
fn main() {
    for p in std::env::args().skip(1) {
        let s = aim_xrk::Session::open(std::path::Path::new(&p)).unwrap();
        let Some(g) = &s.gps else { println!("{p}: no GPS"); continue };
        let (lat0, lon0) = (g.latitude[0], g.longitude[0]);
        let k = 111_320.0 * lat0.to_radians().cos();
        let xy: Vec<(f64, f64)> = (0..g.len()).map(|i| ((g.longitude[i] - lon0) * k, (g.latitude[i] - lat0) * 110_540.0)).collect();
        let (x0, x1) = xy.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
        let (y0, y1) = xy.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
        let end = xy.last().unwrap();
        let dist: f64 = xy.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
        println!("{}  spread {:.0} m x {:.0} m · path {:.0} m · ends {:.0} m from start · start {:.4},{:.4}", p.rsplit('/').next().unwrap(), x1 - x0, y1 - y0, dist, (end.0.powi(2) + end.1.powi(2)).sqrt(), lat0, lon0);
    }
}
