//! Compares each .mc4 with the .drk that has the same start time: laps and every sample.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let drks: Vec<aim_xrk::Session> = std::fs::read_dir(&a[2]).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "drk")).filter_map(|p| aim_xrk::Session::open(&p).ok()).collect();
    let (mut ok, mut bad) = (0, 0);
    for e in std::fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok()) {
        let m = aim_xrk::Session::open(e.path()).unwrap();
        let Some(d) = drks.iter().find(|d| d.info.time == m.info.time && d.info.date == m.info.date) else { println!("{:?} no drk", e.path()); bad += 1; continue };
        let laps = m.laps.iter().map(|l| l.duration_ms()).collect::<Vec<_>>() == d.laps.iter().map(|l| l.duration_ms()).collect::<Vec<_>>();
        let mut same = laps;
        for ch in ["Engine", "Datalogger_Temp", "Battery"] {
            let (x, y) = (&m.channel(ch).unwrap().values, &d.channel(ch).unwrap().values);
            let n = x.len().min(y.len());
            let diff = (0..n).filter(|&i| (x[i] - y[i]).abs() > 1e-9).count();
            if diff > 0 || x.len().abs_diff(y.len()) > 12 { same = false; println!("{:?} {ch}: mc4 {} drk {} differing {diff}", e.path().file_name().unwrap(), x.len(), y.len()); }
        }
        if same { ok += 1 } else { bad += 1 }
    }
    println!("match {ok}, differ {bad}");
}
