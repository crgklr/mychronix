//! Compares the same session as .drk and .xrk: for each channel both have,
//! the medians and how well the .drk's values track the .xrk's over time.
use aim_xrk::Session;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (d, x) = (Session::open(&a[1]).unwrap(), Session::open(&a[2]).unwrap());
    let med = |v: &[f64]| {
        let mut v = v.to_vec();
        v.sort_by(|a, b| a.total_cmp(b));
        v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
    };
    let norm = |n: &str| n.to_lowercase().replace(['_', ' '], "");
    for dc in &d.channels {
        let Some(xc) = x.channels.iter().find(|c| norm(&c.name) == norm(&dc.name)) else {
            continue;
        };
        if dc.values.len() < 20 || xc.values.len() < 20 {
            continue;
        }
        // Sample the .xrk at the .drk's times (drk time 0 = xrk's first sample).
        let x0 = *xc.times_ms.first().unwrap();
        let pairs: Vec<(f64, f64)> = dc
            .times_ms
            .iter()
            .zip(&dc.values)
            .step_by((dc.values.len() / 3000).max(1))
            .filter_map(|(&t, &v)| {
                let i = xc.times_ms.partition_point(|&u| u < t + x0);
                (i < xc.values.len()).then(|| (v, xc.values[i]))
            })
            .collect();
        let n = pairs.len() as f64;
        let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
        let (sxy, sxx, syy) = pairs.iter().fold((0.0, 0.0, 0.0), |a, p| (a.0 + (p.0 - mx) * (p.1 - my), a.1 + (p.0 - mx).powi(2), a.2 + (p.1 - my).powi(2)));
        let r = sxy / (sxx * syy).sqrt();
        // drk = slope * xrk + intercept, over the overlap
        let slope = sxy / syy;
        let icpt = mx - slope * my;
        let _ = med;
        println!("{:<18} {:<6} vs {:<6} slope {:>9.4} intercept {:>9.3}  r {:.3}  n {}", dc.name, dc.units, xc.units, slope, icpt, r, pairs.len());
    }
    println!("drk {} channels, xrk {}, laps {} vs {}", d.channels.len(), x.channels.len(), d.laps.len(), x.laps.len());
}
