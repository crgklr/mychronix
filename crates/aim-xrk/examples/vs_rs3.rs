//! Compares a session as Mychronix reads it with RaceStudio's CSV export of
//! the same session ("Race Studio 2 .csv" from RS3 Analysis): lap times, and
//! every channel the two have in common, sample by sample.
//!
//! `cargo run -p aim-xrk --example vs_rs3 -- <file.xrk|.xrz> <export.csv>`

use std::collections::BTreeSet;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(file), Some(csv_path)) = (args.next(), args.next()) else {
        eprintln!("usage: vs_rs3 <file.xrk|.xrz> <export.csv>");
        std::process::exit(2);
    };
    let s = aim_xrk::Session::open(std::path::Path::new(&file)).expect("open session");
    let csv = std::fs::read_to_string(&csv_path).expect("read csv");
    let fields = |l: &str| -> Vec<String> { l.split("\",\"").map(|f| f.trim_matches('"').to_string()).collect() };
    let lines: Vec<&str> = csv.lines().collect();

    // Header block: "Segment Times" lists RaceStudio's laps.
    let meta = |key: &str| lines.iter().find(|l| l.starts_with(&format!("\"{key}\""))).map(|l| fields(l)[1..].to_vec()).unwrap_or_default();
    let rs3_laps: Vec<f64> = meta("Segment Times").iter().filter_map(|t| parse_time(t)).collect();

    let head_i = lines.iter().position(|l| l.starts_with("\"Time\"") && l.matches(',').count() > 2).expect("channel header");
    let head = fields(lines[head_i]);
    let units = fields(lines[head_i + 1]);
    let rows: Vec<Vec<f64>> = lines[head_i + 2..]
        .iter()
        .map(|l| fields(l).iter().map(|x| x.parse().unwrap_or(f64::NAN)).collect::<Vec<f64>>())
        .filter(|r: &Vec<f64>| r.len() == head.len() && r[0].is_finite())
        .collect();

    println!("{}  vs  {}", file.rsplit('/').next().unwrap(), csv_path.rsplit('/').next().unwrap());
    println!("RaceStudio: {} rows ({} channels) · Mychronix: {} channels\n", rows.len(), head.len() - 1, s.channels.len());

    // Laps, matched by when they end (one side may split a lap the other doesn't).
    let ours: Vec<f64> = s.laps.iter().map(|l| l.duration_ms() as f64 / 1000.0).collect();
    println!("Laps: RaceStudio {} · Mychronix {}", rs3_laps.len(), ours.len());
    let ends = |v: &[f64]| v.iter().scan(0.0, |acc, x| { *acc += x; Some(*acc) }).collect::<Vec<f64>>();
    let (rs3_end, our_end) = (ends(&rs3_laps), ends(&ours));
    let mut worst_lap = 0.0f64;
    let mut shown = BTreeSet::new();
    for (j, &e) in our_end.iter().enumerate() {
        // The RaceStudio lap ending nearest the same moment (their clock may start a little apart).
        let start_shift = rs3_end.first().zip(our_end.first()).map(|_| 0.0).unwrap_or(0.0);
        let (i, d) = rs3_end.iter().enumerate().map(|(i, &r)| (i, (r - e - start_shift).abs())).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((0, f64::MAX));
        if d < 1.5 {
            shown.insert(i);
            let diff = ours[j] - rs3_laps[i];
            let note = if j + 1 < ours.len() && j > 0 && diff.abs() < 1.0 { worst_lap = worst_lap.max(diff.abs()); "" } else if diff.abs() >= 1.0 { "  (split differently)" } else { "  (out/in lap)" };
            println!("  ours {:>3} {:>9.3}  RS3 {:>3} {:>9.3}  {:+.3}{note}", j + 1, ours[j], i + 1, rs3_laps[i], diff);
        } else {
            println!("  ours {:>3} {:>9.3}  no RaceStudio lap ends here", j + 1, ours[j]);
        }
    }
    for (i, l) in rs3_laps.iter().enumerate().filter(|(i, _)| !shown.contains(i)) {
        println!("  RS3 {:>3} {:>9.3}  (RaceStudio splits here; Mychronix doesn't)", i + 1, l);
    }
    println!("  largest difference on a racing lap: {worst_lap:.3} s\n");

    // Line the clocks up on a channel both have, by its best match over ±3 s.
    let key = ["GPS Speed", "RPM", "Engine RPM"].iter().find(|k| head.iter().any(|h| h == *k) && s.channel(k).is_some());
    let t0 = s.channels.iter().filter_map(|c| c.times_ms.first()).min().copied().unwrap_or(0) as f64;
    let mut offset = 0.0;
    if let Some(k) = key {
        let col = head.iter().position(|h| h == *k).unwrap();
        let ch = s.channel(k).unwrap();
        let mut best = (f64::MAX, 0.0);
        for off in (-3000..=3000).step_by(10) {
            let e: f64 = rows.iter().step_by(5).map(|r| (r[col] - at(ch, t0 + r[0] * 1000.0 + off as f64)).powi(2)).filter(|v| v.is_finite()).sum();
            if e < best.0 {
                best = (e, off as f64);
            }
        }
        offset = best.1;
        println!("Clocks lined up on {k}: ours = RaceStudio {:+.0} ms\n", offset);
    }

    // Every channel in common.
    println!("{:<26} {:>8} {:>11} {:>11} {:>11} {:>11}", "channel", "corr", "rms diff", "max diff", "RS3 mean", "our mean");
    let mut matched = BTreeSet::new();
    let mut results = Vec::new();
    for (k, name) in head.iter().enumerate().skip(1) {
        let Some(ch) = s.channel(name) else { continue };
        matched.insert(name.clone());
        // Channels sampled faster than the export are averaged over each row's slot,
        // as a 20 Hz export of 100 Hz data would be.
        let row_ms = rows.get(1).map(|r| (r[0] - rows[0][0]) * 1000.0).unwrap_or(50.0);
        let fast = ch.times_ms.len() as f64 > rows.len() as f64 * 1.5;
        let pairs: Vec<(f64, f64)> = rows
            .iter()
            .map(|r| {
                let t = t0 + r[0] * 1000.0 + offset;
                (r[k], if fast { mean_over(ch, t - row_ms / 2.0, t + row_ms / 2.0) } else { at(ch, t) })
            })
            .filter(|p| p.0.is_finite() && p.1.is_finite())
            .collect();
        if pairs.is_empty() {
            continue;
        }
        let n = pairs.len() as f64;
        let (ma, mb) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
        let cov: f64 = pairs.iter().map(|p| (p.0 - ma) * (p.1 - mb)).sum();
        let (va, vb): (f64, f64) = (pairs.iter().map(|p| (p.0 - ma).powi(2)).sum(), pairs.iter().map(|p| (p.1 - mb).powi(2)).sum());
        let corr = if va > 0.0 && vb > 0.0 { cov / (va * vb).sqrt() } else { f64::NAN };
        let rms = (pairs.iter().map(|p| (p.0 - p.1).powi(2)).sum::<f64>() / n).sqrt();
        let max = pairs.iter().map(|p| (p.0 - p.1).abs()).fold(0.0, f64::max);
        let unit = units.get(k).cloned().unwrap_or_default();
        results.push((name.clone(), corr, rms, max, ma, mb, unit, ch.units.clone()));
    }
    // Channels that disagree: is it a time shift? Find each one's best offset.
    let row_ms = rows.get(1).map(|r| (r[0] - rows[0][0]) * 1000.0).unwrap_or(50.0);
    let corr_at = |k: usize, ch: &aim_xrk::Channel, off: f64| {
        let fast = ch.times_ms.len() as f64 > rows.len() as f64 * 1.5;
        let pairs: Vec<(f64, f64)> = rows
            .iter()
            .step_by(2)
            .map(|r| {
                let t = t0 + r[0] * 1000.0 + off;
                (r[k], if fast { mean_over(ch, t - row_ms / 2.0, t + row_ms / 2.0) } else { at(ch, t) })
            })
            .filter(|p| p.0.is_finite() && p.1.is_finite())
            .collect();
        let n = pairs.len() as f64;
        let (ma, mb) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
        let cov: f64 = pairs.iter().map(|p| (p.0 - ma) * (p.1 - mb)).sum();
        let (va, vb): (f64, f64) = (pairs.iter().map(|p| (p.0 - ma).powi(2)).sum(), pairs.iter().map(|p| (p.1 - mb).powi(2)).sum());
        cov / (va * vb).sqrt()
    };
    let mut shifted = Vec::new();
    for (name, corr, ..) in &results {
        if corr.is_nan() || *corr > 0.98 {
            continue;
        }
        let k = head.iter().position(|h| h == name).unwrap();
        let ch = s.channel(name).unwrap();
        let best = (-3000..=3000).step_by(10).map(|o| (o, corr_at(k, ch, offset + o as f64))).filter(|p| p.1.is_finite()).max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((o, c)) = best {
            shifted.push(format!("{name}: {corr:.3} as lined up, {c:.3} if shifted {o:+} ms more"));
        }
    }
    for (name, corr, rms, max, ma, mb, unit, ours_unit) in &results {
        let flag = if unit.trim() != ours_unit.trim() && !unit.trim().is_empty() { format!("  units RS3 '{unit}' ours '{ours_unit}'") } else { String::new() };
        println!("{name:<26} {:>8} {rms:>11.4} {max:>11.4} {ma:>11.3} {mb:>11.3}{flag}", if corr.is_nan() { "const".to_string() } else { format!("{corr:.4}") });
    }
    if !shifted.is_empty() {
        println!("\nBest time shift for the channels that disagree:");
        for l in &shifted {
            println!("  {l}");
        }
    }
    let only_rs3: Vec<&String> = head.iter().skip(1).filter(|h| !matched.contains(*h)).collect();
    let only_ours: Vec<&str> = s.channels.iter().map(|c| c.name.as_str()).filter(|n| !head.iter().any(|h| h == n)).collect();
    println!("\nOnly in RaceStudio's export: {}", if only_rs3.is_empty() { "none".into() } else { only_rs3.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ") });
    println!("Only in Mychronix: {}", if only_ours.is_empty() { "none".into() } else { only_ours.join(", ") });
}

/// A channel's value at `t_ms`: interpolated for smooth channels, held for stepped ones.
fn at(c: &aim_xrk::Channel, t_ms: f64) -> f64 {
    if c.times_ms.is_empty() {
        return f64::NAN;
    }
    let i = c.times_ms.partition_point(|&x| (x as f64) < t_ms);
    if i == 0 {
        return c.values[0];
    }
    if i >= c.times_ms.len() {
        return *c.values.last().unwrap();
    }
    let (t1, t2) = (c.times_ms[i - 1] as f64, c.times_ms[i] as f64);
    if !c.interpolate || t2 <= t1 {
        return if (c.times_ms[i] as f64) == t_ms { c.values[i] } else { c.values[i - 1] };
    }
    let f = (t_ms - t1) / (t2 - t1);
    c.values[i - 1] + f * (c.values[i] - c.values[i - 1])
}

/// Mean of a channel's samples in [a, b) ms (the value there when none fall inside).
fn mean_over(c: &aim_xrk::Channel, a: f64, b: f64) -> f64 {
    let lo = c.times_ms.partition_point(|&x| (x as f64) < a);
    let hi = c.times_ms.partition_point(|&x| (x as f64) < b);
    if hi <= lo {
        return at(c, (a + b) / 2.0);
    }
    c.values[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
}

/// "1:22.248" or "0:35.000" → seconds.
fn parse_time(t: &str) -> Option<f64> {
    let (m, s) = t.split_once(':')?;
    Some(m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?)
}
