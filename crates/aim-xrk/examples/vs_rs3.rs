//! Compares our GPS LonAcc / LatAcc with a RaceStudio CSV export of the same
//! session. `cargo run -p aim-xrk --example vs_rs3 -- <file.xrz> <export.csv>`
fn main() {
    let mut args = std::env::args().skip(1);
    let s = aim_xrk::Session::open(std::path::Path::new(&args.next().unwrap())).expect("open");
    let csv = std::fs::read_to_string(args.next().unwrap()).unwrap();
    let mut lines = csv.lines().skip_while(|l| !(l.starts_with("\"Time\"") && l.contains("GPS Speed")));
    let head: Vec<String> = lines.next().unwrap().split(',').map(|h| h.trim_matches('"').to_string()).collect();
    let col = |name: &str| head.iter().position(|h| h == name).unwrap();
    let (ct, cs, clon, clat) = (col("Time"), col("GPS Speed"), col("GPS LonAcc"), col("GPS LatAcc"));
    let rows: Vec<Vec<f64>> = lines
        .filter_map(|l| {
            let v: Vec<f64> = l.split(',').map(|x| x.trim_matches('"').parse().unwrap_or(f64::NAN)).collect();
            (v.len() == head.len() && v[ct].is_finite()).then_some(v)
        })
        .collect();
    let gps = s.gps.as_ref().expect("gps");
    let t0 = gps.times_ms[0];
    let at = |c: &aim_xrk::Channel, t: f64| {
        let i = c.times_ms.partition_point(|&x| (x as f64) < t).min(c.times_ms.len() - 1);
        c.values[i]
    };
    let ours_s = s.channel("GPS Speed").unwrap();
    // Best time offset between their clock and ours, by matching speed.
    let mut best = (f64::MAX, 0i64);
    for off in (-2000..=2000).step_by(10) {
        let e: f64 = rows.iter().map(|r| (r[cs] - at(ours_s, t0 as f64 + r[ct] * 1000.0 + off as f64)).powi(2)).sum();
        if e < best.0 { best = (e, off); }
    }
    let off = best.1 as f64;
    println!("rows {}  time offset {} ms  speed rms {:.2} km/h", rows.len(), best.1, (best.0 / rows.len() as f64).sqrt());
    let (lon, lat) = (s.channel("GPS LonAcc").unwrap(), s.channel("GPS LatAcc").unwrap());
    for (name, c, k) in [("LonAcc", lon, clon), ("LatAcc", lat, clat)] {
        let pairs: Vec<(f64, f64)> = rows.iter().map(|r| (r[k], at(c, t0 as f64 + r[ct] * 1000.0 + off))).filter(|p| p.0.is_finite()).collect();
        let n = pairs.len() as f64;
        let (ma, mb) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
        let cov: f64 = pairs.iter().map(|p| (p.0 - ma) * (p.1 - mb)).sum();
        let (va, vb): (f64, f64) = (pairs.iter().map(|p| (p.0 - ma).powi(2)).sum(), pairs.iter().map(|p| (p.1 - mb).powi(2)).sum());
        let rms = (pairs.iter().map(|p| (p.0 - p.1).powi(2)).sum::<f64>() / n).sqrt();
        let (pa, pb) = (pairs.iter().map(|p| p.0.abs()).fold(0.0, f64::max), pairs.iter().map(|p| p.1.abs()).fold(0.0, f64::max));
        let sign = if cov < 0.0 { "  (opposite sign!)" } else { "" };
        println!("{name}: correlation {:.3}  rms diff {:.3} g  peak RS3 {:.2} g / ours {:.2} g{sign}", cov / (va * vb).sqrt(), rms, pa, pb);
        // Same thresholds, both ways.
        let (th, cond): (f64, fn(f64, f64) -> bool) = if name == "LonAcc" { (-0.15, |v, t| v < t) } else { (0.2, |v, t| v.abs() > t) };
        let share = |f: &dyn Fn(&(f64, f64)) -> f64| pairs.iter().filter(|p| cond(f(p), th)).count() as f64 * 100.0 / n;
        println!("   % beyond {th} g: RS3 {:.1}%  ours {:.1}%", share(&|p| p.0), share(&|p| p.1));
    }

    // Smoothing sweep: half-window (ms) for each, against RaceStudio.
    let corr = |vals: &[f64], k: usize| {
        let ch = aim_xrk::Channel { name: String::new(), group: None, units: String::new(), decimals: 2, interpolate: true, times_ms: gps.times_ms.clone(), values: vals.to_vec() };
        let pairs: Vec<(f64, f64)> = rows.iter().map(|r| (r[k], at(&ch, t0 as f64 + r[ct] * 1000.0 + off))).collect();
        let n = pairs.len() as f64;
        let rms = (pairs.iter().map(|p| (p.0 - p.1).powi(2)).sum::<f64>() / n).sqrt();
        rms
    };
    for half in [40i64, 80, 120, 160, 200, 250, 300, 400, 500] {
        let lon: Vec<f64> = (0..gps.times_ms.len()).map(|i| {
            let t = &gps.times_ms; let v = &gps.speed_kmh;
            let a = t[..i].partition_point(|&x| x < t[i] - half);
            let b = (i + t[i..].partition_point(|&x| x <= t[i] + half)).saturating_sub(1).max(i);
            let dt = (t[b] - t[a]) as f64 / 1000.0;
            if dt <= 0.0 { 0.0 } else { (v[b] - v[a]) / 3.6 / dt / 9.81 }
        }).collect();
        let lat: Vec<f64> = (0..gps.times_ms.len()).map(|i| {
            let t = &gps.times_ms;
            let a = t[..i].partition_point(|&x| x < t[i] - half);
            let b = (i + t[i..].partition_point(|&x| x <= t[i] + half)).max(i + 1).min(t.len());
            let r = &gps.yaw_rate_dps[a..b];
            gps.speed_kmh[i] / 3.6 * (r.iter().sum::<f64>() / r.len() as f64).to_radians() / 9.81
        }).collect();
        println!("±{half:>3} ms: lon rms {:.4}  lat rms {:.4}", corr(&lon, clon), corr(&lat, clat));
    }
}
