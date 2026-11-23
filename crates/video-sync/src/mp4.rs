//! Just enough of the MP4/MOV container to find a data track (GoPro's `gpmd`) and
//! where and when each of its samples sits in the file. Reads box headers and
//! sample tables only, so a 4 GB video costs a few kilobytes of reading.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::Error;

/// One sample of a track: where its bytes are and when it plays, in seconds.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub offset: u64,
    pub size: u32,
    pub start_s: f64,
    pub duration_s: f64,
}

/// A track: its sample format (`stsd`'s first entry, e.g. `gpmd`, `mp4a`, `avc1`) and samples.
#[derive(Debug, Clone)]
pub struct Track {
    pub format: [u8; 4],
    pub samples: Vec<Sample>,
}

/// What the container says about the whole recording.
#[derive(Debug, Clone, Default)]
pub struct Movie {
    pub duration_s: f64,
    /// The camera's clock when recording started (seconds since 1970), if it wrote one.
    /// Often wrong: a camera whose clock was never set says 2016.
    pub created_unix: Option<i64>,
    pub tracks: Vec<Track>,
}

/// Seconds between 1904 (MP4's epoch) and 1970.
const MP4_EPOCH: i64 = 2_082_844_800;

pub fn read(file: &mut File) -> Result<Movie, Error> {
    let len = file.metadata()?.len();
    let moov = find_box(file, 0, len, b"moov")?.ok_or(Error::NotMp4)?;
    let mut movie = Movie::default();
    for (kind, start, end) in children(file, moov.0, moov.1)? {
        match &kind {
            b"mvhd" => {
                let body = read_at(file, start, (end - start).min(40) as usize)?;
                let (created, scale, dur) = if body[0] == 1 {
                    (
                        be_u64(&body[4..]) as i64,
                        be_u32(&body[20..]),
                        be_u64(&body[24..]),
                    )
                } else {
                    (
                        be_u32(&body[4..]) as i64,
                        be_u32(&body[12..]),
                        be_u32(&body[16..]) as u64,
                    )
                };
                if scale > 0 {
                    movie.duration_s = dur as f64 / scale as f64;
                }
                if created > MP4_EPOCH {
                    movie.created_unix = Some(created - MP4_EPOCH);
                }
            }
            b"trak" => {
                if let Some(track) = read_track(file, start, end)? {
                    movie.tracks.push(track);
                }
            }
            _ => {}
        }
    }
    Ok(movie)
}

fn read_track(file: &mut File, start: u64, end: u64) -> Result<Option<Track>, Error> {
    let Some(mdia) = find_box(file, start, end, b"mdia")? else {
        return Ok(None);
    };
    let mut scale = 0u32;
    let mut stbl = None;
    for (kind, s, e) in children(file, mdia.0, mdia.1)? {
        match &kind {
            b"mdhd" => {
                let b = read_at(file, s, 24)?;
                scale = if b[0] == 1 {
                    be_u32(&b[20..])
                } else {
                    be_u32(&b[12..])
                };
            }
            b"minf" => stbl = find_box(file, s, e, b"stbl")?,
            _ => {}
        }
    }
    let (Some((s, e)), true) = (stbl, scale > 0) else {
        return Ok(None);
    };
    let mut format = [0u8; 4];
    let (mut stts, mut stsz, mut stsc, mut chunks) = (vec![], vec![], vec![], vec![]);
    for (kind, bs, be) in children(file, s, e)? {
        let wanted = matches!(&kind, b"stts" | b"stsc" | b"stsz" | b"stco" | b"co64");
        let b = if wanted {
            read_at(file, bs, (be - bs) as usize)?
        } else {
            vec![]
        };
        if wanted && b.len() < 12 {
            continue;
        }
        match &kind {
            b"stsd" => format.copy_from_slice(&read_at(file, bs + 12, 4)?),
            b"stts" => stts = pairs(&b),
            b"stsc" => {
                let n = be_u32(&b[4..]) as usize;
                stsc = (0..n.min(b.len().saturating_sub(8) / 12))
                    .map(|i| (be_u32(&b[8 + i * 12..]), be_u32(&b[12 + i * 12..])))
                    .collect();
            }
            b"stsz" => {
                let (fixed, n) = (be_u32(&b[4..]), be_u32(&b[8..]) as usize);
                stsz = if fixed != 0 {
                    // The count is the file's word: no more samples than fit in it.
                    let file_len = file.metadata()?.len();
                    vec![fixed; n.min((file_len / fixed as u64) as usize)]
                } else {
                    (0..n.min(b.len().saturating_sub(12) / 4))
                        .map(|i| be_u32(&b[12 + i * 4..]))
                        .collect()
                };
            }
            b"stco" => {
                let n = be_u32(&b[4..]) as usize;
                chunks = (0..n.min(b.len().saturating_sub(8) / 4))
                    .map(|i| be_u32(&b[8 + i * 4..]) as u64)
                    .collect();
            }
            b"co64" => {
                let n = be_u32(&b[4..]) as usize;
                chunks = (0..n.min(b.len().saturating_sub(8) / 8))
                    .map(|i| be_u64(&b[8 + i * 8..]))
                    .collect();
            }
            _ => {}
        }
    }
    // Offsets: walk chunks, each holding the sample count its stsc run says.
    let mut offsets = Vec::with_capacity(stsz.len());
    let mut sample = 0usize;
    for (ci, &chunk_offset) in chunks.iter().enumerate() {
        let chunk_no = ci as u32 + 1;
        let per_chunk = stsc
            .iter()
            .take_while(|(first, _)| *first <= chunk_no)
            .last()
            .map_or(0, |r| r.1);
        let mut at = chunk_offset;
        for _ in 0..per_chunk {
            let Some(&size) = stsz.get(sample) else { break };
            offsets.push(at);
            at += size as u64;
            sample += 1;
        }
    }
    // Times: stts runs of (count, delta) in the track's time scale.
    let mut samples = Vec::with_capacity(offsets.len());
    let mut t = 0u64;
    let mut i = 0usize;
    'runs: for (count, delta) in stts {
        for _ in 0..count {
            if i >= offsets.len() {
                break 'runs;
            }
            samples.push(Sample {
                offset: offsets[i],
                size: stsz[i],
                start_s: t as f64 / scale as f64,
                duration_s: delta as f64 / scale as f64,
            });
            t += delta as u64;
            i += 1;
        }
    }
    Ok(Some(Track { format, samples }))
}

fn pairs(b: &[u8]) -> Vec<(u32, u32)> {
    let n = be_u32(&b[4..]) as usize;
    (0..n.min(b.len().saturating_sub(8) / 8))
        .map(|i| (be_u32(&b[8 + i * 8..]), be_u32(&b[12 + i * 8..])))
        .collect()
}

/// The boxes directly inside `start..end`: (type, body start, body end).
fn children(file: &mut File, start: u64, end: u64) -> Result<Vec<([u8; 4], u64, u64)>, Error> {
    let mut out = vec![];
    let mut at = start;
    while at + 8 <= end {
        let h = read_at(file, at, 16.min((end - at) as usize))?;
        let mut size = be_u32(&h) as u64;
        let kind: [u8; 4] = h[4..8].try_into().unwrap();
        let mut header = 8;
        if size == 1 && h.len() >= 16 {
            size = be_u64(&h[8..]);
            header = 16;
        } else if size == 0 {
            size = end - at;
        }
        if size < header || at + size > end {
            break;
        }
        out.push((kind, at + header, at + size));
        at += size;
    }
    Ok(out)
}

fn find_box(
    file: &mut File,
    start: u64,
    end: u64,
    kind: &[u8; 4],
) -> Result<Option<(u64, u64)>, Error> {
    Ok(children(file, start, end)?
        .into_iter()
        .find(|(k, _, _)| k == kind)
        .map(|(_, s, e)| (s, e)))
}

pub fn read_at(file: &mut File, at: u64, len: usize) -> Result<Vec<u8>, Error> {
    file.seek(SeekFrom::Start(at))?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

fn be_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b[..4].try_into().unwrap())
}

fn be_u64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().unwrap())
}
