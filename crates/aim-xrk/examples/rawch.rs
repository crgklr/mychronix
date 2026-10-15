fn main() { let a: Vec<String> = std::env::args().collect(); let s = aim_xrk::Session::open(&a[1]).unwrap();
  let c = s.channel(&a[2]).unwrap(); println!("n={} first={:?} min={:?} max={:?}", c.values.len(), &c.values[..c.values.len().min(10)], c.values.iter().cloned().reduce(f64::min), c.values.iter().cloned().reduce(f64::max)); }
