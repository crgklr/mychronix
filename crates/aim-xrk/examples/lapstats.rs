//! Min / max / mean of every channel within one lap. `lapstats <file> <lap>`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let s = aim_xrk::Session::open(&a[1]).unwrap();
    let n: i32 = a[2].parse().unwrap();
    let lap = s.laps.iter().find(|l| l.number == n).unwrap();
    for c in &s.channels {
        let v: Vec<f64> = c.times_ms.iter().zip(&c.values).filter(|(t, _)| **t >= lap.start_ms && **t <= lap.end_ms).map(|(_, v)| *v).collect();
        if v.is_empty() { println!("{:16} (no samples)", c.name); continue; }
        let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |a, &x| (a.0.min(x), a.1.max(x)));
        println!("{:16} {} samples  min {lo:.2}  max {hi:.2}  mean {:.2} {}", c.name, v.len(), v.iter().sum::<f64>() / v.len() as f64, c.units);
    }
}
