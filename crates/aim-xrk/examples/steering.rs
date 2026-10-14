//! Steering reversals per lap at several thresholds, to calibrate a
//! "corrections" count. `cargo run -p aim-xrk --example steering -- <file>`
fn main() {
    let s = aim_xrk::Session::open(std::path::Path::new(&std::env::args().nth(1).unwrap())).unwrap();
    let st = s.channels.iter().find(|c| c.name.to_lowercase().contains("steer")).expect("steering");
    println!("{} · {} samples", st.name, st.values.len());
    for lap in s.complete_laps() {
        let lo = st.times_ms.partition_point(|&t| t < lap.start_ms);
        let hi = st.times_ms.partition_point(|&t| t <= lap.end_ms);
        let v = &st.values[lo..hi];
        let peak = v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        let mean_abs = v.iter().map(|x| x.abs()).sum::<f64>() / v.len().max(1) as f64;
        let mut sorted = v.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
        let counts: Vec<String> = [2.0, 3.0, 5.0, 8.0, 12.0].iter().map(|&th| format!("{th}°:{}", reversals(v, th))).collect();
        println!("lap {:>2} {:>7.3}s  median {:>6.1}°  peak {:>5.1}°  mean|x| {:>5.1}°  reversals {}", lap.number, lap.duration_ms() as f64 / 1000.0, median, peak, mean_abs, counts.join(" "));
    }
}

/// Direction changes of at least `th` degrees (hysteresis): how often the wheel
/// changes direction by a meaningful amount.
fn reversals(v: &[f64], th: f64) -> usize {
    let (mut n, mut ext, mut dir) = (0, v.first().copied().unwrap_or(0.0), 0i8);
    for &x in v {
        match dir {
            1 if x > ext => ext = x,
            -1 if x < ext => ext = x,
            1 if x < ext - th => { n += 1; dir = -1; ext = x; }
            -1 if x > ext + th => { n += 1; dir = 1; ext = x; }
            0 if (x - ext).abs() >= th => { dir = if x > ext { 1 } else { -1 }; ext = x; }
            _ => {}
        }
    }
    n
}
