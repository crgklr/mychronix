fn main() {
    for p in std::env::args().skip(1) {
        let s = aim_xrk::Session::open(&p).unwrap();
        let e = s.channel("Engine").unwrap();
        let hi: Vec<(i64, f64)> = e.times_ms.iter().zip(&e.values).filter(|(_, v)| **v > 13000.0).map(|(t, v)| (*t, *v)).take(4).collect();
        let t = s.channel("Temperature").map(|c| (c.values.len(), c.values.iter().take(3).cloned().collect::<Vec<_>>())).unwrap_or((0, vec![]));
        println!("{} engine n={} >13k: {:?} | temp {:?}", p.rsplit('/').next().unwrap(), e.values.len(), hi, t);
    }
}
