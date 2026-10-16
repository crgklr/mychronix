//! Session summaries the logger keeps beside each recording.
//!
//! - `a_NNNN.xri`: one session, as text records framed like the binary ones
//!   but with decimal fields: `<hTAG` + 10-digit length + flag + `>`, a body of
//!   `key=value|` lines, then `<TAG` + 6-digit decimal byte sum + `>`.
//! - `summary1.csv`: every session on the logger in one file, newest first.
//!
//! Both carry what a session list needs (date, track, laps, best lap) without
//! downloading the recordings. The logger leaves driver and vehicle empty;
//! RaceStudio 3 adds those on the PC.

use std::collections::HashMap;
use std::fmt;

use crate::Error;

/// A calendar date. Logger files write it day-first (`05/10/2026` is 5 October).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    fn parse_day_first(s: &str) -> Option<Self> {
        let mut parts = s.split('/').map(|p| p.trim().parse::<u16>().ok());
        let (day, month, year) = (parts.next()??, parts.next()??, parts.next()??);
        ((1..=12).contains(&month) && (1..=31).contains(&day))
            .then_some(Self { year, month: month as u8, day: day as u8 })
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionSummary {
    /// File name on the logger, e.g. `a_0035.xrz` (from the CSV; the `.xri`
    /// itself doesn't name its recording).
    pub file: Option<String>,
    pub number: Option<u32>,
    pub date: Option<Date>,
    /// Local start time as the logger records it, `HH:MM:SS`.
    pub start_time: Option<String>,
    /// Track name as the logger matched it (an AiM or custom track).
    pub track: Option<String>,
    /// The matched track's id, shared with its `.ria` record and `.tkk` file.
    pub track_guid: Option<String>,
    /// Start/finish line position, in degrees.
    pub start_finish: Option<(f64, f64)>,
    pub duration_ms: Option<u64>,
    pub laps: Option<u32>,
    pub best_lap_ms: Option<u64>,
    pub best_lap_number: Option<u32>,
    pub max_rpm: Option<u32>,
    pub max_speed: Option<f64>,
    pub max_lateral_g: Option<f64>,
    /// The logger's model, e.g. "MyChron 4", when the file says.
    pub model: Option<String>,
    pub driver: Option<String>,
    pub vehicle: Option<String>,
    pub championship: Option<String>,
}

impl SessionSummary {
    /// True when the logger matched the session to a track.
    pub fn has_track(&self) -> bool {
        self.track.as_deref().is_some_and(|t| !t.is_empty())
    }

    fn from_fields(f: &HashMap<String, String>) -> Self {
        let text = |k: &str| f.get(k).map(|v| v.trim()).filter(|v| !v.is_empty()).map(str::to_string);
        let num = |k: &str| text(k).and_then(|v| v.parse::<u64>().ok());
        let float = |k: &str| text(k).and_then(|v| v.parse::<f64>().ok());
        let coord = |k: &str| text(k).and_then(|v| v.parse::<i64>().ok()).filter(|&v| v != 0).map(|v| v as f64 / 1e7);
        let start_finish = coord("track_lat_0")
            .or_else(|| coord("track_lat"))
            .zip(coord("track_lon_0").or_else(|| coord("track_lon")));
        Self {
            file: text("name"),
            number: num("sess_num").map(|n| n as u32),
            date: text("date").as_deref().and_then(Date::parse_day_first),
            start_time: text("hour"),
            track: text("track_name"),
            track_guid: text("track_guid"),
            start_finish,
            duration_ms: num("test_dur"),
            laps: num("nlap").map(|n| n as u32),
            best_lap_ms: num("best"),
            best_lap_number: num("nbest").map(|n| n as u32),
            max_rpm: num("maxrpm").map(|n| n as u32),
            max_speed: float("maxvel"),
            max_lateral_g: float("maxlacc"),
            model: None,
            driver: text("pilota"),
            vehicle: text("veicolo"),
            championship: text("campionato"),
        }
    }
}

/// One text record from an `.xri`-style file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRecord {
    pub tag: String,
    pub fields: Vec<(String, String)>,
}

/// Splits an `.xri`-style buffer into records, verifying each checksum.
pub fn parse_text_records(buf: &[u8]) -> Result<Vec<TextRecord>, Error> {
    let bad = |what: &str, at: usize| Error::Parse(format!("{what} at byte {at}"));
    let mut records = Vec::new();
    let mut pos = 0;
    while let Some(rel) = find(&buf[pos..], b"<h") {
        let start = pos + rel;
        // `<h` TAG(4) LEN(10 digits) FLAG(1) `>`
        let header = buf.get(start..start + 18).ok_or_else(|| bad("truncated record header", start))?;
        if header[17] != b'>' {
            return Err(bad("malformed record header", start));
        }
        let tag = String::from_utf8_lossy(&header[2..6]).trim_end().to_string();
        let len: usize = std::str::from_utf8(&header[6..16])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("bad record length", start))?;
        let body_start = start + 18;
        if len == 0 {
            // A container (IMEM wraps the whole file); its trailer comes at the very end.
            pos = body_start;
            continue;
        }
        let body = buf.get(body_start..body_start + len).ok_or_else(|| bad("truncated record body", start))?;

        // Trailer: optional CR/LF, then `<` TAG(4) SUM(6 digits) `>`.
        let mut t = body_start + len;
        while matches!(buf.get(t), Some(b'\r' | b'\n')) {
            t += 1;
        }
        let trailer = buf.get(t..t + 12).ok_or_else(|| bad("truncated record trailer", t))?;
        if trailer[0] != b'<' || trailer[11] != b'>' || &trailer[1..5] != &header[2..6] {
            return Err(bad("malformed record trailer", t));
        }
        let sum: u64 = std::str::from_utf8(&trailer[5..11])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("bad record checksum", t))?;
        let actual: u64 = body.iter().map(|&b| b as u64).sum();
        if sum != actual {
            return Err(Error::Parse(format!("{tag} record checksum {sum} != {actual}")));
        }

        let fields = String::from_utf8_lossy(body)
            .split('|')
            .filter_map(|line| {
                let line = line.trim_matches(|c| c == '\r' || c == '\n');
                line.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
        records.push(TextRecord { tag, fields });
        pos = t + 12;
    }
    Ok(records)
}

/// Reads a session's `.xri` summary.
pub fn parse_xri(buf: &[u8]) -> Result<SessionSummary, Error> {
    let fields: HashMap<String, String> =
        parse_text_records(buf)?.into_iter().flat_map(|r| r.fields).collect();
    if fields.is_empty() {
        return Err(Error::Parse("no fields in .xri".into()));
    }
    Ok(SessionSummary::from_fields(&fields))
}

/// Reads the logger's `summary1.csv`: one row per session, newest first.
///
/// The CSV's `maxvel` column is not a plain speed (it doesn't match the
/// `.xri`), so `max_speed` is left empty here.
pub fn parse_summary_csv(text: &str) -> Vec<SessionSummary> {
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return Vec::new() };
    let columns: Vec<&str> = header.split(',').map(str::trim).collect();
    lines
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let fields: HashMap<String, String> = columns
                .iter()
                .zip(line.split(','))
                .filter(|(c, _)| !c.is_empty() && **c != "maxvel")
                .map(|(c, v)| (c.to_string(), v.to_string()))
                .collect();
            let mut s = SessionSummary::from_fields(&fields);
            s.number = s.number.or_else(|| session_number(s.file.as_deref()?));
            s
        })
        .collect()
}

/// `a_0035.xrz` → 35.
pub fn session_number(file: &str) -> Option<u32> {
    let stem = file.rsplit('/').next()?.split('.').next()?;
    // MyChron 5/6: "a_0035" (or "a_0035-2", a second recording with that
    // name after the logger was formatted); RaceStudio 2: "<name> <date>_003".
    let n = stem.strip_prefix("a_").or_else(|| stem.rsplit_once('_').map(|(_, n)| n))?;
    n.split('-').next()?.parse().ok()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an `.xri` the way the logger writes one (CRLF lines, decimal sums).
    fn record(tag: &str, fields: &[(&str, &str)]) -> String {
        let body: String = fields.iter().map(|(k, v)| format!("\r\n{k}={v}|")).collect();
        let sum: u32 = body.bytes().map(u32::from).sum();
        format!("<h{tag:<4}{:010}a>{body}\r\n<{tag:<4}{sum:06}>", body.len())
    }

    fn sample_xri() -> Vec<u8> {
        let mut s = String::from("<hIMEM0000000000a>\r\n");
        s += &record("USR", &[("pilota", ""), ("veicolo", "")]);
        s += &record("TINF", &[("sess_num", "32"), ("date", "05/10/2026"), ("hour", "12:40:53")]);
        s += &record("TRK", &[("track_name", "Test Ring"), ("track_lat_0", "451234567"), ("track_lon_0", "-91234567")]);
        s += &record("TDUR", &[("test_dur", "505695")]);
        s += &record("LAPS", &[("nlap", "13"), ("maxrpm", "9768"), ("maxvel", "57.6"), ("maxlacc", "1.47"), ("nbest", "8"), ("best", "31239")]);
        s += "\r\n<IMEM000000>";
        s.into_bytes()
    }

    #[test]
    fn reads_xri() {
        let s = parse_xri(&sample_xri()).unwrap();
        assert_eq!(s.number, Some(32));
        assert_eq!(s.date, Some(Date { year: 2026, month: 10, day: 5 }));
        assert_eq!(s.start_time.as_deref(), Some("12:40:53"));
        assert_eq!(s.track.as_deref(), Some("Test Ring"));
        assert_eq!(s.start_finish, Some((45.1234567, -9.1234567)));
        assert_eq!((s.laps, s.best_lap_ms, s.best_lap_number), (Some(13), Some(31239), Some(8)));
        assert_eq!((s.max_rpm, s.max_speed, s.max_lateral_g), (Some(9768), Some(57.6), Some(1.47)));
        assert_eq!((s.driver.clone(), s.vehicle.clone()), (None, None));
        assert!(s.has_track());
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut xri = sample_xri();
        let i = xri.windows(5).position(|w| w == b"13|\r\n").unwrap();
        xri[i] = b'2';
        assert!(parse_xri(&xri).is_err());
    }

    #[test]
    fn reads_summary_csv() {
        let csv = "name,size,date,hour,nlap,nbest,best,pilota,track_name,maxvel,track_lat,track_lon,test_dur,\n\
                   a_0035.xrz,2236455,10/10/2026,14:19:23,15,10,30480,,Test Ring,1078807756,451234567,-91234567,736819,\n\
                   a_0025.xrz,318069,05/10/2026,18:47:02,1,,,,,1079051878,0,0,107479,\n";
        let rows = parse_summary_csv(csv);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].number, Some(35));
        assert_eq!(rows[0].best_lap_ms, Some(30480));
        assert_eq!(rows[0].max_speed, None);
        assert_eq!(rows[1].date, Some(Date { year: 2026, month: 10, day: 5 }));
        assert!(!rows[1].has_track());
        assert_eq!(rows[1].start_finish, None);
    }
}
