//! Measures candidate session-note signals to calibrate thresholds.
fn slope(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let syy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    (sxy / sxx, if sxx > 0.0 && syy > 0.0 { sxy / (sxx * syy).sqrt() } else { 0.0 })
}
fn main() {
    for p in std::env::args().skip(1) {
        let Ok(s) = aim_xrk::Session::open(std::path::Path::new(&p)) else { continue };
        let laps = s.complete_laps();
        let Some(best) = s.best_lap(10_000) else { continue };
        let _ = best;
        let mut ds: Vec<i64> = laps.iter().map(|l| l.duration_ms()).filter(|&d| d >= 10_000).collect();
        ds.sort_unstable();
        let median = ds.get(ds.len() / 2).copied().unwrap_or(0) as f64;
        let clean: Vec<&aim_xrk::Lap> = laps.iter().filter(|l| l.duration_ms() >= 10_000 && l.duration_ms() as f64 <= median * 1.15).collect();
        if clean.len() < 4 { continue; }
        let t: Vec<f64> = clean.iter().map(|l| l.duration_ms() as f64 / 1000.0).collect();
        let x: Vec<f64> = (0..t.len()).map(|i| i as f64).collect();
        let (sl, r) = slope(&x, &t);
        let mean = t.iter().sum::<f64>() / t.len() as f64;
        let sd = (t.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / t.len() as f64).sqrt();
        let mut line = format!("{:<22} clean {:>2}/{:<2} lap slope {:+.3}s/lap r {:+.2} sd {:.3}s", p.rsplit('/').next().unwrap().chars().take(22).collect::<String>(), clean.len(), laps.len(), sl, r, sd);
        for name in ["Exhaust Temp", "Water Temp", "Temp TC 1"] {
            if let Some(c) = s.channel(name).filter(|c| !c.values.is_empty()) {
                let mx: Vec<f64> = clean.iter().filter_map(|l| {
                    let lo = c.times_ms.partition_point(|&q| q < l.start_ms); let hi = c.times_ms.partition_point(|&q| q <= l.end_ms);
                    c.values[lo..hi].iter().copied().reduce(f64::max)
                }).collect();
                if mx.len() == clean.len() {
                    let (es, er) = slope(&x, &mx);
                    line += &format!(" | {name} max {:.0}..{:.0}C slope {:+.1}C/lap r {:+.2}", mx.iter().cloned().fold(f64::MAX, f64::min), mx.iter().cloned().fold(f64::MIN, f64::max), es, er);
                }
            }
        }
        // RPM drops: >1500 rpm within 200 ms while accelerating (GPS LonAcc>0.05) above 20 km/h.
        if let (Some(rpm), Some(lon), Some(sp)) = (s.channel("RPM"), s.channel("GPS LonAcc"), s.channel("GPS Speed")) {
            let at = |c: &aim_xrk::Channel, tt: i64| { let i = c.times_ms.partition_point(|&q| q <= tt); c.values.get(i.saturating_sub(1)).copied().unwrap_or(0.0) };
            let mut drops = 0; let mut last = i64::MIN;
            for i in 0..rpm.values.len() {
                let t0 = rpm.times_ms[i];
                if t0 - last < 1000 { continue; }
                let j = rpm.times_ms.partition_point(|&q| q <= t0 + 200);
                if j <= i + 1 { continue; }
                let min = rpm.values[i..j].iter().copied().fold(f64::MAX, f64::min);
                if rpm.values[i] - min > 1500.0 && min > 1000.0 && at(lon, t0) > 0.05 && at(sp, t0) > 20.0 && clean.iter().any(|l| t0 >= l.start_ms && t0 <= l.end_ms) {
                    drops += 1; last = t0;
                }
            }
            line += &format!(" | rpm drops {drops}");
        }
        println!("{line}");
    }
}
