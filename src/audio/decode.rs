use std::path::Path;

use anyhow::{anyhow, Result};
use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::buffer::AudioBuffer;
use super::loudness::{Loudness, Meter};
use crate::i18n::tr;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "wave", "aif", "aiff", "mp3", "flac", "ogg", "oga", "m4a", "aac", "mp4", "caf",
];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Son décodé, avec sa mesure de niveau éventuelle et un avertissement si le fichier est abîmé.
pub struct Decoded {
    pub buffer: AudioBuffer,
    pub loudness: Option<Loudness>,
    /// Paquets illisibles ignorés, ou fichier plus court que ce qu'annonce son en-tête.
    pub warning: Option<String>,
}

/// Décode un fichier complet en mémoire, converti en stéréo et rééchantillonné à `target_rate`.
#[cfg(test)]
pub fn decode_file(path: &Path, target_rate: u32) -> Result<AudioBuffer> {
    decode(path, target_rate, false).map(|d| d.buffer)
}

/// Décode un fichier ; `analyze` mesure aussi son niveau, au taux d'origine comme `analyze_file`
/// (les deux chemins donnent la même mesure).
pub fn decode(path: &Path, target_rate: u32, analyze: bool) -> Result<Decoded> {
    let Planar { mut left, mut right, rate: src_rate, warning } = decode_planar(path)?;
    let loudness = analyze.then(|| measure(&left, &right, src_rate));
    if src_rate != target_rate {
        let (l, r) = resample(&left, &right, src_rate, target_rate)?;
        // Les tampons au taux d'origine sont libérés avant l'entrelacement.
        left = l;
        right = r;
    }
    Ok(Decoded { buffer: AudioBuffer::from_planar(left, right, target_rate), loudness, warning })
}

/// Mesure le niveau d'un fichier à son taux d'origine (sonie, crêtes, RMS).
pub fn analyze_file(path: &Path) -> Result<Loudness> {
    let p = decode_planar(path)?;
    Ok(measure(&p.left, &p.right, p.rate))
}

fn measure(left: &[f32], right: &[f32], rate: u32) -> Loudness {
    let mut meter = Meter::new(rate);
    for (l, r) in left.iter().zip(right.iter()) {
        meter.push(*l, *r);
    }
    meter.finish()
}

struct Planar {
    left: Vec<f32>,
    right: Vec<f32>,
    rate: u32,
    warning: Option<String>,
}

/// Coefficients (gauche, droite) de chaque canal pour le passage en stéréo.
/// Mono et stéréo restent tels quels ; au-delà, mixage ITU (centre et arrières à -3 dB,
/// LFE ignoré), ramené pour que la somme d'un côté ne dépasse pas 1.
fn downmix_matrix(channels: Option<Channels>, count: usize) -> Vec<(f32, f32)> {
    if count <= 2 {
        return if count == 1 { vec![(1.0, 1.0)] } else { vec![(1.0, 0.0), (0.0, 1.0)] };
    }
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let mut m: Vec<(f32, f32)> = match channels {
        Some(ch) if ch.count() == count => ch
            .iter()
            .map(|c| match c {
                Channels::FRONT_LEFT | Channels::FRONT_LEFT_WIDE | Channels::FRONT_LEFT_HIGH => (1.0, 0.0),
                Channels::FRONT_RIGHT | Channels::FRONT_RIGHT_WIDE | Channels::FRONT_RIGHT_HIGH => (0.0, 1.0),
                Channels::FRONT_CENTRE | Channels::FRONT_CENTRE_HIGH | Channels::TOP_CENTRE => (H, H),
                Channels::FRONT_LEFT_CENTRE => (1.0, 0.0),
                Channels::FRONT_RIGHT_CENTRE => (0.0, 1.0),
                Channels::REAR_LEFT | Channels::SIDE_LEFT | Channels::REAR_LEFT_CENTRE | Channels::TOP_REAR_LEFT | Channels::TOP_FRONT_LEFT => (H, 0.0),
                Channels::REAR_RIGHT | Channels::SIDE_RIGHT | Channels::REAR_RIGHT_CENTRE | Channels::TOP_REAR_RIGHT | Channels::TOP_FRONT_RIGHT => (0.0, H),
                Channels::REAR_CENTRE | Channels::TOP_REAR_CENTRE | Channels::TOP_FRONT_CENTRE => (0.5, 0.5),
                Channels::LFE1 | Channels::LFE2 => (0.0, 0.0),
                _ => (0.5, 0.5),
            })
            .collect(),
        // Disposition inconnue : les deux premiers canaux, comme avant.
        _ => (0..count).map(|i| match i {
            0 => (1.0, 0.0),
            1 => (0.0, 1.0),
            _ => (0.0, 0.0),
        }).collect(),
    };
    let (sl, sr) = m.iter().fold((0f32, 0f32), |(a, b), &(l, r)| (a + l, b + r));
    let norm = sl.max(sr).max(1.0);
    for c in m.iter_mut() {
        *c = (c.0 / norm, c.1 / norm);
    }
    m
}

/// Décode un fichier en stéréo planaire, à son taux d'origine.
fn decode_planar(path: &Path) -> Result<Planar> {
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
    let expected = track.codec_params.n_frames;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    // Taille connue d'avance : pas de réallocations (qui doublent la mémoire au pic).
    let cap = expected.unwrap_or(0).min(1 << 31) as usize;
    let mut left = Vec::with_capacity(cap);
    let mut right = Vec::with_capacity(cap);
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut buf_frames = 0usize;
    let mut matrix: Vec<(f32, f32)> = Vec::new();
    let mut skipped = 0usize;

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
            // Paquet corrompu : on l'ignore, et on le signale à la fin.
            Err(SymError::DecodeError(_)) => {
                skipped += 1;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        src_rate = spec.rate;
        let channels = spec.channels.count().max(1);
        if matrix.len() != channels {
            matrix = downmix_matrix(Some(spec.channels), channels);
        }
        if sample_buf.is_none() || decoded.capacity() > buf_frames {
            buf_frames = decoded.capacity();
            sample_buf = Some(SampleBuffer::new(buf_frames as u64, spec));
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);
        if channels <= 2 {
            for frame in buf.samples().chunks_exact(channels) {
                let l = frame[0];
                let r = if channels > 1 { frame[1] } else { l };
                left.push(l);
                right.push(r);
            }
        } else {
            for frame in buf.samples().chunks_exact(channels) {
                let (l, r) = frame.iter().zip(&matrix).fold((0f32, 0f32), |(a, b), (&x, &(cl, cr))| (a + x * cl, b + x * cr));
                left.push(l);
                right.push(r);
            }
        }
    }

    if left.is_empty() {
        return Err(anyhow!("fichier vide ou illisible : {}", path.display()));
    }
    left.shrink_to_fit();
    right.shrink_to_fit();
    let mut warning = None;
    if skipped > 0 {
        warning = Some(format!("{} : {skipped} {}", tr("fichier abîmé", "damaged file"), tr("paquet(s) illisible(s) ignoré(s), le son a des trous", "unreadable packet(s) skipped, the sound has gaps")));
    }
    // Fichier tronqué : nettement moins d'images que ce qu'annonce l'en-tête (au-delà d'une demi-seconde).
    if let Some(n) = expected {
        let got = left.len() as u64;
        if n > got && n - got > src_rate as u64 / 2 {
            let secs = |f: u64| f as f64 / src_rate.max(1) as f64;
            warning = Some(format!(
                "{} : {:.1} s {} {:.1} s",
                tr("fichier tronqué", "truncated file"),
                secs(got),
                tr("lues sur", "read out of"),
                secs(n)
            ));
        }
    }
    Ok(Planar { left, right, rate: src_rate, warning })
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

#[cfg(test)]
mod bench {
    /// Temps de décodage d'un long fichier : `JS_BENCH=fichier cargo test --release bench_decode -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_decode() {
        let path = std::env::var("JS_BENCH").expect("JS_BENCH");
        let t = std::time::Instant::now();
        let b = super::decode_file(std::path::Path::new(&path), 48_000).unwrap();
        println!("décodage + rééchantillonnage : {:?} pour {:.0} s", t.elapsed(), b.duration_secs());
        let t = std::time::Instant::now();
        let _ = super::analyze_file(std::path::Path::new(&path)).unwrap();
        println!("analyse : {:?}", t.elapsed());
    }
}
