//! Runs downloaded straight from a MyChron 4 data key (`.mc4`, written by
//! `aim_link::datakey::Run::to_file`): the key's 256-byte run header, its
//! 64-byte lap records and the raw samples. Format in docs/protocol-notes.md.
//!
//! Samples are u16 words in blocks of 10 frames: a `0xD0xx` marker (not
//! before the first block), then `[RPM, temp] [logger temp, battery]` and
//! nine more `[RPM, temp]`. Each lap ends with a 6-word record starting
//! `0xD2xx` (lap number, lap time in hundredths), between frames. Fast channels are
//! every 100 logger ticks, slow ones every 1000; the logger's clock drifts,
//! so ticks are scaled so the samples span the laps (which cover the run).

use crate::{Channel, Error, Lap, Result, Session, SessionInfo};

const NOT_CONNECTED: u16 = 0xCFFF;

struct Parts<'a> {
    header: &'a [u8],
    laps: &'a [u8],
    data: &'a [u8],
}

fn parts(file: &[u8]) -> Result<Parts<'_>> {
    let bad = || Error::Parse("not a downloaded MyChron 4 run (.mc4)".into());
    if file.get(..4) != Some(b"MC4R") {
        return Err(bad());
    }
    let mut pos = 8;
    let mut take = || -> Result<&[u8]> {
        let n = u32::from_le_bytes(file.get(pos..pos + 4).ok_or_else(bad)?.try_into().unwrap()) as usize;
        let part = file.get(pos + 4..pos + 4 + n).ok_or_else(bad)?;
        pos += 4 + n;
        Ok(part)
    };
    Ok(Parts { header: take()?, laps: take()?, data: take()? })
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn text_at(b: &[u8], o: usize, max: usize) -> Option<String> {
    let s = b.get(o..(o + max).min(b.len()))?;
    let end = s.iter().position(|&c| c == 0 || c == 0xFF).unwrap_or(s.len());
    let t: String = s[..end].iter().map(|&c| c as char).collect::<String>().trim().to_string();
    (!t.is_empty()).then_some(t)
}

fn bcd(b: u8) -> u32 {
    (b >> 4) as u32 * 10 + (b & 0x0F) as u32
}

/// The four channels of a MyChron 4 without expansions, as raw words.
struct Samples {
    rpm: Vec<u16>,
    temp: Vec<u16>,
    logger_temp: Vec<u16>,
    battery: Vec<u16>,
}

fn samples(data: &[u8]) -> Samples {
    let w: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let mut s = Samples { rpm: Vec::new(), temp: Vec::new(), logger_temp: Vec::new(), battery: Vec::new() };
    let (mut i, mut frame) = (0, 0usize);
    while i + 1 < w.len() {
        match w[i] {
            // A lap record: marker, lap number, lap time in hundredths (u32), two more words.
            0xD200..=0xD2FF => {
                i += 6;
                continue;
            }
            // Block marker before every tenth frame.
            0xD000..=0xD0FF => {
                i += 1;
                continue;
            }
            _ => {}
        }
        s.rpm.push(w[i]);
        s.temp.push(w[i + 1]);
        i += 2;
        if frame % 10 == 0 && i + 1 < w.len() {
            s.logger_temp.push(w[i]);
            s.battery.push(w[i + 1]);
            i += 2;
        }
        frame += 1;
    }
    s
}

pub fn read(file: &[u8]) -> Result<Session> {
    let p = parts(file)?;
    let h = p.header;

    // Laps: one 64-byte record each, time in hundredths at +4.
    let mut laps = Vec::new();
    let mut t = 0i64;
    for (k, rec) in p.laps.chunks_exact(64).enumerate() {
        let Some(cs) = u32_at(rec, 4).filter(|&c| c > 0 && c < 0x7FFF_FFFF) else { continue };
        let ms = cs as i64 * 10;
        laps.push(Lap { number: k as i32 + 1, start_ms: t, end_ms: t + ms });
        t += ms;
    }

    let s = samples(p.data);
    // Nominal 100 ms per fast sample, scaled to the laps when they cover the run.
    let nominal = s.rpm.len() as f64 * 100.0;
    let scale = if nominal > 0.0 && t > 0 { (t as f64 / nominal).clamp(0.95, 1.05) } else { 1.0 };
    let chan = |name: &str, units: &str, decimals: u8, raw: &[u16], step: f64, conv: fn(u16) -> f64| {
        let unplugged = raw.iter().filter(|&&r| r == NOT_CONNECTED).count() * 10 >= raw.len() * 9;
        let (times_ms, values) = raw
            .iter()
            .enumerate()
            .filter(|(_, r)| !unplugged && **r != NOT_CONNECTED)
            .map(|(i, &r)| ((i as f64 * step * scale).round() as i64, conv(r)))
            .unzip();
        Channel { name: name.into(), group: None, units: units.into(), decimals, interpolate: true, times_ms, values }
    };
    let channels = vec![
        chan("Engine", "rpm", 0, &s.rpm, 100.0, |r| r as f64),
        chan("Temperature", "C", 1, &s.temp, 100.0, |r| r as f64 / 100.0),
        chan("Datalogger_Temp", "C", 1, &s.logger_temp, 1000.0, |r| r as f64 / 100.0),
        chan("Battery", "V", 1, &s.battery, 1000.0, |r| r as f64 * 0.0055),
    ];

    // Header: run number at 0, driver at 0x12, track at 0x32, serial at 0x78,
    // model at 0x7C, start time as BCD seconds/minutes/hours/day/weekday/month/year at 0x9A.
    let stamp = h.get(0x9A..0xA1);
    let (date, time) = match stamp {
        Some(b) if bcd(b[3]) > 0 && bcd(b[5]) > 0 => (
            Some(format!("{:02}/{:02}/{}", bcd(b[5]), bcd(b[3]), 2000 + bcd(b[6]))),
            Some(format!("{:02}:{:02}:{:02}", bcd(b[2]), bcd(b[1]), bcd(b[0]))),
        ),
        _ => (None, None),
    };
    let info = SessionInfo {
        driver: text_at(h, 0x12, 32),
        venue: text_at(h, 0x32, 32),
        session: u16_at(h, 0).map(|n| format!("Run {n}")),
        date,
        time,
        device: Some("MyChron 4".into()),
        logger_id: u32_at(h, 0x78).filter(|&n| n > 0),
        logger_model_id: u16_at(h, 0x7C),
        logger_model: Some("MyChron 4".into()),
        vehicle: None,
    };
    if channels[0].values.is_empty() && laps.is_empty() {
        return Err(Error::Parse("the downloaded run holds no data".into()));
    }
    Ok(Session { info, channels, laps, gps: None })
}

/// The list-view summary of a `.mc4`.
pub fn summary(path: &std::path::Path) -> Result<crate::summary::SessionSummary> {
    let s = read(&std::fs::read(path)?)?;
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let date = s.info.date.as_deref().and_then(|d| {
        let p: Vec<u32> = d.split('/').filter_map(|x| x.parse().ok()).collect();
        (p.len() == 3).then(|| crate::summary::Date { year: p[2] as u16, month: p[0] as u8, day: p[1] as u8 })
    });
    let best = s.laps.iter().skip(1).filter(|l| l.duration_ms() >= 10_000).min_by_key(|l| l.duration_ms());
    let duration = s.channels.iter().filter_map(|c| c.times_ms.last()).max().copied().max(s.laps.last().map(|l| l.end_ms));
    let max_rpm = s.channel("Engine").and_then(|c| c.values.iter().copied().reduce(f64::max)).map(|v| v as u32);
    Ok(crate::summary::SessionSummary {
        number: file.as_deref().and_then(crate::summary::session_number),
        file,
        date,
        start_time: s.info.time.clone(),
        track: s.info.venue.clone(),
        duration_ms: duration.map(|d| d.max(0) as u64),
        laps: Some(s.laps.len() as u32),
        best_lap_ms: best.map(|l| l.duration_ms() as u64),
        best_lap_number: best.map(|l| l.number as u32),
        max_rpm,
        driver: s.info.driver.clone(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(header: &[u8], laps: &[u8], data: &[u8]) -> Vec<u8> {
        let mut f = b"MC4R".to_vec();
        f.extend(1u32.to_le_bytes());
        for part in [header, laps, data] {
            f.extend((part.len() as u32).to_le_bytes());
            f.extend(part);
        }
        f
    }

    #[test]
    fn decodes_blocks_markers_and_end() {
        let mut h = vec![0xFFu8; 256];
        h[0..2].copy_from_slice(&7u16.to_le_bytes());
        h[0x12..0x16].copy_from_slice(b"ANN\0");
        h[0x78..0x7C].copy_from_slice(&123456u32.to_le_bytes());
        h[0x7C..0x7E].copy_from_slice(&106u16.to_le_bytes());
        h[0x9A..0xA1].copy_from_slice(&[0x51, 0x31, 0x07, 0x19, 0x05, 0x09, 0x26]);
        let mut lap = vec![0xFFu8; 64];
        lap[4..8].copy_from_slice(&200u32.to_le_bytes()); // 2.00 s
        // 20 frames: block 1 without marker, block 2 with one, then the end.
        let mut w: Vec<u16> = Vec::new();
        for f in 0..20u16 {
            if f == 10 {
                w.push(0xD01F);
            }
            w.extend([5000 + f, NOT_CONNECTED]);
            if f % 10 == 0 {
                w.extend([2400, 1500]);
            }
        }
        w.extend([0xD202, 1, 200, 0, 0, 0x1234]);
        let data: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        let s = read(&file(&h, &lap, &data)).unwrap();
        let e = s.channel("Engine").unwrap();
        assert_eq!(e.values.len(), 20);
        assert_eq!(e.values[10], 5010.0);
        assert_eq!(s.channel("Battery").unwrap().values.len(), 2);
        assert!(s.channel("Temperature").unwrap().values.is_empty());
        assert_eq!(s.info.date.as_deref(), Some("09/19/2026"));
        assert_eq!(s.info.time.as_deref(), Some("07:31:51"));
        assert_eq!((s.info.logger_id, s.info.driver.as_deref()), (Some(123456), Some("ANN")));
        assert_eq!(s.laps[0].duration_ms(), 2000);
    }
}
