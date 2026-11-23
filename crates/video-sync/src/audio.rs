//! The video's sound, decoded to mono at a low, fixed rate: enough to hear an
//! engine's firing frequency and its first few harmonics.

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::Error;

/// Samples per second of the decoded sound.
pub const RATE: f64 = 8000.0;

/// Mono sound at [`RATE`], starting at video time `start_s`.
pub struct Sound {
    pub start_s: f64,
    pub samples: Vec<f32>,
}

pub fn decode(path: &Path) -> Result<Option<Sound>, Error> {
    let mss = MediaSourceStream::new(Box::new(File::open(path)?), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| Error::Audio(e.to_string()))?;
    let Some(track) = format.default_track(TrackType::Audio) else {
        return Ok(None);
    };
    let Some(params) = track.codec_params.as_ref().and_then(|p| p.audio()) else {
        return Ok(None);
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| Error::Audio(e.to_string()))?;
    let id = track.id;
    let base = track.time_base;
    let secs = |ts: i64| {
        base.map_or(0.0, |b| {
            ts as f64 * b.numer.get() as f64 / b.denom.get() as f64
        })
    };

    // Each output sample is the mean of the input samples that fall in its slot
    // (a box filter: crude, but the engine's harmonics sit well below 4 kHz).
    let mut start_s = None;
    let mut sum = Vec::<f64>::new();
    let mut count = Vec::<u32>::new();
    let mut frame = Vec::<f32>::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(_)) => break,
            Err(e) => return Err(Error::Audio(e.to_string())),
        };
        if packet.track_id != id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(Error::Audio(e.to_string())),
        };
        let rate = buf.spec().rate() as f64;
        let channels = buf.spec().channels().count().max(1);
        frame.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(&mut frame);
        let t0 = secs(packet.pts.get());
        let first = *start_s.get_or_insert(t0);
        for (i, f) in frame.chunks_exact(channels).enumerate() {
            let slot = ((t0 - first + i as f64 / rate) * RATE).floor();
            if slot < 0.0 {
                continue;
            }
            let slot = slot as usize;
            if slot >= sum.len() {
                sum.resize(slot + 1, 0.0);
                count.resize(slot + 1, 0);
            }
            sum[slot] += f.iter().map(|&x| x as f64).sum::<f64>() / channels as f64;
            count[slot] += 1;
        }
    }
    let Some(start_s) = start_s else {
        return Ok(None);
    };
    let samples = sum
        .iter()
        .zip(&count)
        .map(|(&s, &n)| if n > 0 { (s / n as f64) as f32 } else { 0.0 })
        .collect();
    Ok(Some(Sound { start_s, samples }))
}
