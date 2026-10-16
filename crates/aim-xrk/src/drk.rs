//! RaceStudio 2 `.drk` sessions (MyChron 4 and other pre-2015 AiM loggers),
//! read from the file structure worked out in docs/protocol-notes.md.
//!
//! A `.drk` is a run of blocks, each a 32-byte header (two letters, the
//! format version as a u16, a checksum) and its data: `RD` the session (date
//! and time), `PP` its names (track, driver), `II`, one `MM` per channel, one
//! `GG` per lap and `RR` the samples (a 4 KB table, then every channel's
//! samples as u16, channel after channel). The table can span several 4 KB
//! `RR` blocks.
//!
//! Time: the logger counts its own clock ticks (nominally 1 ms; a MyChron 4
//! here ran 1.5% fast) and samples every 100 ticks for fast channels, 1000
//! for slow ones. Each lap records its time in both ticks and milliseconds,
//! so ticks are scaled to milliseconds the way RaceStudio 2 does.

use crate::sensors::{is_error, rate_hz, scale};
use crate::{Channel, Error, Lap, Result, Session, SessionInfo};

/// Samples at the end of each channel that are a trailer, not readings.
const TRAILER: usize = 3;

struct Block<'a> {
    tag: [u8; 2],
    data: &'a [u8],
}

fn is_header(data: &[u8], i: usize) -> bool {
    data.get(i..i + 4).is_some_and(|h| h[0].is_ascii_uppercase() && h[1].is_ascii_uppercase() && h[2] == 0x58 && h[3] == 0x02)
}

/// The blocks before the samples, and the samples.
/// Blocks are looked for only up to the samples, whose bytes could look
/// like a header.
fn blocks(data: &[u8]) -> Result<(Vec<Block<'_>>, &[u8])> {
    // Header starts: two capital letters followed by the version (600 = 0x0258).
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        if is_header(data, i) {
            starts.push(i);
            if &data[i..i + 2] == b"RR" {
                break;
            }
            i += 0x20;
        } else {
            i += 1;
        }
    }
    if starts.first() != Some(&0) {
        return Err(Error::Parse("not a RaceStudio 2 .drk file (or a version this can't read yet)".into()));
    }
    // RR starts with a table of 4 KB blocks, each with its own RR header (as
    // many as the first word says, though RaceStudio 2 can compact them away
    // without changing it); the samples follow the last one.
    let mut samples: &[u8] = &[];
    if let Some(&s) = starts.last().filter(|&&s| &data[s..s + 2] == b"RR") {
        let mut s = s;
        while data.get(s + 0x1020..s + 0x1022) == Some(b"RR") && is_header(data, s + 0x1020) {
            s += 0x1020;
        }
        samples = data.get(s + 0x1020..).unwrap_or(&[]);
    }
    let blocks = starts
        .iter()
        .enumerate()
        .filter(|&(_, &s)| &data[s..s + 2] != b"RR")
        .map(|(k, &s)| {
            let end = starts.get(k + 1).copied().unwrap_or(data.len());
            Block { tag: [data[s], data[s + 1]], data: &data[(s + 0x20).min(end)..end] }
        })
        .collect();
    Ok((blocks, samples))
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

pub fn read(data: &[u8]) -> Result<Session> {
    read_with_clock(data).map(|(s, _)| s)
}

/// The session and its milliseconds per logger tick.
pub fn read_with_clock(data: &[u8]) -> Result<(Session, f64)> {
    let (blocks, rr) = blocks(data)?;
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
    if rr.is_empty() {
        return Err(Error::Parse("the .drk has no samples".into()));
    }
    let mut pos = 0;
    let mut channels = Vec::new();
    let fast_n = blocks.iter().filter(|b| &b.tag == b"MM").filter_map(|b| u32_at(b.data, 0x48)).max().unwrap_or(0);
    for b in blocks.iter().filter(|b| &b.tag == b"MM") {
        // GPS channels as MyChron 5/6 logs name them ("GPS Speed").
        let name = text_at(b.data, 0, 20).unwrap_or_else(|| "Channel".into()).replace("GPS_", "GPS ");
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
        // Ticks between samples (1000 a second): from the channel's rate code at
        // 0x46, else slow channels record a tenth as often as the fastest.
        let step_ticks = match b.data.get(0x46).copied().and_then(rate_hz) {
            Some(hz) => 1000.0 / hz as f64,
            None if n > 0 && fast_n as usize > n * 5 => 1000.0,
            None => 100.0,
        };
        // Scaling by the channel's AiM sensor code (at 0x40), with its own
        // calibration (zero and gain, floats at 0x5C and 0x60) for sensors
        // calibrated per channel. Unknown sensors keep their raw counts.
        let f32_at = |o: usize| b.data.get(o..o + 4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]) as f64);
        let calibration = f32_at(0x5C).zip(f32_at(0x60));
        let sc = u16_at(b.data, 0x40).and_then(|c| scale(c, calibration));
        // Mostly "nothing connected" (a few stray zeros aside): no sensor fitted.
        let unplugged = raw.iter().filter(|&&r| is_error(r)).count() * 10 >= raw.len() * 9;
        // Readings past the channel's full scale (a float at 0x50) are glitches,
        // e.g. single RPM samples of 35,000.
        let full_scale = b.data.get(0x50..0x54).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]) as f64).filter(|v| *v > 0.0);
        let (times_ms, values): (Vec<i64>, Vec<f64>) = raw
            .iter()
            .enumerate()
            .filter(|(_, r)| !unplugged && !is_error(**r))
            .map(|(i, &r)| ((i as f64 * step_ticks * ms_per_tick).round() as i64, sc.map_or(r as f64, |sc| sc.apply(r))))
            // RPM readings far past full scale are glitches (other channels'
            // full scale can be in other units than their values).
            .filter(|(_, v)| sc.is_none_or(|sc| sc.units != "rpm") || full_scale.is_none_or(|f| *v <= f * 1.25))
            .unzip();
        let (mut units, decimals) = sc.map_or((String::new(), 0), |sc| (sc.units.to_string(), sc.decimals));
        // Imported channels carry their unit as text where others have a code.
        if u16_at(b.data, 0x40) == Some(999) {
            let text: String = b.data[0x14..0x20].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
            let text = text.trim();
            if !text.is_empty() && !text.starts_with(|c: char| c.is_ascii_digit()) {
                units = match text {
                    "°C" => "C".into(),
                    "°F" => "F".into(),
                    "#" | "gear" => String::new(),
                    t => t.into(),
                };
            }
        }
        channels.push(Channel { name, group: None, units, decimals, interpolate: true, times_ms, values });
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
        logger_model: model_id.and_then(crate::mychron_model).map(str::to_string),
        ..Default::default()
    };
    Ok((Session { info, channels, laps, gps: None }, ms_per_tick))
}

/// A file beside `path` with the same name and extension `ext` (any case).
fn companion(path: &std::path::Path, ext: &str) -> Option<std::path::PathBuf> {
    let stem = path.file_stem()?;
    std::fs::read_dir(path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."))).ok()?.filter_map(|e| e.ok().map(|e| e.path())).find(|p| {
        p.file_stem() == Some(stem) && p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext))
    })
}

/// Opens a `.drk` with what RaceStudio 2 keeps beside it: GPS positions from
/// its `.gpk` (else `.rrk`), placed on the session's clock; its `.imd` (the
/// raw MyChron 4 download), checked against the `.drk`; and, when the `.drk`
/// is damaged, its `.bak` (RaceStudio 2 Analysis's copy of the original) or
/// the `.imd` instead.
pub fn open(path: &std::path::Path) -> Result<Session> {
    let imd = || companion(path, "imd").and_then(|p| std::fs::read(p).ok()).and_then(|d| crate::mc4::read_imd(&d).ok());
    let (mut s, ms_per_tick) = match read_with_clock(&std::fs::read(path)?) {
        Ok(x) => x,
        Err(e) => {
            // RaceStudio 2 Analysis's backup of the original .drk, else the raw download.
            if let Some(x) = companion(path, "bak").and_then(|p| std::fs::read(p).ok()).and_then(|d| read_with_clock(&d).ok()) {
                log::warn!("{}: {e}; read its .bak instead", path.display());
                x
            } else if let Some(s) = imd() {
                log::warn!("{}: {e}; read its .imd instead", path.display());
                return Ok(s);
            } else {
                return Err(e);
            }
        }
    };
    if let Some(raw) = imd() {
        let engine = |s: &Session| s.channel("Engine").map(|c| c.values.clone()).unwrap_or_default();
        let (a, b) = (engine(&s), engine(&raw));
        let n = a.len().min(b.len());
        let differ = (0..n).filter(|&i| (a[i] - b[i]).abs() > 1e-9).count();
        if s.laps.len() != raw.laps.len() || differ > 0 || a.len().abs_diff(b.len()) > 12 {
            log::warn!("{}: its .imd disagrees ({} vs {} laps, {differ} RPM samples differ)", path.display(), s.laps.len(), raw.laps.len());
        }
    }
    // Positions from the processed .gpk, with the receiver's own speed and
    // heading from the .rrk; either alone will do.
    let read = |ext| companion(path, ext).and_then(|p| std::fs::read(p).ok());
    let raw = read("rrk").map(|d| crate::rs2gps::read_rrk(&d)).unwrap_or_default();
    let processed = read("gpk").map(|d| crate::rs2gps::read_gpk(&d)).unwrap_or_default();
    let fixes = if processed.len() >= 10 { crate::rs2gps::with_velocity(processed, &raw) } else { raw };
    let fixes = Some(fixes).filter(|f| f.len() >= 10);
    if let Some(fixes) = fixes {
        // The fixes carry a time on the logger's clock: its ticks (scaled like
        // the samples) or already milliseconds; whichever lines their speed
        // up with the session's GPS speed the same at the start and the end
        // wins, and the shift that does it fine-tunes them.
        let mut scale = ms_per_tick;
        let mut offset = 0;
        if let Some(sp) = s.channel("GPS Speed") {
            let half = fixes.len() / 2;
            let fit = |k: f64| -> Option<(i64, i64, f64)> {
                let (a, b) = (crate::rs2gps::best_offset(&fixes[..half], k, sp)?, crate::rs2gps::best_offset(&fixes[half..], k, sp)?);
                (a.1 > 0.9 && b.1 > 0.9).then_some(((a.0 - b.0).abs(), (a.0 + b.0) / 2, a.1.min(b.1)))
            };
            let candidates = [ms_per_tick, 1.0].into_iter().filter_map(|k| Some((k, fit(k)?)));
            if let Some((k, (_, off, r))) = candidates.min_by_key(|(_, f)| f.0) {
                log::info!("{}: GPS positions at {k:.5} ms per tick, shifted {off} ms (speed match {r:.3})", path.display());
                (scale, offset) = (k, off);
            }
        }
        s.gps = Some(crate::rs2gps::track(&fixes, scale, offset));
    }
    Ok(s)
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
        model: s.info.logger_model.clone(),
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
        mm[0x46] = 2; // 10 Hz
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
        for v in [5000u16, 6000, 0xCFFF, 0x4316, 0x2620, 0x1015] {
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
