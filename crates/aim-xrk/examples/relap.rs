//! Times a session's laps from GPS against the start/finish lines in some
//! `.xri` summaries. `relap <session> <summary.xri>...`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let s = aim_xrk::Session::open(&a[1]).unwrap();
    let lines: Vec<(f64, f64)> = a[2..].iter().filter_map(|p| aim_xrk::parse_xri(&std::fs::read(p).ok()?).ok()?.start_finish).collect();
    println!("logger laps: {:?}", s.laps.iter().map(|l| l.duration_ms()).collect::<Vec<_>>());
    println!("needs retiming: {}", aim_xrk::relap::needs_retiming(&s));
    for (k, sf) in lines.iter().enumerate() {
        println!("  line {k} {sf:?}: {} crossings", aim_xrk::relap::crossings(s.gps.as_ref().unwrap(), *sf).len());
    }
    match aim_xrk::relap::fix(&s, &lines) {
        Some((k, laps)) => println!("line {k}: {} laps: {:?}", laps.len(), laps.iter().map(|l| format!("{:.3}", l.duration_ms() as f64 / 1000.0)).collect::<Vec<_>>()),
        None => println!("left as the logger timed it"),
    }
}
