//! A made-up Mychronix library for tests and screenshots: a track, two
//! drivers and their karts, and MyChron 4 sessions with GPS, written in the
//! formats Mychronix reads (`.drk` + `.gpk` as RaceStudio 2 keeps them, a
//! custom `.tkk`, the Garage's JSON). Nothing in it is anyone's real data,
//! and the same files come out every time.

use std::f64::consts::TAU;
use std::path::Path;

/// Where the made-up track is (a field, not a real track).
const LAT0: f64 = 44.5208;
const LON0: f64 = -89.5731;
const TRACK: &str = "Pinecrest Kart Park";
/// Meters between the points the track and the laps are worked out on.
const STEP: f64 = 1.0;

/// The track's center line: a closed loop of about 820 m with six corners
/// (20 to 44 mph), east/north meters from (`LAT0`, `LON0`), one
/// point a meter, starting on the start/finish straight.
pub fn center_line() -> Vec<(f64, f64)> {
    let shape = |a: f64| {
        let r = 88.0
            * (1.0
                + 0.32 * (2.0 * a + 0.6).sin()
                + 0.24 * (3.0 * a).sin()
                + 0.14 * (5.0 * a + 1.0).sin()
                + 0.06 * (7.0 * a + 0.3).sin());
        (r * a.cos() * 1.3, r * a.sin() * 0.85)
    };
    let fine: Vec<(f64, f64)> = (0..20_000)
        .map(|k| shape(k as f64 / 20_000.0 * TAU))
        .collect();
    // The same loop, a point every meter along it.
    let mut out = vec![fine[0]];
    let mut since = 0.0;
    for w in fine
        .windows(2)
        .chain(std::iter::once(&[fine[fine.len() - 1], fine[0]][..]))
    {
        since += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        if since >= STEP {
            out.push(w[1]);
            since -= STEP;
        }
    }
    out
}

/// The line's curvature at each point (1/m, positive turning left), from
/// the change of heading over 8 m either side.
fn curvature(line: &[(f64, f64)]) -> Vec<f64> {
    let n = line.len();
    let at = |i: isize| line[i.rem_euclid(n as isize) as usize];
    (0..n as isize)
        .map(|i| {
            let (a, b, c) = (at(i - 8), at(i), at(i + 8));
            let h1 = (b.1 - a.1).atan2(b.0 - a.0);
            let h2 = (c.1 - b.1).atan2(c.0 - b.0);
            let pi = std::f64::consts::PI;
            (((h2 - h1 + 3.0 * pi).rem_euclid(TAU)) - pi) / 8.0
        })
        .collect()
}

/// How fast a kart takes each point (m/s): what the corner's grip allows,
/// with braking before corners and the engine's pull out of them, which
/// fades toward the kart's top speed.
fn speeds(line: &[(f64, f64)], top: f64, grip: f64) -> Vec<f64> {
    let n = line.len();
    let mut v: Vec<f64> = curvature(line)
        .iter()
        .map(|k| (grip / k.abs().max(1e-6)).sqrt().min(top))
        .collect();
    // Twice round, so the limits carry across the start/finish line.
    for _ in 0..2 {
        for i in 1..=n {
            let (p, q) = ((i - 1) % n, i % n);
            let pull = 3.4 * (1.0 - (v[p] / top).powi(2)).max(0.02);
            v[q] = v[q].min((v[p] * v[p] + 2.0 * pull * STEP).sqrt());
        }
        for i in (0..n).rev() {
            let (p, q) = ((i + 1) % n, i);
            v[q] = v[q].min((v[p] * v[p] + 2.0 * 6.5 * STEP).sqrt()); // braking
        }
    }
    v
}

/// Repeatable "random" numbers in 0..1.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Where the kart was and what it was doing, every 50 ms.
struct Sample {
    /// Meters round the lap.
    at: f64,
    kmh: f64,
    rpm: f64,
    /// Speeding up (+) and braking (-), and cornering, in m/s².
    forward: f64,
    sideways: f64,
}

/// One session: its laps' times (ms) and its samples.
struct Run {
    laps_ms: Vec<u32>,
    samples: Vec<Sample>,
}

/// A session of `laps` timed laps between an out-lap and an in-lap; `pace`
/// scales the kart's speed (1.0: the quicker driver on a good day).
fn drive(line: &[(f64, f64)], laps: usize, pace: f64, rpm_per_kmh: f64, dice: &mut Dice) -> Run {
    let mut laps_ms = Vec::new();
    let mut samples = Vec::new();
    let n = line.len();
    let mut t_ms = 0.0;
    let mut next_sample = 0.0;
    for lap in 0..laps + 2 {
        let f = if lap == 0 || lap == laps + 1 {
            0.8
        } else {
            pace * (0.975 + 0.025 * dice.next())
        };
        let v = speeds(line, 21.5 * f, 11.0 * f * f);
        let k = curvature(line);
        let start = t_ms;
        for i in 0..n {
            let (a, b) = (v[i], v[(i + 1) % n]);
            let dt = STEP / ((a + b) / 2.0) * 1000.0;
            while next_sample < t_ms + dt {
                let frac = (next_sample - t_ms) / dt;
                let ms = a + (b - a) * frac;
                let kmh = ms * 3.6;
                samples.push(Sample {
                    at: i as f64 + frac,
                    kmh,
                    rpm: (kmh * rpm_per_kmh).max(3600.0) + 40.0 * (dice.next() - 0.5),
                    forward: (b * b - a * a) / (2.0 * STEP) + 0.15 * (dice.next() - 0.5),
                    sideways: ms * ms * k[i] + 0.2 * (dice.next() - 0.5),
                });
                next_sample += 50.0;
            }
            t_ms += dt;
        }
        laps_ms.push((t_ms - start).round() as u32);
    }
    Run { laps_ms, samples }
}

fn east_north_to_lat_lon((e, n): (f64, f64)) -> (f64, f64) {
    (
        LAT0 + n / 110_540.0,
        LON0 + e / (111_320.0 * LAT0.to_radians().cos()),
    )
}

/// A point on the WGS-84 ellipsoid in earth-centered meters.
fn earth_centered(lat: f64, lon: f64) -> (f64, f64, f64) {
    let (a, e2) = (6_378_137.0, 6.694_379_990_14e-3);
    let (sp, cp) = lat.to_radians().sin_cos();
    let (sl, cl) = lon.to_radians().sin_cos();
    let n = a / (1.0 - e2 * sp * sp).sqrt();
    (n * cp * cl, n * cp * sl, n * (1.0 - e2) * sp)
}

/// The session as RaceStudio 2 saves it: the `.drk` and its `.gpk`.
fn drk(
    run: &Run,
    line: &[(f64, f64)],
    serial: u32,
    driver: &str,
    when: &str,
    dice: &mut Dice,
) -> (Vec<u8>, Vec<u8>) {
    let hdr = |tag: &[u8; 2]| {
        let mut h = vec![0u8; 0x20];
        h[..2].copy_from_slice(tag);
        h[2] = 0x58;
        h[3] = 0x02;
        h
    };
    let mut f = hdr(b"RD");
    f.extend(hdr(b"RD"));
    let mut rd = vec![0u8; 0x3e0];
    rd[0x0c..0x0c + when.len()].copy_from_slice(when.as_bytes());
    f.extend(rd);
    f.extend(hdr(b"PP"));
    let mut pp = vec![0u8; 0x400];
    pp[0x28..0x28 + TRACK.len()].copy_from_slice(TRACK.as_bytes());
    pp[0x50..0x50 + driver.len()].copy_from_slice(driver.as_bytes());
    f.extend(pp);
    f.extend(hdr(b"II"));
    let mut ii = vec![0u8; 0x80];
    ii[0x04..0x06].copy_from_slice(&106u16.to_le_bytes()); // a MyChron 4
    ii[0x10..0x14].copy_from_slice(&serial.to_le_bytes());
    f.extend(ii);

    // Channels: name, AiM sensor code, rate code, raw samples (a 3-sample trailer after).
    let n = run.samples.len();
    let mut egt = 480.0;
    let egt_raw: Vec<u16> = run
        .samples
        .iter()
        .map(|&Sample { kmh, .. }| {
            egt += (380.0 + 2.0 * kmh - egt) * 0.01; // follows the pace, slowly
            ((egt + 273.15) / 0.1220703125) as u16
        })
        .collect();
    let batt: Vec<u16> = (0..n.div_ceil(20))
        .map(|_| ((12.55 + 0.05 * dice.next()) / 0.0012207030085846782) as u16)
        .collect();
    // GPS accelerations as m/s² (sensor codes 3003 and 3004: 0.00625 a count, from -100).
    let accel = |a: f64| ((a + 100.0) / 0.00625) as u16;
    let each = |f: &dyn Fn(&Sample) -> u16| run.samples.iter().map(f).collect::<Vec<u16>>();
    let channels: Vec<(&str, u16, u8, Vec<u16>)> = vec![
        ("Engine", 0, 3, each(&|s| s.rpm as u16)),
        ("GPS_Speed", 1, 3, each(&|s| (s.kmh * 10.0) as u16)),
        ("GPS_LatAcc", 3003, 3, each(&|s| accel(s.sideways))),
        ("GPS_LonAcc", 3004, 3, each(&|s| accel(s.forward))),
        ("Exhaust Temp", 45, 3, egt_raw),
        ("Battery", 17, 0, batt),
    ];
    for (name, code, rate, raw) in &channels {
        f.extend(hdr(b"MM"));
        let mut mm = vec![0u8; 0x400];
        mm[..name.len()].copy_from_slice(name.as_bytes());
        mm[0x40..0x42].copy_from_slice(&code.to_le_bytes());
        mm[0x46] = *rate;
        mm[0x48..0x4c].copy_from_slice(&(raw.len() as u32 + 3).to_le_bytes());
        f.extend(mm);
    }
    for &ms in &run.laps_ms {
        f.extend(hdr(b"GG"));
        let mut g = vec![0u8; 0x100];
        g[0x04..0x08].copy_from_slice(&ms.to_le_bytes()); // one tick a millisecond
        g[0x76..0x7a].copy_from_slice(&ms.to_le_bytes());
        f.extend(g);
    }
    f.extend(hdr(b"RR"));
    f.extend(vec![0u8; 0x1000]);
    for (_, _, _, raw) in &channels {
        for v in raw.iter().chain(&[0x4316, 0x2620, 0x1015]) {
            f.extend(v.to_le_bytes());
        }
    }

    // GPS at 8 Hz: where the kart was, as offsets from the track's center.
    let origin = earth_centered(LAT0, LON0);
    let mut g = b"PROV".to_vec();
    g.extend([0u8; 10]);
    for x in [origin.0, origin.1, origin.2] {
        g.extend(x.to_le_bytes());
    }
    let m = line.len();
    for (k, &Sample { at: s, .. }) in run
        .samples
        .iter()
        .enumerate()
        .filter(|(k, _)| k % 5 == 2 || k % 5 == 0)
    {
        // 50 ms samples, two of every five: 8 a second, as the GPS module records.
        let t = k as u32 * 50 + if k % 5 == 2 { 25 } else { 0 };
        let (i, frac) = (s.floor() as usize % m, s.fract());
        let (a, b) = (line[i], line[(i + 1) % m]);
        let wobble = 0.4 * (dice.next() - 0.5); // a kart's own line, and GPS noise
        let (e, nn) = (
            a.0 + (b.0 - a.0) * frac + wobble,
            a.1 + (b.1 - a.1) * frac - wobble,
        );
        g.extend(b"PSOL");
        g.extend(t.to_le_bytes());
        g.extend([0u8; 8]);
        for x in [e, nn, 0.0] {
            g.extend(x.to_le_bytes());
        }
    }
    (f, g)
}

/// Writes the made-up library into `home` (as `$HOME`): `Mychronix/` with
/// its settings, Garage, custom track and sessions. Settings turn off
/// everything that would reach the internet.
pub fn write_library(home: &Path) {
    let root = home.join("Mychronix");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tracks/custom")).unwrap();
    let write = |rel: &str, bytes: &[u8]| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    };
    write(
        "settings.json",
        br#"{"units": "imperial", "clock": "12", "weather": false, "update_check": false, "aim_tracks": false, "mcp": false}"#,
    );

    let line = center_line();
    let length: f64 = line
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum();
    let mut dice = Dice(20300504);
    // The track as a MyChron would make it, from a lap at the quicker pace.
    let v = speeds(&line, 19.5, 11.0);
    let lap: Vec<aim_track::write::LapPoint> = line
        .iter()
        .zip(&v)
        .map(|(&p, &v)| {
            let (lat, lon) = east_north_to_lat_lon(p);
            aim_track::write::LapPoint {
                at: aim_track::LatLon { lat, lon },
                kmh: (v * 3.6) as f32,
            }
        })
        .collect();
    let tkk = aim_track::write::from_lap(
        TRACK,
        "pinecrst",
        [7, 3, 1, 9, 2, 6, 5, 4],
        &lap,
        length as f32,
        None,
    )
    .unwrap();
    write("tracks/custom/pinecrst.tkk", &tkk);

    let setup = |driven: u32| {
        format!(
            r#"{{"drive_teeth": "10", "driven_teeth": "{driven}", "tires": "MG Yellow", "front_size": "10x4.50-5", "rear_size": "11x7.10-5", "front_psi": "10", "rear_psi": "10.5", "weight_lb": "320", "ballast_lb": "15", "jetting": "", "notes": ""}}"#
        )
    };
    write(
        "profiles.json",
        format!(
            r#"{{
  "drivers": [
    {{"id": 1, "name": "Alex", "class": "Junior", "born": "2016-03-14", "weight_lb": "95"}},
    {{"id": 2, "name": "Sam", "class": "Cadet", "born": "2019-08-02", "weight_lb": "70"}}
  ],
  "karts": [
    {{"id": 1, "name": "Blue #21", "engine": "IAME KA100", "driver": 1, "logger": "1001", "baseline": {}}},
    {{"id": 2, "name": "Red #8", "engine": "Vortex Mini ROK", "driver": 2, "logger": "1002", "baseline": {}}}
  ],
  "loggers": {{
    "1001": {{"championship": "Lakes Region Series", "track": "{TRACK}", "model": "MyChron 4"}},
    "1002": {{"championship": "Lakes Region Series", "track": "{TRACK}", "model": "MyChron 4"}}
  }}
}}"#,
            setup(78),
            setup(82)
        )
        .as_bytes(),
    );

    // Three race days, a practice, a heat and a final each, for both drivers.
    let days = [
        ("04-05-30", "2030-05-04"),
        ("18-05-30", "2030-05-18"),
        ("01-06-30", "2030-06-01"),
    ];
    let sessions = [
        ("09:12:00", "Practice", 8),
        ("11:40:00", "Heat 1", 6),
        ("14:25:00", "Final", 10),
    ];
    let drivers = [
        (1001u32, "Alex", "Blue #21", 1.0, 145.0),
        (1002, "Sam", "Red #8", 0.94, 152.0),
    ];
    for (serial, driver, kart, pace, rpm_per_kmh) in drivers {
        let mut number = 1;
        for (d, (stamp_day, _)) in days.iter().enumerate() {
            for (time, kind, laps) in sessions {
                // Each race day a little quicker than the last.
                let pace = pace * (0.985 + 0.0075 * d as f64);
                let run = drive(&line, laps, pace, rpm_per_kmh, &mut dice);
                let (drk, gpk) = drk(
                    &run,
                    &line,
                    serial,
                    driver,
                    &format!("{stamp_day} {time}"),
                    &mut dice,
                );
                let name = format!("sessions/{serial}/{TRACK} {stamp_day}_{number:03}");
                write(&format!("{name}.drk"), &drk);
                write(&format!("{name}.gpk"), &gpk);
                write(
                    &format!("{name}.meta.json"),
                    format!(
                        r#"{{"driver": "{driver}", "kart": "{kart}", "championship": "Lakes Region Series", "track": "{TRACK}", "session_type": "{kind}", "comment": ""}}"#
                    )
                    .as_bytes(),
                );
                number += 1;
            }
        }
    }
}
