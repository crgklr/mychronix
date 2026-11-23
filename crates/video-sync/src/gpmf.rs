//! GoPro's metadata track (GPMF, `gpmd`): the camera's gyro. Each track sample is
//! about a second of KLV records (4-byte key, type, size, repeat count); `GYRO`
//! holds int16 x/y/z, divided by the `SCAL` before it, in rad/s. The samples of
//! one payload are spread evenly over that payload's time.
//!
//! Format: github.com/gopro/gpmf-parser (Apache-2.0), facts only.

use std::fs::File;

use crate::Error;
use crate::mp4::{self, Track};

/// Gyro readings in the camera's own axes: (video time s, [x, y, z] rad/s).
pub fn gyro(file: &mut File, track: &Track) -> Result<Vec<(f64, [f64; 3])>, Error> {
    let mut out = vec![];
    for s in &track.samples {
        let payload = mp4::read_at(file, s.offset, s.size as usize)?;
        let mut found = vec![];
        walk(&payload, &mut None, &mut found);
        let n = found.len();
        for (i, v) in found.into_iter().enumerate() {
            out.push((s.start_s + s.duration_s * i as f64 / n as f64, v));
        }
    }
    Ok(out)
}

/// Collects GYRO readings from one KLV block (recursing into nested ones).
fn walk(b: &[u8], scale: &mut Option<Vec<f64>>, out: &mut Vec<[f64; 3]>) {
    let mut i = 0;
    while i + 8 <= b.len() {
        let key = &b[i..i + 4];
        let kind = b[i + 4];
        let size = b[i + 5] as usize;
        let repeat = u16::from_be_bytes([b[i + 6], b[i + 7]]) as usize;
        let len = size * repeat;
        let Some(body) = b.get(i + 8..i + 8 + len) else {
            return;
        };
        match (key, kind) {
            (_, 0) => walk(body, scale, out),
            (b"SCAL", _) => *scale = numbers(body, kind, size),
            (b"GYRO", b's') if size == 6 => {
                let sc = scale.as_deref().unwrap_or(&[1.0]);
                let per = |axis: usize| sc.get(axis).or(sc.first()).copied().unwrap_or(1.0);
                for r in body.as_chunks::<6>().0 {
                    let v = |k: usize| i16::from_be_bytes([r[k * 2], r[k * 2 + 1]]) as f64 / per(k);
                    out.push([v(0), v(1), v(2)]);
                }
            }
            _ => {}
        }
        i += 8 + len.div_ceil(4) * 4;
    }
}

/// A SCAL record's values, whatever integer width the camera chose.
fn numbers(body: &[u8], kind: u8, size: usize) -> Option<Vec<f64>> {
    let read = |c: &[u8]| -> f64 {
        match kind {
            b's' => i16::from_be_bytes([c[0], c[1]]) as f64,
            b'S' => u16::from_be_bytes([c[0], c[1]]) as f64,
            b'l' => i32::from_be_bytes(c[..4].try_into().unwrap()) as f64,
            b'L' => u32::from_be_bytes(c[..4].try_into().unwrap()) as f64,
            b'f' => f32::from_be_bytes(c[..4].try_into().unwrap()) as f64,
            _ => 1.0,
        }
    };
    let width = match kind {
        b's' | b'S' => 2,
        b'l' | b'L' | b'f' => 4,
        _ => return None,
    };
    if !size.is_multiple_of(width) {
        return None;
    }
    let v: Vec<f64> = body.chunks_exact(width).map(read).collect();
    (!v.is_empty() && v.iter().all(|x| *x != 0.0)).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn klv(key: &[u8; 4], kind: u8, size: u8, body: &[u8]) -> Vec<u8> {
        let repeat = if size == 0 {
            0
        } else {
            body.len() / size as usize
        };
        let mut v = key.to_vec();
        v.push(kind);
        v.push(size);
        v.extend((repeat as u16).to_be_bytes());
        v.extend(body);
        v.resize(v.len().div_ceil(4) * 4, 0);
        v
    }

    #[test]
    fn reads_scaled_gyro_inside_nested_streams() {
        let mut body = klv(b"SCAL", b's', 2, &100i16.to_be_bytes());
        let mut readings = vec![];
        for v in [100i16, -200, 50, 0, 0, 300] {
            readings.extend(v.to_be_bytes());
        }
        body.extend(klv(b"GYRO", b's', 6, &readings));
        let strm = klv(b"STRM", 0, 1, &body);
        let devc = klv(b"DEVC", 0, 1, &strm);
        let mut out = vec![];
        walk(&devc, &mut None, &mut out);
        assert_eq!(out, vec![[1.0, -2.0, 0.5], [0.0, 0.0, 3.0]]);
    }
}
