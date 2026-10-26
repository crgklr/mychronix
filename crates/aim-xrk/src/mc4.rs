//! Runs downloaded straight from a MyChron 4 data key (`.mc4`, written by
//! `aim_link::datakey::Run::to_file`): the key's 256-byte run header, its
//! 64-byte lap records and the raw samples. Format in docs/protocol-notes.md.
//!
//! Which channels a run holds comes from its header: the model (0x7C), an
//! expansion box (0x7E) and flag bits for optional inputs (lambda, a second
//! temperature, the channels expansion). Each channel samples at a fixed rate
//! and their samples are interleaved over a one-second cycle of 500 slots,
//! the way RaceStudio 2 lays them out: channels in number order, each taking
//! the first free slot and then one every 500/rate slots. Samples are u16
//! words; words from `0xD000` to `0xDEFF` are markers: `0xD0xx` starts a
//! cycle, `0xD2xx` a 6-word lap record, `0xD1xx` (4 words), `0xD3xx` and
//! `0xD4xx` (9 words, a CAN device announcing itself) other records.
//! The logger's clock drifts, so times are scaled so the samples span the laps.

use crate::sensors::{is_error, rate_hz, scale};
use crate::{Channel, Error, Lap, Result, Session, SessionInfo};

/// Slots in one second of samples.
const CYCLE: usize = 500;

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

/// A channel a run may hold: its number, rate code, sensor code and name.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Def {
    num: u8,
    freq: u8,
    sensor: u16,
    name: &'static str,
}

const fn def(num: u8, freq: u8, sensor: u16, name: &'static str) -> Def {
    Def { num, freq, sensor, name }
}

/// RaceStudio 2's default channels for each MyChron 4 model and expansion.
fn defs(code: u16) -> &'static [Def] {
    const M4: [Def; 7] = [
        def(0, 2, 0, "Engine"),
        def(1, 2, 96, "Temperature"),
        def(2, 0, 45, "Datalogger_Temp"),
        def(3, 0, 68, "Battery"),
        def(13, 2, 501, "Lambda"),
        def(15, 2, 91, "Lambda_Temp"),
        def(20, 2, 77, "SHAFT"),
    ];
    const M4_2T: [Def; 8] = [
        def(0, 2, 0, "Engine"),
        def(1, 2, 96, "Temp_1"),
        def(2, 0, 45, "Datalogger_Temp"),
        def(3, 0, 68, "Battery"),
        def(13, 2, 501, "Lambda"),
        def(15, 2, 91, "Lambda_Temp"),
        def(19, 2, 96, "Temp_2"),
        def(20, 2, 77, "SHAFT"),
    ];
    // Expansion boxes: Gold (102) and Extreme (103).
    const GOLD: [Def; 5] = [
        def(4, 2, 1, "Speed"),
        def(5, 2, 600, "Calculated_Gear"),
        def(6, 2, 405, "LatAcc"),
        def(7, 2, 96, "EBoxTemp1"),
        def(8, 2, 96, "EBoxTemp2"),
    ];
    const EXTREME: [Def; 8] = [
        def(4, 2, 1, "Speed"),
        def(5, 2, 600, "Calculated_Gear"),
        def(6, 2, 405, "LatAcc"),
        def(7, 2, 96, "EBoxTemp1"),
        def(8, 2, 96, "EBoxTemp2"),
        def(10, 2, 600, "EboxAnalog1"),
        def(11, 2, 600, "EboxAnalog2"),
        def(12, 2, 600, "EboxAnalog3"),
    ];
    // The channels expansion.
    const CHANNELS: [Def; 4] = [def(96, 2, 600, "M4ChExp1"), def(97, 2, 600, "M4ChExp2"), def(98, 2, 600, "M4ChExp3"), def(99, 2, 600, "M4ChExp4")];
    // The 660 (drag racing): 50 Hz, exhaust and head temperatures; the
    // "Europe" version (20001) has a speed input where the 660 has SHAFT.
    const M4_660: [Def; 8] = [
        def(0, 4, 0, "RPM"),
        def(1, 4, 96, "EGT"),
        def(2, 0, 45, "LoggerT"),
        def(3, 0, 68, "Battery"),
        def(13, 4, 501, "Lambda"),
        def(15, 4, 91, "LambdaT"),
        def(19, 4, 96, "CHT"),
        def(20, 4, 77, "SHAFT"),
    ];
    const M4_660_EUROPE: [Def; 8] = [
        def(0, 4, 0, "RPM"),
        def(1, 4, 96, "TEMP1"),
        def(2, 0, 45, "LoggerT"),
        def(3, 0, 68, "Battery"),
        def(13, 4, 501, "Lambda"),
        def(15, 4, 91, "LambdaT"),
        def(19, 4, 96, "TEMP2"),
        def(20, 4, 1, "SpdM4"),
    ];
    match code {
        59 | 60 => &M4_660,
        20001 => &M4_660_EUROPE,
        101 | 106 => &M4,
        104 | 107 => &M4_2T,
        102 => &GOLD,
        103 => &EXTREME,
        108 => &CHANNELS,
        _ => &[],
    }
}

/// The channels this run recorded, from its header.
fn recorded(h: &[u8]) -> Vec<Def> {
    let b = |o: usize| h.get(o).copied().unwrap_or(0);
    let model = u16_at(h, 0x7C).unwrap_or(106);
    let expansion = u16_at(h, 0x7E).filter(|&e| e != 0xFFFF).unwrap_or(0);
    // Firmware version, as RaceStudio 2 compares it (bytes swapped).
    let firmware = u16::from_be_bytes([b(0x76), b(0x77)]);
    let flags16 = u16::from_le_bytes([b(0x0E), b(0x0F)]);
    let on = |num: u8| -> bool {
        if firmware < 0x3125 {
            return num <= 3;
        }
        match num {
            0 | 2 | 3 => true,
            1 => b(0x0E) & 2 != 0,
            19 => matches!(model, 59 | 60 | 104 | 107) && b(0x0F) & 1 != 0,
            20 => matches!(model, 59 | 60) || (matches!(model, 101 | 104 | 106 | 107) && flags16 & 0x200 != 0),
            13 | 15 => (firmware > 0x3126 || matches!(model, 59 | 60)) && b(0x10) & 2 != 0,
            4 => expansion > 101 && b(0x0E) & 1 != 0,
            5 => expansion > 101 && b(0x0E) & 0x80 != 0,
            6 => expansion > 101,
            7 => expansion > 101 && b(0x0E) & 4 != 0,
            8 => expansion > 101 && b(0x0E) & 8 != 0,
            10 => expansion > 101 && b(0x0E) & 0x10 != 0,
            11 => expansion > 101 && b(0x0E) & 0x20 != 0,
            12 => expansion > 101 && b(0x0E) & 0x40 != 0,
            96..=99 => b(0x10) & 0x10 != 0 && b(0xC7) & (1 << (num - 96)) != 0,
            _ => false,
        }
    };
    // A 660 on newer firmware without bit 0 of byte 0x11 is the Europe version.
    let base = if matches!(model, 59 | 60) && firmware > 0x4203 && b(0x11) & 1 == 0 { 20001 } else { model };
    let mut codes = vec![base, expansion];
    if b(0x10) & 0x10 != 0 {
        codes.push(108);
    }
    let mut v: Vec<Def> = codes.iter().flat_map(|&c| defs(c).iter().copied()).filter(|d| on(d.num)).collect();
    v.sort_by_key(|d| d.num);
    v.dedup_by_key(|d| d.num);
    v
}

/// The order samples arrive in over one cycle: indexes into `chans`.
/// RaceStudio 2's placement: each channel in turn takes the first free slot
/// and then one every `CYCLE / rate` slots, moving to the nearer free slot
/// when one is taken.
fn cycle_order(chans: &[Def]) -> Vec<usize> {
    let mut slots: Vec<Option<usize>> = vec![None; CYCLE];
    let mut placed = vec![0u32; chans.len()];
    let per_ms = CYCLE as f32 / 1000.0;
    for (k, c) in chans.iter().enumerate() {
        let Some(hz) = rate_hz(c.freq) else { continue };
        let step = (1000 / hz) as f32 * per_ms;
        let Some(first) = slots.iter().position(|s| s.is_none()) else { break };
        slots[first] = Some(k);
        placed[k] = 1;
        let mut pos = first as f32;
        for _ in 1..hz {
            pos += step;
            if pos >= CYCLE as f32 {
                break;
            }
            let at = pos as usize;
            if slots[at].is_none() {
                slots[at] = Some(k);
                placed[k] += 1;
                continue;
            }
            let ahead = (at..((pos + step) as usize).min(CYCLE)).find(|&i| slots[i].is_none());
            let lo = ((pos - step) as i64).max(0) as usize;
            let behind = (lo + 1..=at).rev().find(|&i| slots[i].is_none());
            let pick = match (behind, ahead) {
                (Some(b), Some(a)) => Some(if at - b < a - at { b } else { a }),
                (b, a) => b.or(a),
            };
            if let Some(i) = pick {
                slots[i] = Some(k);
                placed[k] += 1;
            }
        }
    }
    // Then any samples still unplaced go in the first free slots.
    for (k, c) in chans.iter().enumerate() {
        for _ in placed[k]..rate_hz(c.freq).unwrap_or(0) {
            if let Some(i) = slots.iter().position(|s| s.is_none()) {
                slots[i] = Some(k);
            }
        }
    }
    slots.into_iter().flatten().collect()
}

/// Each recorded channel's raw samples.
struct Samples {
    chans: Vec<Def>,
    raw: Vec<Vec<u16>>,
}

/// Splits the words into channels by the cycle order; Err when the cycle
/// markers don't fall where that order puts them (a layout this doesn't know).
fn samples(h: &[u8], data: &[u8]) -> Result<Samples> {
    let chans = recorded(h);
    let order = cycle_order(&chans);
    if order.is_empty() {
        return Err(Error::Parse("this MyChron 4 run lists no channels Mychronix knows".into()));
    }
    let w: Vec<u16> = data.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
    let mut raw = vec![Vec::new(); chans.len()];
    let (mut i, mut slot) = (0usize, 0usize);
    let (mut markers, mut misplaced) = (0usize, 0usize);
    while i < w.len() {
        // The rest is erased memory.
        if w[i] == 0xFFFF && w[i..(i + 128).min(w.len())].iter().all(|&x| x == 0xFFFF) {
            break;
        }
        match w[i] >> 8 {
            // A cycle starts.
            0xD0 => {
                markers += 1;
                if slot != 0 {
                    misplaced += 1;
                }
                slot = 0;
                i += 1;
            }
            0xD1 => i += 4,
            // Lap record: marker, lap number, lap time in hundredths (u32), two more words.
            0xD2 => i += 6,
            0xD3 => {
                i += if w.get(i + 7) == Some(&0xD300) {
                    8
                } else {
                    (1..7).find(|&k| w.get(i + k) == Some(&0xD203)).unwrap_or(1)
                };
            }
            // A plug-and-play device announcing itself (9 words): RaceStudio 2
            // registers it but keeps reading the same layout.
            0xD4 => i += 9,
            0xD5..=0xDE => i += 1,
            _ => {
                raw[order[slot]].push(w[i]);
                slot = (slot + 1) % order.len();
                i += 1;
            }
        }
    }
    if misplaced > 2 && misplaced * 10 > markers {
        return Err(Error::Parse(
            "this MyChron 4's samples aren't laid out the way its settings say. Its RaceStudio 2 .drk files open; please send this .mc4 so it can be supported.".into(),
        ));
    }
    Ok(Samples { chans, raw })
}

pub fn read(file: &[u8]) -> Result<Session> {
    from_parts(parts(file)?)
}

/// A run from RaceStudio 2's raw copy of a key download (`.imd`, kept next
/// to the `.drk` it made): "IMD", then the run's own header at 0x14000 (the
/// key's other headers follow it), its lap records at 0x20000 and its
/// samples at 0x50000.
pub fn read_imd(file: &[u8]) -> Result<Session> {
    let bad = || Error::Parse("not a RaceStudio 2 MyChron 4 download (.imd)".into());
    if file.get(..3) != Some(b"IMD") {
        return Err(bad());
    }
    let header = file.get(0x14000..0x14100).ok_or_else(bad)?;
    // Lap records run from the lap address to the data address (+136, +140).
    let (laps_at, data_at) = (u32_at(header, 136).ok_or_else(bad)?, u32_at(header, 140).ok_or_else(bad)?);
    let lap_bytes = data_at.checked_sub(laps_at).filter(|&n| n % 64 == 0 && n <= 0x30000).ok_or_else(bad)? as usize;
    let laps = file.get(0x20000..0x20000 + lap_bytes).ok_or_else(bad)?;
    let data_len = u32_at(header, 150).ok_or_else(bad)? as usize;
    let data = file.get(0x50000..).ok_or_else(bad)?;
    let data = &data[..data_len.min(data.len())];
    from_parts(Parts { header, laps, data })
}

fn from_parts(p: Parts<'_>) -> Result<Session> {
    let h = p.header;

    // Laps: one 64-byte record each, time in hundredths at +4.
    let mut laps = Vec::new();
    let mut t = 0i64;
    for (k, rec) in p.laps.as_chunks::<64>().0.iter().enumerate() {
        let Some(cs) = u32_at(rec, 4).filter(|&c| c > 0 && c < 0x7FFF_FFFF) else { continue };
        let ms = cs as i64 * 10;
        laps.push(Lap { number: k as i32 + 1, start_ms: t, end_ms: t + ms });
        t += ms;
    }

    let s = samples(h, p.data)?;
    // Nominal sample times, scaled to the laps when they cover the run.
    let span = s.chans.iter().zip(&s.raw).filter_map(|(c, r)| Some(r.len() as f64 * 1000.0 / rate_hz(c.freq)? as f64)).fold(0.0, f64::max);
    let stretch = if span > 0.0 && t > 0 { (t as f64 / span).clamp(0.95, 1.05) } else { 1.0 };
    let channels: Vec<Channel> = s
        .chans
        .iter()
        .zip(&s.raw)
        .filter_map(|(c, raw)| {
            let sc = scale(c.sensor, None)?;
            let step = 1000.0 / rate_hz(c.freq)? as f64;
            let unplugged = raw.iter().filter(|&&r| is_error(r)).count() * 10 >= raw.len() * 9;
            let (times_ms, values) = raw
                .iter()
                .enumerate()
                .filter(|(_, r)| !unplugged && !is_error(**r))
                .map(|(i, &r)| ((i as f64 * step * stretch).round() as i64, sc.apply(r)))
                .unzip();
            Some(Channel { name: c.name.into(), group: None, units: sc.units.into(), decimals: sc.decimals, interpolate: true, times_ms, values })
        })
        .collect();

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
        logger_model: u16_at(h, 0x7C).and_then(crate::mychron_model).map(str::to_string),
        vehicle: None,
        imu_mount: None,
        laps_from_gps: None,
    };
    if channels.iter().all(|c| c.values.is_empty()) && laps.is_empty() {
        return Err(Error::Parse("the downloaded run holds no data".into()));
    }
    Ok(Session { info, channels, laps, gps: None })
}

/// The list-view summary of a `.mc4` or `.imd`.
pub fn summary(path: &std::path::Path) -> Result<crate::summary::SessionSummary> {
    let bytes = std::fs::read(path)?;
    let s = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("imd")) { read_imd(&bytes)? } else { read(&bytes)? };
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
        model: s.info.logger_model.clone(),
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
        let mut h = vec![0u8; 256];
        h[0..2].copy_from_slice(&7u16.to_le_bytes());
        h[0x0E] = 0x02; // temperature input on
        h[0x76..0x78].copy_from_slice(&[0x59, 0x32]); // firmware
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
            w.extend([5000 + f, 0xCFFF]);
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
        assert_eq!(s.channels.len(), 4);
        assert_eq!(s.info.date.as_deref(), Some("09/19/2026"));
        assert_eq!(s.info.time.as_deref(), Some("07:31:51"));
        assert_eq!((s.info.logger_id, s.info.driver.as_deref()), (Some(123456), Some("ANN")));
        assert_eq!(s.laps[0].duration_ms(), 2000);
    }

    fn header(model: u16, expansion: u16, e: u8, f: u8, g: u8, c7: u8) -> Vec<u8> {
        let mut h = vec![0u8; 256];
        h[0x0E] = e;
        h[0x0F] = f;
        h[0x10] = g;
        h[0xC7] = c7;
        h[0x76..0x78].copy_from_slice(&[0x59, 0x32]);
        h[0x7C..0x7E].copy_from_slice(&model.to_le_bytes());
        h[0x7E..0x80].copy_from_slice(&expansion.to_le_bytes());
        h
    }

    fn names(h: &[u8]) -> Vec<&'static str> {
        let chans = recorded(h);
        cycle_order(&chans).into_iter().map(|k| chans[k].name).collect()
    }

    #[test]
    fn plain_mychron_4_cycle_matches_the_logger() {
        // RPM, temperature, logger temperature, battery, then nine RPM/temperature pairs.
        let n = names(&header(106, 0, 0x02, 0, 0, 0));
        assert_eq!(n.len(), 22);
        assert_eq!(&n[..6], &["Engine", "Temperature", "Datalogger_Temp", "Battery", "Engine", "Temperature"]);
        assert!(n[4..].chunks(2).all(|p| p == ["Engine", "Temperature"]));
    }

    #[test]
    fn options_and_expansions_add_their_channels() {
        // 2T with a second temperature, lambda and the channels expansion (inputs 1 and 3).
        let chans = recorded(&header(107, 0, 0x02, 0x01, 0x12, 0b0101));
        let got: Vec<_> = chans.iter().map(|c| c.name).collect();
        assert_eq!(got, ["Engine", "Temp_1", "Datalogger_Temp", "Battery", "Lambda", "Lambda_Temp", "Temp_2", "M4ChExp1", "M4ChExp3"]);
        // 7 channels at 10 Hz and 2 at 1 Hz: 72 samples a second, slow ones in the first frame.
        let n = names(&header(107, 0, 0x02, 0x01, 0x12, 0b0101));
        assert_eq!(n.len(), 72);
        assert_eq!(&n[..9], &["Engine", "Temp_1", "Datalogger_Temp", "Battery", "Lambda", "Lambda_Temp", "Temp_2", "M4ChExp1", "M4ChExp3"]);
        // Extreme expansion box: speed, gear, lateral g and the box temperatures.
        let got: Vec<_> = recorded(&header(106, 103, 0x8F, 0, 0, 0)).iter().map(|c| c.name).collect();
        assert_eq!(got, ["Engine", "Temperature", "Datalogger_Temp", "Battery", "Speed", "Calculated_Gear", "LatAcc", "EBoxTemp1", "EBoxTemp2"]);
    }

    #[test]
    fn decodes_an_expanded_logger() {
        let h = header(106, 102, 0x03, 0, 0, 0); // temperature + speed, lateral g always
        let chans = recorded(&h);
        let order = cycle_order(&chans);
        let mut w: Vec<u16> = Vec::new();
        for cycle in 0..3u16 {
            if cycle > 0 {
                w.push(0xD000);
            }
            for &k in &order {
                w.push(match chans[k].name {
                    "Engine" => 8000 + cycle,
                    "Speed" => 650,
                    "LatAcc" => (-120i16) as u16,
                    "Temperature" => 905,
                    _ => 2400,
                });
            }
        }
        let data: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        let s = read(&file(&h, &[], &data)).unwrap();
        assert_eq!(s.channel("Engine").unwrap().values.len(), 30);
        assert_eq!(s.channel("Engine").unwrap().values[25], 8002.0);
        assert!((s.channel("Speed").unwrap().values[0] - 65.0).abs() < 1e-4);
        assert!((s.channel("LatAcc").unwrap().values[3] + 1.2).abs() < 1e-6);
        assert!((s.channel("Temperature").unwrap().values[0] - 90.5).abs() < 1e-4);
        assert_eq!(s.channel("Battery").unwrap().values.len(), 3);
    }

    #[test]
    fn device_announcements_dont_stop_the_run() {
        // A CAN device announcing itself (9-word 0xD4 record) at the start and mid-run.
        let h = header(106, 0, 0x02, 0, 0, 0);
        let chans = recorded(&h);
        let order = cycle_order(&chans);
        let announce = [0xD401u16, 0x0B00, 0x1234, 0x5678, 0x9ABC, 0x0001, 0x0002, 0x0003, 0x0004];
        let mut w: Vec<u16> = announce.to_vec();
        for cycle in 0..4u16 {
            if cycle > 0 {
                w.push(0xD000);
            }
            if cycle == 2 {
                w.extend(announce);
            }
            w.extend(order.iter().map(|&k| if chans[k].name == "Engine" { 7000 + cycle } else { 2400 }));
        }
        let data: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        let s = read(&file(&h, &[], &data)).unwrap();
        let e = &s.channel("Engine").unwrap().values;
        assert_eq!(e.len(), 40);
        assert_eq!((e[0], e[39]), (7000.0, 7003.0));
    }

    #[test]
    fn the_660_samples_at_50_hz() {
        // A 660 with its exhaust and head temperatures on.
        let mut h = header(59, 0, 0x02, 0x01, 0, 0);
        h[0x76..0x78].copy_from_slice(&[0x40, 0x00]); // older firmware: not the Europe version
        let n = names(&h);
        assert_eq!(n.iter().filter(|&&c| c == "RPM").count(), 50);
        assert_eq!(n.iter().filter(|&&c| c == "LoggerT").count(), 1);
        assert_eq!(&n[..6], &["RPM", "EGT", "LoggerT", "Battery", "CHT", "SHAFT"]);
        // Newer firmware: the Europe version, with speed instead of SHAFT.
        let mut h = header(60, 0, 0x02, 0x01, 0, 0);
        h[0x76..0x78].copy_from_slice(&[0x59, 0x32]);
        assert!(names(&h).contains(&"SpdM4"));
    }
}
