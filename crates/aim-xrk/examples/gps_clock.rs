//! GPS timing evidence: for each GPS record, the logger's timestamp next to the
//! receiver's own time of week (NAV-SOL iTOW/fTOW), and what the GNFI block holds.
//! `cargo run -p aim-xrk --example gps_clock -- <file.xrk|.xrz>`
fn main() {
    let path = std::env::args().nth(1).expect("file");
    let data = std::fs::read(&path).unwrap();
    let file = aim_xrk::libxrk::read_xrk(&data).expect("read");
    let raw = &file.raw;
    println!("time_offset {} ms · gps bytes {} ({} records) · gnfi bytes {} ({} x32)", raw.time_offset, raw.gps_data.len(), raw.gps_data.len() / 56, raw.gnfi_data.len(), raw.gnfi_data.len() / 32);
    let le32 = |b: &[u8], o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    // GPS: logger tc, iTOW (ms), fTOW (ns).
    let recs: Vec<(i64, f64)> = raw.gps_data.chunks_exact(56).map(|r| (le32(r, 0) as i64, le32(r, 4) as u32 as f64 + le32(r, 8) as f64 / 1e6)).collect();
    for r in recs.iter().take(5) {
        println!("  gps  logger {:>10}  itow {:>14.3}  diff {:>14.3}", r.0, r.1, r.0 as f64 - r.1);
    }
    // Spread of (logger - itow) over the session after removing a straight-line fit.
    if recs.len() > 10 {
        let n = recs.len() as f64;
        let xs: Vec<f64> = recs.iter().map(|r| r.1).collect();
        let ds: Vec<f64> = recs.iter().map(|r| r.0 as f64 - r.1).collect();
        let (mx, md) = (xs.iter().sum::<f64>() / n, ds.iter().sum::<f64>() / n);
        let slope = xs.iter().zip(&ds).map(|(x, d)| (x - mx) * (d - md)).sum::<f64>() / xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>();
        let res: Vec<f64> = xs.iter().zip(&ds).map(|(x, d)| d - md - slope * (x - mx)).collect();
        let sd = (res.iter().map(|r| r * r).sum::<f64>() / n).sqrt();
        let dt: Vec<f64> = recs.windows(2).map(|w| w[1].1 - w[0].1).collect();
        println!("  gps rate: median iTOW step {:.1} ms · logger-minus-iTOW drift {:.1} ppm · jitter sd {:.2} ms", median(dt), slope * 1e6, sd);
        // Arrival delay above the earliest arrivals (drift removed, wild records dropped).
        let mut r: Vec<f64> = res.iter().copied().filter(|v| v.abs() < 1000.0).collect();
        r.sort_by(|a, b| a.total_cmp(b));
        let pct = |p: f64| r[((r.len() - 1) as f64 * p) as usize];
        let base = pct(0.01);
        println!("  delay above the earliest 1%: p10 {:.1}  median {:.1}  mean {:.1}  p90 {:.1}  max {:.1} ms  ({} of {} records kept)",
            pct(0.10) - base, pct(0.5) - base, r.iter().sum::<f64>() / r.len() as f64 - base, pct(0.9) - base, r[r.len() - 1] - base, r.len(), res.len());
    }
    // MYCHRONIX_GPS_DUMP=<file>: per fix, the timestamp libxrk uses, the receiver's time and speed.
    if let (Ok(out), Some(g)) = (std::env::var("MYCHRONIX_GPS_DUMP"), file.gps.as_ref()) {
        let mut w = String::from("logger_ms,itow_ms,kmh\n");
        for (i, r) in recs.iter().enumerate().take(g.timecodes.len()) {
            w.push_str(&format!("{},{:.3},{:.4}\n", g.timecodes[i], r.1, g.speed[i] * 3.6));
        }
        std::fs::write(out, w).unwrap();
    }
    // GNFI: 32-byte records; print the first few as 32-bit words.
    for r in raw.gnfi_data.chunks_exact(32).take(6) {
        let w: Vec<String> = (0..8).map(|k| format!("{:>11}", le32(r, k * 4))).collect();
        println!("  gnfi {}", w.join(" "));
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}
