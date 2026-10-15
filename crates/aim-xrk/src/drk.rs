//! RaceStudio 2 `.drk` sessions (MyChron 4 and other pre-2015 AiM loggers),
//! read from the file structure worked out in docs/protocol-notes.md.
//!
//! A `.drk` is a run of blocks, each a 32-byte header (two letters, the
//! format version as a u16, a checksum) and its data: `RD` the session (date
//! and time), `PP` its names (track, driver), `II`, one `MM` per channel, one
//! `GG` per lap and `RR` the samples (a 4 KB table, then every channel's
//! samples as u16, channel after channel).
//!
//! Time: the logger counts its own clock ticks (nominally 1 ms; a MyChron 4
//! here ran 1.5% fast) and samples every 100 ticks for fast channels, 1000
//! for slow ones. Each lap records its time in both ticks and milliseconds,
//! so ticks are scaled to milliseconds the way RaceStudio 2 does.

use crate::{Channel, Error, Lap, Result, Session, SessionInfo};

/// Placeholder for an input with nothing connected.
const NOT_CONNECTED: u16 = 0xCFFF;
/// Samples at the end of each channel that are a trailer, not readings.
const TRAILER: usize = 3;

struct Block<'a> {
    tag: [u8; 2],
    data: &'a [u8],
}

fn blocks(data: &[u8]) -> Result<Vec<Block<'_>>> {
    // Header starts: two capital letters followed by the version (600 = 0x0258).
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        if data[i].is_ascii_uppercase() && data[i + 1].is_ascii_uppercase() && data[i + 2] == 0x58 && data[i + 3] == 0x02 {
            starts.push(i);
            i += 0x20;
        } else {
            i += 1;
        }
    }
    if starts.first() != Some(&0) {
        return Err(Error::Parse("not a RaceStudio 2 .drk file (or a version this can't read yet)".into()));
    }
    Ok(starts
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let end = starts.get(k + 1).copied().unwrap_or(data.len());
            Block { tag: [data[s], data[s + 1]], data: &data[(s + 0x20).min(end)..end] }
        })
        .collect())
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn text_at(b: &[u8], o: usize, max: usize) -> Option<String> {
    let s = b.get(o..(o + max).min(b.len()))?;
    let s = &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())];
    let s: String = s.iter().map(|&c| c as char).collect::<String>().trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// A channel's units and how its raw u16 samples become values, by name.
fn conversion(name: &str) -> (&'static str, u8, fn(u16) -> f64) {
    match name {
        "Engine" => ("rpm", 0, |r| r as f64),
        // Calibrated against RaceStudio 2's display (8.2 V for a raw ~1495).
        "Battery" => ("V", 1, |r| r as f64 * 0.0055),
        "Datalogger_Temp" => ("C", 1, |r| r as f64 / 100.0),
        // Head/water temperature: same scale as the logger's own, by assumption
        // until a session with the sensor connected confirms it.
        _ => ("C", 1, |r| r as f64 / 100.0),
    }
}

pub fn read(data: &[u8]) -> Result<Session> {
    let blocks = blocks(data)?;
    let find = |t: &[u8; 2]| blocks.iter().find(|b| &b.tag == t);

    // Laps: time in ms at 0x76, in logger ticks at 0x04.
    let laps_raw: Vec<(u32, u32)> = blocks
        .iter()
        .filter(|b| &b.tag == b"GG")
        .filter_map(|b| Some((u32_at(b.data, 0x76)?, u32_at(b.data, 0x04)?)))
        .filter(|&(ms, ticks)| ms > 0 && ticks > 0)
        .collect();
    let (ms_sum, tick_sum) = laps_raw.iter().fold((0u64, 0u64), |a, &(m, t)| (a.0 + m as u64, a.1 + t as u64));
    let ms_per_tick = if tick_sum > 0 { ms_sum as f64 / tick_sum as f64 } else { 1.0 };
    let mut laps = Vec::new();
    let mut t = 0i64;
    for (k, &(ms, _)) in laps_raw.iter().enumerate() {
        laps.push(Lap { number: k as i32 + 1, start_ms: t, end_ms: t + ms as i64 });
        t += ms as i64;
    }
    let total_ms = t;

    // Channels: name at 0, sample count at 0x48; samples follow RR's table.
    let rr = find(b"RR").ok_or_else(|| Error::Parse("the .drk has no samples".into()))?.data;
    let mut pos = 0x1000;
    let mut channels = Vec::new();
    let fast_n = blocks.iter().filter(|b| &b.tag == b"MM").filter_map(|b| u32_at(b.data, 0x48)).max().unwrap_or(0);
    for b in blocks.iter().filter(|b| &b.tag == b"MM") {
        let name = text_at(b.data, 0, 20).unwrap_or_else(|| "Channel".into());
        let n = u32_at(b.data, 0x48).unwrap_or(0) as usize;
        let raw: Vec<u16> = (0..n).filter_map(|i| u16_at(rr, pos + i * 2)).collect();
        pos += n * 2;
        // Each channel ends with a short trailer that isn't readings (values
        // like 0x4316, 0x2620, 0x1015, 0x7FFF in the last two or three
        // samples); the readings before it match RaceStudio 2 exactly.
        let mut raw = raw;
        raw.truncate(raw.len().saturating_sub(TRAILER));
        if raw.len() + TRAILER < n {
            return Err(Error::Parse(format!("the .drk's samples for {name} are cut short")));
        }
        // Slow channels record a tenth as often (1000 ticks instead of 100).
        let step_ticks = if n > 0 && fast_n as usize > n * 5 { 1000.0 } else { 100.0 };
        let (units, decimals, convert) = conversion(&name);
        // Mostly "nothing connected" (a few stray zeros aside): no sensor fitted.
        let unplugged = raw.iter().filter(|&&r| r == NOT_CONNECTED).count() * 10 >= raw.len() * 9;
        // Readings past the channel's full scale (a float at 0x50) are glitches,
        // e.g. single RPM samples of 35,000.
        let full_scale = b.data.get(0x50..0x54).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]) as f64).filter(|v| *v > 0.0);
        let (times_ms, values): (Vec<i64>, Vec<f64>) = raw
            .iter()
            .enumerate()
            .filter(|(_, r)| !unplugged && **r != NOT_CONNECTED)
            .map(|(i, &r)| ((i as f64 * step_ticks * ms_per_tick).round() as i64, convert(r)))
            .filter(|(_, v)| full_scale.is_none_or(|f| *v <= f * 1.25))
            .unzip();
        channels.push(Channel { name, group: None, units: units.into(), decimals, interpolate: true, times_ms, values });
    }
    let _ = total_ms;

    // Names from PP (track at 0x28, driver at 0x50); date and time from RD.
    let pp = find(b"PP").map(|b| b.data).unwrap_or(&[]);
    let rd = blocks.get(1).filter(|b| &b.tag == b"RD").map(|b| b.data).unwrap_or(&[]);
    let stamp = text_at(rd, 0x0c, 20); // "11-10-26 02:21:31"
    let (date, time) = match stamp.as_deref().and_then(|s| s.split_once(' ')) {
        Some((d, t)) => {
            // DD-MM-YY → MM/DD/YYYY, as libxrk reports dates.
            let p: Vec<&str> = d.split('-').collect();
            let date = (p.len() == 3).then(|| format!("{}/{}/20{}", p[1], p[0], p[2]));
            (date, Some(t.to_string()))
        }
        None => (None, None),
    };
    // The logger's serial number is at 0x10 of the II block (and repeated in
    // RD); AiM's model code is the u16 at 0x04 (106 on a MyChron 4).
    let ii = find(b"II").map(|b| b.data).unwrap_or(&[]);
    let logger_id = u32_at(ii, 0x10).filter(|&n| n > 0);
    let model_id = u16_at(ii, 0x04).filter(|&m| m > 0);
    let info = SessionInfo {
        logger_id,
        logger_model_id: model_id,
        driver: text_at(pp, 0x50, 40),
        venue: text_at(pp, 0x28, 40),
        session: text_at(pp, 0x78, 60),
        date,
        time,
        device: Some("MyChron 4".into()),
        logger_model: Some("MyChron 4".into()),
        ..Default::default()
    };
    Ok(Session { info, channels, laps, gps: None })
}

/// The list-view summary of a `.drk`, worked out from the session itself
/// (RaceStudio 2 sessions have no `.xri` beside them).
pub fn summary(path: &std::path::Path) -> Result<crate::summary::SessionSummary> {
    let s = read(&std::fs::read(path)?)?;
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let date = s.info.date.as_deref().and_then(|d| {
        let p: Vec<u32> = d.split('/').filter_map(|x| x.parse().ok()).collect();
        (p.len() == 3).then(|| crate::summary::Date { year: p[2] as u16, month: p[0] as u8, day: p[1] as u8 })
    });
    // The first lap runs from the pits; with no other lap, nothing was timed.
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

    /// A minimal .drk: RD header, RD, PP, one MM, two GG laps, RR.
    fn sample() -> Vec<u8> {
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
        rd[0x0c..0x0c + 17].copy_from_slice(b"11-10-26 14:21:31");
        f.extend(rd);
        f.extend(hdr(b"PP"));
        let mut pp = vec![0u8; 0x400];
        pp[0x28..0x2b].copy_from_slice(b"NKA");
        f.extend(pp);
        f.extend(hdr(b"II"));
        let mut ii = vec![0u8; 0x80];
        ii[0x04..0x06].copy_from_slice(&106u16.to_le_bytes());
        ii[0x10..0x14].copy_from_slice(&123456u32.to_le_bytes());
        f.extend(ii);
        f.extend(hdr(b"MM"));
        let mut mm = vec![0u8; 0x400];
        mm[..6].copy_from_slice(b"Engine");
        mm[0x48..0x4c].copy_from_slice(&6u32.to_le_bytes());
        f.extend(mm);
        for (ms, ticks) in [(1000u32, 990u32), (2000, 1980)] {
            f.extend(hdr(b"GG"));
            let mut g = vec![0u8; 0x100];
            g[0x04..0x08].copy_from_slice(&ticks.to_le_bytes());
            g[0x76..0x7a].copy_from_slice(&ms.to_le_bytes());
            f.extend(g);
        }
        f.extend(hdr(b"RR"));
        let mut rr = vec![0u8; 0x1000];
        for v in [5000u16, 6000, NOT_CONNECTED, 0x4316, 0x2620, 0x1015] {
            rr.extend(v.to_le_bytes());
        }
        f.extend(rr);
        f
    }

    #[test]
    fn reads_channels_laps_and_names() {
        let s = read(&sample()).unwrap();
        assert_eq!(s.info.venue.as_deref(), Some("NKA"));
        assert_eq!((s.info.logger_id, s.info.logger_model.as_deref()), (Some(123456), Some("MyChron 4")));
        assert_eq!(s.info.date.as_deref(), Some("10/11/2026"));
        assert_eq!(s.laps.iter().map(|l| l.duration_ms()).collect::<Vec<_>>(), [1000, 2000]);
        let e = &s.channels[0];
        assert_eq!(e.values, [5000.0, 6000.0]); // the "nothing connected" sample is dropped
        assert_eq!(e.times_ms, [0, 101]); // 100 ticks at 1000/990 ms per tick
    }
}
