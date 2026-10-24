use std::path::Path;

use anyhow::{anyhow, Result};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::buffer::AudioBuffer;
use super::loudness::{Loudness, Meter};

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "wave", "aif", "aiff", "mp3", "flac", "ogg", "oga", "m4a", "aac", "mp4", "caf",
];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Décode un fichier complet en mémoire, converti en stéréo et rééchantillonné à `target_rate`.
pub fn decode_file(path: &Path, target_rate: u32) -> Result<AudioBuffer> {
    let (mut left, mut right, src_rate) = decode_planar(path)?;
    if src_rate != target_rate {
        let (l, r) = resample(&left, &right, src_rate, target_rate)?;
        left = l;
        right = r;
    }
    Ok(AudioBuffer::from_planar(left, right, target_rate))
}

/// Mesure le niveau d'un fichier à son taux d'origine (sonie, crêtes, RMS).
pub fn analyze_file(path: &Path) -> Result<Loudness> {
    let (left, right, rate) = decode_planar(path)?;
    let mut meter = Meter::new(rate);
    for (l, r) in left.iter().zip(right.iter()) {
        meter.push(*l, *r);
    }
    Ok(meter.finish())
}

/// Décode un fichier en stéréo planaire, à son taux d'origine.
fn decode_planar(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32)> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("aucune piste audio dans {}", path.display()))?;
    let track_id = track.id;
    let mut src_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut buf_frames = 0usize;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // Paquet corrompu : on l'ignore et on continue.
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        src_rate = spec.rate;
        let channels = spec.channels.count().max(1);
        if sample_buf.is_none() || decoded.capacity() > buf_frames {
            buf_frames = decoded.capacity();
            sample_buf = Some(SampleBuffer::new(buf_frames as u64, spec));
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks_exact(channels) {
            let l = frame[0];
            let r = if channels > 1 { frame[1] } else { l };
            left.push(l);
            right.push(r);
        }
    }

    if left.is_empty() {
        return Err(anyhow!("fichier vide ou illisible : {}", path.display()));
    }
    Ok((left, right, src_rate))
}

fn resample(left: &[f32], right: &[f32], from: u32, to: u32) -> Result<(Vec<f32>, Vec<f32>)> {
    use rubato::{FftFixedIn, Resampler};

    const CHUNK: usize = 1024;
    let mut rs = FftFixedIn::<f32>::new(from as usize, to as usize, CHUNK, 2, 2)?;
    let delay = rs.output_delay();
    let expected = (left.len() as u64 * to as u64 / from as u64) as usize;
    let mut out_l = Vec::with_capacity(expected + delay + CHUNK * 4);
    let mut out_r = Vec::with_capacity(expected + delay + CHUNK * 4);

    let mut pos = 0;
    while pos + CHUNK <= left.len() {
        let out = rs.process(&[&left[pos..pos + CHUNK], &right[pos..pos + CHUNK]], None)?;
        out_l.extend_from_slice(&out[0]);
        out_r.extend_from_slice(&out[1]);
        pos += CHUNK;
    }
    if pos < left.len() {
        let out = rs.process_partial(Some(&[&left[pos..], &right[pos..]]), None)?;
        out_l.extend_from_slice(&out[0]);
        out_r.extend_from_slice(&out[1]);
    }
    // Vide la latence interne du rééchantillonneur.
    while out_l.len() < expected + delay {
        let out = rs.process_partial(None::<&[&[f32]]>, None)?;
        if out[0].is_empty() {
            break;
        }
        out_l.extend_from_slice(&out[0]);
        out_r.extend_from_slice(&out[1]);
    }
    let skip = delay.min(out_l.len());
    out_l.drain(..skip);
    out_r.drain(..skip);
    out_l.truncate(expected);
    out_r.truncate(expected);
    Ok((out_l, out_r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_length_and_level() {
        let n = 44_100;
        let l: Vec<f32> = (0..n).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin() * 0.5).collect();
        let (out_l, out_r) = resample(&l, &l, 44_100, 48_000).unwrap();
        assert_eq!(out_l.len(), 48_000);
        assert_eq!(out_r.len(), 48_000);
        let peak = out_l[1000..47_000].iter().fold(0f32, |a, b| a.max(b.abs()));
        assert!((peak - 0.5).abs() < 0.02, "crête {peak}");
    }
}
