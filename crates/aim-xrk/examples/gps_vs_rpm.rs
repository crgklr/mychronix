//! How far GPS speed lags or leads RPM in a session (with the clutch locked the
//! two move together, so the best-matching shift shows where GPS sits in time
//! against the engine channels).
//! `cargo run -p aim-xrk --example gps_vs_rpm -- <file.xrk|.xrz>`
fn main() {
    let path = std::env::args().nth(1).expect("file");
    let s = aim_xrk::Session::open(std::path::Path::new(&path)).expect("open");
    let (speed, rpm) = (s.channel("GPS Speed").expect("GPS Speed"), s.channel("RPM").expect("RPM"));
    let at = |c: &aim_xrk::Channel, t: f64| {
        let i = c.times_ms.partition_point(|&x| (x as f64) < t);
        if i == 0 || i >= c.times_ms.len() {
            return f64::NAN;
        }
        let (t1, t2) = (c.times_ms[i - 1] as f64, c.times_ms[i] as f64);
        c.values[i - 1] + (c.values[i] - c.values[i - 1]) * (t - t1) / (t2 - t1).max(1e-9)
    };
    // Samples while moving well above clutch engagement.
    let times: Vec<f64> = speed.times_ms.iter().zip(&speed.values).filter(|(_, v)| **v > 25.0).map(|(t, _)| *t as f64).collect();
    let corr = |lag: f64| {
        let pairs: Vec<(f64, f64)> = times.iter().map(|&t| (at(speed, t), at(rpm, t + lag))).filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
        let n = pairs.len() as f64;
        let (ma, mb) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
        let cov: f64 = pairs.iter().map(|p| (p.0 - ma) * (p.1 - mb)).sum();
        let (va, vb): (f64, f64) = (pairs.iter().map(|p| (p.0 - ma).powi(2)).sum(), pairs.iter().map(|p| (p.1 - mb).powi(2)).sum());
        cov / (va * vb).sqrt()
    };
    let (best, c) = (-400..=400).step_by(5).map(|l| (l, corr(l as f64))).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    println!("{} samples while moving · best match when RPM is read {best:+} ms from GPS (corr {c:.4}; at 0 ms {:.4})", times.len(), corr(0.0));
    println!("  {}", if best > 0 { "GPS is stamped EARLIER than the engine saw the same speed" } else if best < 0 { "GPS is stamped LATER than the engine saw the same speed" } else { "in step" });
}
