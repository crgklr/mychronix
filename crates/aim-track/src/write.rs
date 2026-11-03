//! Changing and making `.tkk` track files (see `docs/protocol-notes.md`,
//! "Track files"). Only records whose layout is known are changed; every other
//! byte of a file is kept as it was, and the file is re-framed (every one of
//! AiM's published files re-frames to the same bytes).
//!
//! What RaceStudio 3 or a MyChron require of a track they load isn't known
//! yet: files made here are for Mychronix until one has been tried there.

use aim_link::frame::{self, Frame};

use crate::{Error, LatLon};

/// The flag byte on every record of the files seen.
const FLAG: u8 = b'a';

/// Byte offsets in `Vnfo`.
const NAME: std::ops::Range<usize> = 0..28;
const CENTER: usize = 48;
const LENGTH: usize = 44;
const START_FINISH: usize = 80;
const GUID_FROM_END: usize = 8;

fn e7(p: LatLon) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&((p.lat * 1e7).round() as i32).to_le_bytes());
    b[4..].copy_from_slice(&((p.lon * 1e7).round() as i32).to_le_bytes());
    b
}

/// `s` into a NUL-padded field of `len` bytes (cut short at a character boundary).
fn padded(s: &str, len: usize) -> Vec<u8> {
    let mut end = s.len().min(len - 1);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut b = s.as_bytes()[..end].to_vec();
    b.resize(len, 0);
    b
}

fn frames(buf: &[u8]) -> Result<Vec<Frame>, Error> {
    frame::parse_all(buf).map_err(|e| Error::Format(e.to_string()))
}

fn encode(frames: &[Frame]) -> Vec<u8> {
    frames.iter().flat_map(Frame::encode).collect()
}

fn vnfo(frames: &mut [Frame]) -> Result<&mut Vec<u8>, Error> {
    frames
        .iter_mut()
        .find(|f| f.tag_str() == "Vnfo")
        .map(|f| &mut f.payload)
        .filter(|p| p.len() >= START_FINISH + 8)
        .ok_or_else(|| Error::Format("no Vnfo record".into()))
}

/// Puts the start/finish line at `at`. A track a MyChron made has its center
/// on the start/finish; that moves with it.
fn move_start_finish(frames: &mut [Frame], at: LatLon) -> Result<(), Error> {
    let v = vnfo(frames)?;
    let line_was_center = v[CENTER..CENTER + 8] == v[START_FINISH..START_FINISH + 8];
    let bytes = e7(at);
    v[START_FINISH..START_FINISH + 8].copy_from_slice(&bytes);
    if line_was_center {
        v[CENTER..CENTER + 8].copy_from_slice(&bytes);
    }
    Ok(())
}

/// The same track with its start/finish line at `at`.
pub fn with_start_finish(buf: &[u8], at: LatLon) -> Result<Vec<u8>, Error> {
    let mut f = frames(buf)?;
    move_start_finish(&mut f, at)?;
    Ok(encode(&f))
}

/// One point of a lap a track is made from: where, and the speed there.
#[derive(Debug, Clone, Copy)]
pub struct LapPoint {
    pub at: LatLon,
    pub kmh: f32,
}

/// A new track from one lap that starts and ends at the start/finish line,
/// laid out as a MyChron lays out a track it makes (`Ptkk Vnfo Vidx pts anf
/// lid`): the outline is the lap, with the speed at each point; the center
/// and the start/finish are the lap's first point. `lid` names the MyChron
/// the lap was recorded on (64 bytes), when there's one to copy.
pub fn from_lap(
    name: &str,
    id: &str,
    guid: [u8; 8],
    lap: &[LapPoint],
    length_m: f32,
    lid: Option<&[u8]>,
) -> Result<Vec<u8>, Error> {
    let first = lap
        .first()
        .ok_or_else(|| Error::Format("no lap to make the track from".into()))?
        .at;
    let mut ptkk = vec![0u8; 268];
    ptkk[4..260].copy_from_slice(&padded(name, 256));
    ptkk[260..].copy_from_slice(&padded(id, 9)[..8]);
    let mut vnfo = vec![0u8; 476];
    vnfo[NAME].copy_from_slice(&padded(name, NAME.len()));
    vnfo[LENGTH..LENGTH + 4].copy_from_slice(&length_m.to_bits().to_le_bytes());
    vnfo[CENTER..CENTER + 8].copy_from_slice(&e7(first));
    // Type and the word after it, as in a MyChron-made track.
    vnfo[72..76].copy_from_slice(&2u32.to_le_bytes());
    vnfo[76..80].copy_from_slice(&[0, 0, 1, 0]);
    vnfo[START_FINISH..START_FINISH + 8].copy_from_slice(&e7(first));
    vnfo[476 - GUID_FROM_END..].copy_from_slice(&guid);
    let vidx = vec![0xff, 0xff, 0, 0, 0xff, 0xff, 0xff, 0xff];
    // Points: lat, lon, then 4 bytes a MyChron leaves unset (0 here).
    let pts: Vec<u8> = lap
        .iter()
        .flat_map(|p| e7(p.at).into_iter().chain([0u8; 4]))
        .collect();
    // Speed (km/h) at each point, then a value a MyChron writes as 30 or 31 (meaning unknown).
    let anf: Vec<u8> = lap
        .iter()
        .flat_map(|p| {
            p.kmh
                .to_bits()
                .to_le_bytes()
                .into_iter()
                .chain(30u32.to_le_bytes())
        })
        .collect();
    let lid = lid
        .filter(|l| l.len() == 64)
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| {
            let mut l = vec![0u8; 64];
            l[..4].copy_from_slice(b"idn\x01");
            l
        });
    let records = [
        (b"Ptkk", ptkk),
        (b"Vnfo", vnfo),
        (b"Vidx", vidx),
        (b"pts\0", pts),
        (b"anf\0", anf),
        (b"lid\0", lid),
    ];
    Ok(encode(
        &records
            .into_iter()
            .map(|(tag, payload)| Frame {
                tag: *tag,
                flag: FLAG,
                payload,
            })
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_tkk;

    fn lap() -> Vec<LapPoint> {
        (0..80)
            .map(|k| {
                let a = k as f64 / 80.0 * std::f64::consts::TAU;
                LapPoint {
                    at: LatLon {
                        lat: 45.0 + 0.0004 * a.sin(),
                        lon: 9.0 + 0.0006 * a.cos(),
                    },
                    kmh: 30.0 + 10.0 * a.cos() as f32,
                }
            })
            .collect()
    }

    #[test]
    fn a_track_made_from_a_lap_reads_back() {
        let bytes = from_lap(
            "Club Ring",
            "mx000001",
            [1, 2, 3, 4, 5, 6, 7, 8],
            &lap(),
            312.5,
            None,
        )
        .unwrap();
        let t = parse_tkk(&bytes).unwrap();
        assert_eq!(t.name, "Club Ring");
        assert_eq!(t.id, "mx000001");
        assert_eq!(t.guid.as_deref(), Some("0102030405060708"));
        assert_eq!(t.outline.len(), 80);
        assert_eq!(t.length_m, Some(312.5));
        let sf = t.start_finish.unwrap();
        assert!(sf.distance_m(&lap()[0].at) < 0.01);
        assert!(t.center.unwrap().distance_m(&sf) < 0.01);
    }

    #[test]
    fn moving_the_line_changes_nothing_else() {
        let made = from_lap("Club Ring", "mx000001", [9; 8], &lap(), 312.5, None).unwrap();
        let to = lap()[40].at;
        let moved = with_start_finish(&made, to).unwrap();
        let t = parse_tkk(&moved).unwrap();
        assert!(t.start_finish.unwrap().distance_m(&to) < 0.01);
        // A MyChron-made track's center goes with its line.
        assert!(t.center.unwrap().distance_m(&to) < 0.01);
        // Only the 16 bytes of the two points (and their records' sums) differ.
        assert_eq!(moved.len(), made.len());
        let differ = moved.iter().zip(&made).filter(|(a, b)| a != b).count();
        assert!(differ <= 16 + 2, "{differ} bytes changed");
    }
}
