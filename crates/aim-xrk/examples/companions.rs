//! Checks RaceStudio 2 companion files: every .imd in a folder against its
//! .drk, and a GPS session's .gpk and .rrk positions against each other and
//! the session's GPS speed.
use aim_xrk::{Session, rs2gps};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mut same, mut differ) = (0, 0);
    for p in std::fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "imd")) {
        let imd = aim_xrk::mc4::read_imd(&std::fs::read(&p).unwrap());
        let drk = aim_xrk::drk::read(&std::fs::read(p.with_extension("drk")).unwrap());
        match (imd, drk) {
            (Ok(i), Ok(d)) => {
                let ok = i.laps.len() == d.laps.len()
                    && ["Engine", "Datalogger_Temp", "Battery"].iter().all(|c| {
                        let (x, y) = (&i.channel(c).unwrap().values, &d.channel(c).unwrap().values);
                        (0..x.len().min(y.len())).all(|k| (x[k] - y[k]).abs() < 1e-9) && x.len().abs_diff(y.len()) <= 12
                    });
                if ok {
                    same += 1
                } else {
                    differ += 1;
                    println!("differs: {}", p.display())
                }
            }
            (i, d) => {
                differ += 1;
                println!("{}: imd {:?} drk {:?}", p.display(), i.err(), d.err())
            }
        }
    }
    println!(".imd vs .drk: {same} identical, {differ} differ");
    if let Some(g) = a.get(2) {
        let base = std::path::Path::new(g);
        let s = Session::open(base).unwrap();
        let gps = s.gps.as_ref().expect("no GPS");
        println!("{}: {} positions, first {:.6},{:.6} at {} ms", base.display(), gps.len(), gps.latitude[0], gps.longitude[0], gps.times_ms[0]);
        let (gk, rr) = (rs2gps::read_gpk(&std::fs::read(base.with_extension("gpk")).unwrap()), rs2gps::read_rrk(&std::fs::read(base.with_extension("rrk")).unwrap()));
        // Same tick in both: how far apart (m)?
        let mut d: Vec<f64> = rr
            .iter()
            .filter_map(|r| gk.iter().find(|g| g.ticks == r.ticks).map(|g| ((g.lat - r.lat).to_radians() * 6.371e6).hypot((g.lon - r.lon).to_radians() * 6.371e6 * r.lat.to_radians().cos())))
            .collect();
        d.sort_by(|a, b| a.total_cmp(b));
        println!("gpk {} fixes, rrk {}; same-tick pairs {}: median gap {:.2} m, 95th {:.2} m", gk.len(), rr.len(), d.len(), d[d.len() / 2], d[d.len() * 95 / 100]);
        let ms = s.laps.iter().map(|l| l.duration_ms()).sum::<i64>() as f64;
        let _ = ms;
        if let Some(sp) = s.channel("GPS Speed") {
            for (name, f) in [("gpk", &gk), ("rrk", &rr)] {
                println!("{name} best offset vs GPS Speed: {:?}", rs2gps::best_offset(f, 1.0, sp));
            }
        }
        let max_v = gps.speed_kmh.iter().cloned().fold(0.0, f64::max);
        println!("track speed max {max_v:.1} km/h, laps {}", s.laps.len());
    }
}
