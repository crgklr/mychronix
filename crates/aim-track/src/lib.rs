//! AiM track files.
//!
//! - `.ria`: a track database, 128 bytes per track. RaceStudio 3 keeps the
//!   full AiM list in `aim_tracks/aim_tracks.ria`; a MyChron keeps its own copy
//!   in `0:/tkk/al.ria` (alphabetical; `ll.ria` holds the same records sorted
//!   by position) and its custom tracks in `0:/tkk/dev.ria`. Records give each
//!   track's name, country, start/finish position and ids, but not its shape.
//! - `.tkk`: one custom track, framed like the logger's other files. It adds
//!   the track outline.
//!
//! Positions are degrees; the files store them as i32 × 1e-7.

use std::path::Path;

use aim_link::frame;

pub const RIA_RECORD: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Format(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    fn from_e7(lat: i32, lon: i32) -> Option<Self> {
        (lat != 0 || lon != 0).then_some(Self { lat: lat as f64 * 1e-7, lon: lon as f64 * 1e-7 })
    }

    /// Great-circle distance in meters.
    pub fn distance_m(&self, other: &LatLon) -> f64 {
        let (p1, p2) = (self.lat.to_radians(), other.lat.to_radians());
        let dp = p2 - p1;
        let dl = (other.lon - self.lon).to_radians();
        let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
        6_371_000.0 * 2.0 * a.sqrt().asin()
    }
}

/// One track from a `.ria` database.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRecord {
    pub name: String,
    /// ISO country code, e.g. `US`.
    pub country: String,
    /// State or region code where the database has one, e.g. `CT`.
    pub region: String,
    /// The track's center: where a logger looks to tell which track it's at.
    /// Not the start/finish line, which only the track's `.tkk` records.
    pub center: Option<LatLon>,
    pub length_m: Option<f32>,
    /// Eight-character id, also the track's `.tkk` file name.
    pub id: String,
    /// Shared with session summaries (`track_guid`), as 16 hex digits.
    pub guid: String,
}

/// Parses a whole `.ria` database.
pub fn parse_ria(buf: &[u8]) -> Result<Vec<TrackRecord>, Error> {
    if !buf.len().is_multiple_of(RIA_RECORD) {
        return Err(Error::Format(format!("{} bytes is not a whole number of {RIA_RECORD}-byte records", buf.len())));
    }
    Ok(buf.as_chunks::<RIA_RECORD>().0.iter().filter_map(|r| parse_ria_record(r)).collect())
}

pub fn read_ria(path: impl AsRef<Path>) -> Result<Vec<TrackRecord>, Error> {
    parse_ria(&std::fs::read(path)?)
}


fn parse_ria_record(r: &[u8]) -> Option<TrackRecord> {
    let name = c_str(&r[0..28]);
    if name.is_empty() {
        return None;
    }
    let i32_at = |o: usize| i32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    Some(TrackRecord {
        name,
        country: c_str(&r[28..32]),
        region: c_str(&r[32..36]),
        center: LatLon::from_e7(i32_at(36), i32_at(40)),
        length_m: Some(f32::from_le_bytes(r[48..52].try_into().unwrap())).filter(|l| l.is_finite() && *l > 0.0),
        id: c_str(&r[56..64]),
        guid: hex(&r[64..72]),
    })
}

/// A track file (`.tkk`): one per AiM track in RaceStudio's `aim_tracks/`,
/// and one per custom track.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackFile {
    /// Short name as the logger shows it, e.g. `NKA`.
    pub name: String,
    /// Full venue name, e.g. `Norwalk Karting Association`.
    pub full_name: String,
    pub id: String,
    pub guid: Option<String>,
    pub country: String,
    pub region: String,
    /// The track's center (the point its `.ria` record carries). A track
    /// the MyChron made puts it on the start/finish; AiM's and RaceStudio's
    /// put it mid-track.
    pub center: Option<LatLon>,
    /// Where laps start and end (as the logger records in each session).
    pub start_finish: Option<LatLon>,
    pub length_m: Option<f32>,
    pub kind: TrackKind,
    /// Surface labels, e.g. `["Paved"]`.
    pub surface: Vec<&'static str>,
    /// The track's center line.
    pub outline: Vec<LatLon>,
    /// Elevation of each outline point in meters, when the file has it.
    pub elevation_m: Vec<f32>,
    /// IANA zone, e.g. `America/New_York`.
    pub timezone: Option<String>,
    pub info: TrackInfo,
    /// The venue logo as PNG bytes.
    pub logo_png: Option<Vec<u8>>,
}

/// Contact details AiM keeps for a venue (the file's `plus` record).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackInfo {
    pub city: Option<String>,
    pub address: Option<String>,
    pub postcode: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub comment: Option<String>,
    /// Every field as stored, including ones not named above.
    pub fields: Vec<(String, String)>,
}

/// Track type codes, as far as they could be matched to RaceStudio's labels
/// ("Race Track, Kart Track, Oval, Drag Strip, Rally, Hill Climb, Motocross,
/// ROval, Off Road, Drifting") by looking at which venues carry each code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackKind {
    RaceTrack,
    KartTrack,
    Oval,
    DragStrip,
    HillClimb,
    Motocross,
    /// A code whose label hasn't been confirmed.
    Other(u32),
    #[default]
    Unknown,
}

impl TrackKind {
    pub fn from_code(code: u32) -> Self {
        match code {
            0 => Self::Unknown,
            1 => Self::RaceTrack,
            2 => Self::KartTrack,
            3 => Self::Oval,
            5 => Self::DragStrip,
            7 => Self::HillClimb,
            8 => Self::Motocross,
            other => Self::Other(other),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::RaceTrack => "Race track".into(),
            Self::KartTrack => "Kart track".into(),
            Self::Oval => "Oval".into(),
            Self::DragStrip => "Drag strip".into(),
            Self::HillClimb => "Hill climb".into(),
            Self::Motocross => "Motocross".into(),
            Self::Other(code) => format!("Other (type {code})"),
            Self::Unknown => "Unknown".into(),
        }
    }
}

pub fn parse_tkk(buf: &[u8]) -> Result<TrackFile, Error> {
    let frames = frame::parse_all(buf).map_err(|e| Error::Format(e.to_string()))?;
    let find = |tag: &str| frames.iter().find(|f| f.tag_str() == tag).map(|f| f.payload.as_slice());

    let header = find("Ptkk").ok_or_else(|| Error::Format("no Ptkk record".into()))?;
    let mut t = TrackFile {
        full_name: c_str(header.get(4..header.len().saturating_sub(8)).unwrap_or_default()),
        id: c_str(header.get(header.len().saturating_sub(8)..).unwrap_or_default()),
        ..Default::default()
    };

    // Vnfo: name 0..28, country 28..32, region 32..36, length f32 at 44,
    // center at 48..56, type at 72, start/finish at 80..88, guid in the last
    // 8 bytes. The start/finish matches what loggers record in sessions at
    // the track (verified on NKA and on a custom track, Oct 24, 2026).
    if let Some(v) = find("Vnfo") {
        let u32_at = |o: usize| v.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        t.name = c_str(v.get(0..28).unwrap_or_default());
        t.country = c_str(v.get(28..32).unwrap_or_default());
        t.region = c_str(v.get(32..36).unwrap_or_default());
        t.length_m = u32_at(44).map(f32::from_bits).filter(|l| l.is_finite() && *l > 0.0);
        let point = |o: usize| u32_at(o).zip(u32_at(o + 4)).and_then(|(a, b)| LatLon::from_e7(a as i32, b as i32));
        t.center = point(48);
        t.start_finish = point(80);
        t.kind = u32_at(72).map(TrackKind::from_code).unwrap_or_default();
        t.guid = v.len().checked_sub(8).map(|o| hex(&v[o..])).filter(|g| g != "0000000000000000");
    }
    if t.name.is_empty() {
        t.name = t.full_name.clone();
    }

    // 12-byte points: lat, lon (i32 × 1e-7), elevation (i32 millimeters).
    // Some custom tracks leave the elevation bytes uninitialized; drop them
    // unless every value is a believable height.
    if let Some(p) = find("pts") {
        let mut elevation = Vec::new();
        for c in p.as_chunks::<12>().0 {
            let at = |o: usize| i32::from_le_bytes(c[o..o + 4].try_into().unwrap());
            if let Some(pos) = LatLon::from_e7(at(0), at(4)) {
                t.outline.push(pos);
                elevation.push(at(8) as f32 / 1000.0);
            }
        }
        if elevation.iter().all(|e| (-500.0..9000.0).contains(e)) && elevation.iter().any(|e| *e != 0.0) {
            fill_missing_heights(&mut elevation);
            t.elevation_m = elevation;
        }
    }

    // srfs: one bit per surface, in RaceStudio's label order.
    if let Some(bits) = find("srfs").and_then(|b| b.get(0..4)).map(|b| u32::from_le_bytes(b.try_into().unwrap())) {
        const SURFACES: [&str; 5] = ["Paved", "Dirt", "Ice", "Water", "Combo"];
        t.surface = SURFACES.iter().enumerate().filter(|(i, _)| bits & (1 << i) != 0).map(|(_, s)| *s).collect();
    }
    t.timezone = find("zots").map(|z| c_str(z.get(0..64).unwrap_or(z))).filter(|z| !z.is_empty());
    // lgo: "<id>.png\0" followed by the PNG.
    t.logo_png = find("lgo").and_then(|l| {
        let at = l.windows(4).position(|w| w == b"\x89PNG")?;
        Some(l[at..].to_vec())
    });
    if let Some(xml) = find("plus") {
        t.info = parse_plus(&String::from_utf8_lossy(xml));
    }
    Ok(t)
}

/// AiM writes 0 where a point has no height (often the first and last points,
/// sometimes runs in between). Fill those from the nearest heights either side.
fn fill_missing_heights(h: &mut [f32]) {
    let known: Vec<usize> = (0..h.len()).filter(|&i| h[i] != 0.0).collect();
    let (Some(&first), Some(&last)) = (known.first(), known.last()) else { return };
    for i in 0..h.len() {
        if h[i] != 0.0 {
            continue;
        }
        let after = known.partition_point(|&k| k < i);
        h[i] = match (after.checked_sub(1).map(|j| known[j]), known.get(after).copied()) {
            (Some(a), Some(b)) => h[a] + (h[b] - h[a]) * (i - a) as f32 / (b - a) as f32,
            (None, _) => h[first],
            (_, None) => h[last],
        };
    }
}

/// The `plus` record: `<p n="Key">value</p>` pairs in a small XML document.
fn parse_plus(xml: &str) -> TrackInfo {
    let mut info = TrackInfo::default();
    for part in xml.split("<p n=\"").skip(1) {
        let Some((key, rest)) = part.split_once("\">") else { continue };
        let Some((value, _)) = rest.split_once("</p>") else { continue };
        let value = unescape(value.trim());
        if value.is_empty() {
            continue;
        }
        match key {
            "Cty" => info.city = Some(value.clone()),
            "Adr" => info.address = Some(value.clone()),
            "Pco" => info.postcode = Some(value.clone()),
            "Tel" => info.phone = Some(value.clone()),
            "Url" => info.website = Some(value.clone()),
            "Cmt" => info.comment = Some(value.clone()),
            _ => {}
        }
        info.fields.push((key.to_string(), value));
    }
    info
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// A track length the way it's usually quoted: ovals as the nearest
/// standard fraction of a mile; otherwise meters under a kilometer and
/// kilometers above, or in US units feet under a mile and miles above.
pub fn length_label(length_m: f32, kind: TrackKind, us_units: bool) -> String {
    const MILE: f32 = 1609.344;
    const OVALS: [(&str, f32); 21] = [
        ("1/20", 0.05), ("1/10", 0.1), ("1/9", 1.0 / 9.0), ("1/8", 0.125), ("1/7", 1.0 / 7.0), ("1/6", 1.0 / 6.0),
        ("1/5", 0.2), ("1/4", 0.25), ("3/10", 0.3), ("5/16", 0.3125), ("1/3", 1.0 / 3.0), ("3/8", 0.375),
        ("4/10", 0.4), ("1/2", 0.5), ("5/8", 0.625), ("6/10", 0.6), ("7/10", 0.7), ("3/4", 0.75), ("8/10", 0.8),
        ("1", 1.0), ("2", 2.0),
    ];
    if kind == TrackKind::Oval {
        let miles = length_m / MILE;
        if let Some((label, _)) = OVALS
            .iter()
            .map(|(l, m)| (l, (miles - m).abs() / m))
            .filter(|(_, err)| *err < 0.08)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return format!("{label} mi");
        }
    }
    if us_units {
        if length_m < MILE {
            let feet = (length_m * 3.280_84).round() as u32;
            return if feet >= 1000 { format!("{},{:03} ft", feet / 1000, feet % 1000) } else { format!("{feet} ft") };
        }
        return format!("{:.2} mi", length_m / MILE);
    }
    if length_m < 1000.0 { format!("{length_m:.0} m") } else { format!("{:.2} km", length_m / 1000.0) }
}

pub fn read_tkk(path: impl AsRef<Path>) -> Result<TrackFile, Error> {
    parse_tkk(&std::fs::read(path)?)
}

fn c_str(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, country: &str, region: &str, lat: i32, lon: i32, id: &str, guid: u64) -> Vec<u8> {
        let mut r = vec![0u8; RIA_RECORD];
        r[..name.len()].copy_from_slice(name.as_bytes());
        r[28..28 + country.len()].copy_from_slice(country.as_bytes());
        r[32..32 + region.len()].copy_from_slice(region.as_bytes());
        r[36..40].copy_from_slice(&lat.to_le_bytes());
        r[40..44].copy_from_slice(&lon.to_le_bytes());
        r[56..56 + id.len()].copy_from_slice(id.as_bytes());
        r[64..72].copy_from_slice(&guid.to_le_bytes());
        r
    }

    #[test]
    fn parses_ria_records() {
        let mut db = record("10 Mile", "US", "LA", 309_553_461, -928_034_725, "0s5jo9ob", 0x0e44_e32d_0d53);
        db.extend(record("Test Ring", "IT", "", 451_234_567, 91_234_567, "abcd1234", 1));
        let tracks = parse_ria(&db).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].name, "10 Mile");
        assert_eq!((tracks[0].country.as_str(), tracks[0].region.as_str()), ("US", "LA"));
        let c = tracks[0].center.unwrap();
        assert!((c.lat - 30.9553461).abs() < 1e-9 && (c.lon + 92.8034725).abs() < 1e-9);
        assert_eq!(tracks[0].id, "0s5jo9ob");
        assert_eq!(tracks[0].guid, "530d2de3440e0000");
        assert_eq!(tracks[1].region, "");
    }

    #[test]
    fn rejects_partial_records() {
        assert!(parse_ria(&[0u8; 100]).is_err());
    }

    #[test]
    fn parses_tkk() {
        let mut header = vec![0u8; 268];
        header[4..20].copy_from_slice(b"Test Ring Circus");
        header[260..268].copy_from_slice(b"ring0001");
        let mut info = vec![0u8; 476];
        info[0..4].copy_from_slice(b"Ring");
        info[28..30].copy_from_slice(b"IT");
        info[44..48].copy_from_slice(&1234.5f32.to_le_bytes());
        info[72..76].copy_from_slice(&2u32.to_le_bytes());
        info[468..476].copy_from_slice(&0x0001_ae52_e9e5_2ea3u64.to_le_bytes());
        info[48..52].copy_from_slice(&451_234_567i32.to_le_bytes());
        info[52..56].copy_from_slice(&(-91_234_567i32).to_le_bytes());
        info[80..84].copy_from_slice(&451_239_999i32.to_le_bytes());
        info[84..88].copy_from_slice(&(-91_230_000i32).to_le_bytes());
        let mut pts = Vec::new();
        for k in 0..3i32 {
            pts.extend((451_234_567 + k * 100).to_le_bytes());
            pts.extend((-91_234_567 - k * 100).to_le_bytes());
            pts.extend((265_000 + k * 1000).to_le_bytes());
        }
        let mut logo = b"ring0001.png\0".to_vec();
        logo.extend(b"\x89PNGrest");
        let plus = r#"<?xml version="1.0"?><DplRoot><a><p n="Cty">Monza &amp; Co</p><p n="Url">https://example.org/</p><p n="Nmg">X</p></a></DplRoot>"#;
        let mut buf = frame::Frame::new(b"Ptkk", header).encode();
        buf.extend(frame::Frame::new(b"Vnfo", info).encode());
        buf.extend(frame::Frame::new(b"pts\0", pts).encode());
        buf.extend(frame::Frame::new(b"srfs", 0x11u32.to_le_bytes().to_vec()).encode());
        let mut zone = b"Europe/Rome".to_vec();
        zone.resize(408, 0);
        buf.extend(frame::Frame::new(b"zots", zone).encode());
        buf.extend(frame::Frame::new(b"lgo\0", logo).encode());
        buf.extend(frame::Frame::new(b"plus", plus.as_bytes().to_vec()).encode());

        let t = parse_tkk(&buf).unwrap();
        assert_eq!((t.name.as_str(), t.full_name.as_str(), t.id.as_str()), ("Ring", "Test Ring Circus", "ring0001"));
        assert_eq!((t.country.as_str(), t.length_m, t.kind), ("IT", Some(1234.5), TrackKind::KartTrack));
        assert_eq!(t.timezone.as_deref(), Some("Europe/Rome"));
        assert_eq!(t.surface, ["Paved", "Combo"]);
        assert_eq!(t.logo_png.as_deref(), Some(&b"\x89PNGrest"[..]));
        assert_eq!(t.info.city.as_deref(), Some("Monza & Co"));
        assert_eq!(t.info.website.as_deref(), Some("https://example.org/"));
        assert_eq!(t.info.fields.len(), 3);
        assert_eq!(t.outline.len(), 3);
        assert_eq!(t.elevation_m, [265.0, 266.0, 267.0]);

        // A leading 0 is AiM's placeholder, not sea level.
        let mut zero_first = Vec::new();
        for (k, mm) in [0i32, 265_000, 0, 267_000, 0].into_iter().enumerate() {
            zero_first.extend((451_234_567 + k as i32 * 100).to_le_bytes());
            zero_first.extend((-91_234_567i32).to_le_bytes());
            zero_first.extend(mm.to_le_bytes());
        }
        let mut header2 = vec![0u8; 268];
        header2[4..8].copy_from_slice(b"Ring");
        let mut buf2 = frame::Frame::new(b"Ptkk", header2).encode();
        buf2.extend(frame::Frame::new(b"pts\0", zero_first).encode());
        assert_eq!(parse_tkk(&buf2).unwrap().elevation_m, [265.0, 265.0, 266.0, 267.0, 267.0]);
        assert!((t.center.unwrap().lat - 45.1234567).abs() < 1e-9);
        let sf = t.start_finish.unwrap();
        assert!((sf.lat - 45.1239999).abs() < 1e-9 && (sf.lon + 9.123).abs() < 1e-9);
        assert_eq!(t.guid.as_deref(), Some("a32ee5e952ae0100"));
    }

    #[test]
    fn length_labels() {
        assert_eq!(length_label(637.2, TrackKind::KartTrack, false), "637 m");
        assert_eq!(length_label(1750.0, TrackKind::RaceTrack, false), "1.75 km");
        assert_eq!(length_label(603.5, TrackKind::Oval, false), "3/8 mi");
        assert_eq!(length_label(826.9, TrackKind::Oval, true), "1/2 mi");
        assert_eq!(length_label(312.0, TrackKind::KartTrack, true), "1,024 ft");
        assert_eq!(length_label(250.0, TrackKind::KartTrack, true), "820 ft");
        assert_eq!(length_label(6515.0, TrackKind::RaceTrack, true), "4.05 mi");
    }

    #[test]
    fn distance() {
        // One thousandth of a degree of latitude is about 111 m anywhere.
        let a = LatLon { lat: 45.0, lon: 9.0 };
        let b = LatLon { lat: 45.001, lon: 9.0 };
        assert!((a.distance_m(&b) - 111.2).abs() < 0.5);
    }
}
