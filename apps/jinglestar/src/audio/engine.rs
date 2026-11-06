//! Gestion du périphérique audio : un thread dédié possède le flux cpal
//! (non `Send` sur certaines plateformes) et y installe un nouveau `Mixer`
//! à chaque (ré)ouverture.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SizedSample, StreamConfig};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use super::buffer::AudioBuffer;
use crate::i18n::tr;
use super::mixer::{Command, Event, Mixer, Telemetry, MAX_BLOCK};

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub host: String,
    pub name: String,
    pub channels: usize,
    pub sample_rate: u32,
    /// Le périphérique demandé est absent : c'est la sortie par défaut qui joue.
    pub wanted_missing: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeviceDesc {
    pub name: String,
    pub channels: usize,
    pub is_default: bool,
}

pub fn host_names() -> Vec<String> {
    cpal::available_hosts().iter().map(|h| h.name().to_string()).collect()
}

fn find_host(name: Option<&str>) -> cpal::Host {
    name.and_then(|n| cpal::available_hosts().into_iter().find(|h| h.name() == n))
        .and_then(|id| cpal::host_from_id(id).ok())
        .unwrap_or_else(cpal::default_host)
}

fn max_channels(device: &cpal::Device) -> usize {
    device
        .supported_output_configs()
        .map(|it| it.map(|c| c.channels() as usize).max().unwrap_or(0))
        .unwrap_or(0)
}

pub fn list_devices(host: Option<&str>) -> Vec<DeviceDesc> {
    let host = find_host(host);
    let default = host.default_output_device().map(|d| d.to_string());
    let Ok(devices) = host.output_devices() else { return Vec::new() };
    devices
        .map(|d| {
            let name = d.to_string();
            DeviceDesc {
                is_default: default.as_deref() == Some(name.as_str()),
                channels: max_channels(&d),
                name,
            }
        })
        .collect()
}

/// Choisit la configuration avec le plus de canaux, à 48 kHz ou 44,1 kHz de préférence.
fn pick_config(device: &cpal::Device) -> Result<(StreamConfig, SampleFormat), String> {
    let ranges: Vec<_> = device
        .supported_output_configs()
        .map_err(|e| format!("{} : {e}", tr("configurations indisponibles", "no configuration available")))?
        .collect();
    let best = ranges
        .iter()
        .map(|r| {
            let rate = [48_000, 44_100]
                .into_iter()
                .find(|&sr| r.min_sample_rate() <= sr && sr <= r.max_sample_rate())
                .unwrap_or(r.max_sample_rate());
            let rate_score = match rate {
                48_000 => 2,
                44_100 => 1,
                _ => 0,
            };
            let fmt_score = (r.sample_format() == SampleFormat::F32) as u8;
            ((r.channels(), rate_score, fmt_score), r.with_sample_rate(rate))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, c)| c);
    match best {
        Some(c) => Ok((c.config(), c.sample_format())),
        None => device
            .default_output_config()
            .map(|c| (c.config(), c.sample_format()))
            .map_err(|e| format!("{} : {e}", tr("aucune configuration de sortie", "no output configuration"))),
    }
}

enum HostMsg {
    Open {
        host: Option<String>,
        device: Option<String>,
        buffer_size: Option<u32>,
        /// Seulement le périphérique demandé : s'il est absent, rien ne change.
        strict: bool,
        reply: Sender<Result<DeviceInfo, String>>,
    },
}

pub struct AudioEngine {
    commands: Sender<Command>,
    pub events: Receiver<Event>,
    /// Erreurs remontées par le flux (périphérique débranché, etc.).
    pub stream_errors: Receiver<String>,
    pub telemetry: Arc<Telemetry>,
    host_tx: Sender<HostMsg>,
    /// Commandes perdues (file pleine pendant que le flux est arrêté), depuis la dernière lecture.
    dropped: AtomicU64,
}

impl AudioEngine {
    pub fn new() -> Self {
        // File bornée : le callback lit sans jamais allouer ni libérer de mémoire.
        let (cmd_tx, cmd_rx) = bounded::<Command>(4096);
        let (ev_tx, ev_rx) = bounded::<Event>(1024);
        let (err_tx, err_rx) = bounded::<String>(64);
        let (garbage_tx, garbage_rx) = bounded::<Arc<AudioBuffer>>(1024);
        let (host_tx, host_rx) = unbounded::<HostMsg>();
        let telemetry = Arc::new(Telemetry::new());

        // Libère la mémoire des sons terminés hors du thread audio.
        thread::Builder::new()
            .name("audio-garbage".into())
            .spawn(move || for _ in garbage_rx {})
            .expect("thread audio-garbage");

        let tel = telemetry.clone();
        thread::Builder::new()
            .name("audio-host".into())
            .spawn(move || {
                let mut stream: Option<cpal::Stream> = None;
                for msg in host_rx {
                    match msg {
                        HostMsg::Open { host, device, buffer_size, strict, reply } => {
                            if strict && !device_present(host.as_deref(), device.as_deref()) {
                                let name = device.unwrap_or_default();
                                let _ = reply.send(Err(format!("{} : {name}", tr("périphérique absent", "device not found"))));
                                continue;
                            }
                            stream = None;
                            let mixer_for = |channels, rate| {
                                Mixer::new(
                                    channels,
                                    rate,
                                    cmd_rx.clone(),
                                    ev_tx.clone(),
                                    garbage_tx.clone(),
                                    tel.clone(),
                                )
                            };
                            let res = open_stream(
                                host.as_deref(),
                                device.as_deref(),
                                buffer_size,
                                mixer_for,
                                err_tx.clone(),
                            );
                            let _ = reply.send(res.map(|(s, info)| {
                                stream = Some(s);
                                info
                            }));
                        }
                    }
                }
                drop(stream);
            })
            .expect("thread audio-host");

        Self { commands: cmd_tx, events: ev_rx, stream_errors: err_rx, telemetry, host_tx, dropped: AtomicU64::new(0) }
    }

    /// N'attend jamais : si le flux est arrêté et la file pleine, la commande est perdue
    /// (les réglages sont renvoyés à la réouverture, les départs périmés sont ignorés).
    /// Les commandes perdues sont comptées (voir `take_dropped`).
    pub fn send(&self, cmd: Command) {
        if self.commands.try_send(cmd).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Nombre de commandes perdues depuis le dernier appel.
    pub fn take_dropped(&self) -> u64 {
        self.dropped.swap(0, Ordering::Relaxed)
    }

    /// Demande l'ouverture (ou la réouverture) du périphérique, sans attendre : un pilote lent
    /// (ASIO, WASAPI exclusif) ne fige pas l'interface. La réponse arrive sur le récepteur rendu.
    pub fn request_open(
        &self,
        host: Option<String>,
        device: Option<String>,
        buffer_size: Option<u32>,
        strict: bool,
    ) -> Receiver<Result<DeviceInfo, String>> {
        let (tx, rx) = bounded(1);
        if self.host_tx.send(HostMsg::Open { host, device, buffer_size, strict, reply: tx.clone() }).is_err() {
            let _ = tx.send(Err(tr("moteur audio arrêté", "audio engine stopped").to_string()));
        }
        rx
    }
}

/// Le périphérique demandé est-il branché ? (`None` : la sortie par défaut, s'il y en a une.)
fn device_present(host_name: Option<&str>, device_name: Option<&str>) -> bool {
    let host = find_host(host_name);
    match device_name {
        Some(n) => host.output_devices().is_ok_and(|mut it| it.any(|d| d.to_string() == n)),
        None => host.default_output_device().is_some(),
    }
}

fn open_stream(
    host_name: Option<&str>,
    device_name: Option<&str>,
    buffer_size: Option<u32>,
    mixer_for: impl Fn(usize, u32) -> Mixer,
    errors: Sender<String>,
) -> Result<(cpal::Stream, DeviceInfo), String> {
    let host = find_host(host_name);
    let wanted = device_name.and_then(|n| host.output_devices().ok()?.find(|d| d.to_string() == n));
    // Périphérique demandé absent : on joue sur la sortie par défaut, en le signalant.
    let wanted_missing = device_name.filter(|_| wanted.is_none()).map(str::to_string);
    let device = wanted
        .or_else(|| host.default_output_device())
        .ok_or_else(|| tr("aucun périphérique de sortie", "no output device").to_string())?;
    let (mut config, format) = pick_config(&device)?;
    let info = DeviceInfo {
        host: host.id().name().to_string(),
        name: device.to_string(),
        channels: config.channels as usize,
        sample_rate: config.sample_rate,
        wanted_missing,
    };

    let build = |config: StreamConfig| {
        let mixer = mixer_for(config.channels as usize, config.sample_rate);
        let errors = errors.clone();
        let on_err = move |e: cpal::Error| {
            let _ = errors.try_send(e.to_string());
        };
        match format {
            SampleFormat::F32 => build_typed::<f32>(&device, config, mixer, on_err),
            SampleFormat::F64 => build_typed::<f64>(&device, config, mixer, on_err),
            SampleFormat::I16 => build_typed::<i16>(&device, config, mixer, on_err),
            SampleFormat::I24 => build_typed::<cpal::I24>(&device, config, mixer, on_err),
            SampleFormat::I32 => build_typed::<i32>(&device, config, mixer, on_err),
            SampleFormat::U16 => build_typed::<u16>(&device, config, mixer, on_err),
            SampleFormat::U8 => build_typed::<u8>(&device, config, mixer, on_err),
            other => Err(format!("{} : {other:?}", tr("format d'échantillon non géré", "unsupported sample format"))),
        }
    };

    let stream = match buffer_size {
        Some(n) => {
            config.buffer_size = BufferSize::Fixed(n);
            // Si la taille demandée est refusée, on retombe sur celle du pilote.
            build(config).or_else(|_| {
                config.buffer_size = BufferSize::Default;
                build(config)
            })?
        }
        None => build(config)?,
    };
    stream.play().map_err(|e| format!("{} : {e}", tr("démarrage du flux impossible", "cannot start stream")))?;
    Ok((stream, info))
}

fn build_typed<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut mixer: Mixer,
    on_err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + OutputSample,
{
    let mut scratch = vec![0f32; MAX_BLOCK * config.channels.max(1) as usize];
    let mut dither = Dither(0x2545_F491);
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                if scratch.len() < data.len() {
                    scratch.resize(data.len(), 0.0);
                }
                let buf = &mut scratch[..data.len()];
                mixer.process(buf);
                if T::DITHER {
                    for (o, &s) in data.iter_mut().zip(buf.iter()) {
                        // Le silence numérique reste parfaitement silencieux.
                        let noise = if s == 0.0 { 0.0 } else { dither.tpdf() };
                        *o = T::from_f32_dithered(s, noise);
                    }
                } else {
                    for (o, &s) in data.iter_mut().zip(buf.iter()) {
                        *o = T::from_f32(s);
                    }
                }
            },
            on_err,
            None,
        )
        .map_err(|e| format!("{} : {e}", tr("ouverture du flux impossible", "cannot open stream")))
}

/// Conversion vers le format du pilote : écrêtage à la pleine échelle, arrondi au plus
/// proche (pas de troncature, qui ajoute une distorsion d'un bit) et silence pour un NaN.
///
/// Les conversions de cpal débordent à +1,0 en 24 bits (8 388 608 ne tient pas dans un
/// I24 : la crête positive devient la crête négative, un claquement à pleine échelle).
pub trait OutputSample: Sized {
    /// Format de 16 bits ou moins : un dither TPDF masque la distorsion d'arrondi
    /// (audible sur les fins de fondu).
    const DITHER: bool = false;
    fn from_f32(s: f32) -> Self;
    /// `noise` : bruit triangulaire en pas de quantification (entre -1 et 1).
    fn from_f32_dithered(s: f32, _noise: f32) -> Self {
        Self::from_f32(s)
    }
}

/// Générateur de bruit du dither (xorshift32), sans allocation dans le callback.
struct Dither(u32);

impl Dither {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Bruit à densité triangulaire, entre -1 et 1 pas de quantification.
    fn tpdf(&mut self) -> f32 {
        self.next() - self.next()
    }
}

#[inline]
fn to_int(s: f32, scale: f64) -> i64 {
    to_int_dithered(s, scale, 0.0)
}

#[inline]
fn to_int_dithered(s: f32, scale: f64, noise: f32) -> i64 {
    let s = if s.is_finite() { s as f64 } else { 0.0 };
    (s * scale + noise as f64).round().clamp(-scale, scale - 1.0) as i64
}

impl OutputSample for f32 {
    fn from_f32(s: f32) -> Self {
        if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 }
    }
}

impl OutputSample for f64 {
    fn from_f32(s: f32) -> Self {
        f32::from_f32(s) as f64
    }
}

impl OutputSample for i16 {
    const DITHER: bool = true;
    fn from_f32(s: f32) -> Self {
        to_int(s, 32_768.0) as i16
    }
    fn from_f32_dithered(s: f32, noise: f32) -> Self {
        to_int_dithered(s, 32_768.0, noise) as i16
    }
}

impl OutputSample for cpal::I24 {
    fn from_f32(s: f32) -> Self {
        cpal::I24::new(to_int(s, 8_388_608.0) as i32).unwrap_or(<cpal::I24 as cpal::Sample>::EQUILIBRIUM)
    }
}

impl OutputSample for i32 {
    fn from_f32(s: f32) -> Self {
        to_int(s, 2_147_483_648.0) as i32
    }
}

impl OutputSample for u16 {
    const DITHER: bool = true;
    fn from_f32(s: f32) -> Self {
        (to_int(s, 32_768.0) + 32_768) as u16
    }
    fn from_f32_dithered(s: f32, noise: f32) -> Self {
        (to_int_dithered(s, 32_768.0, noise) + 32_768) as u16
    }
}

impl OutputSample for u8 {
    const DITHER: bool = true;
    fn from_f32(s: f32) -> Self {
        (to_int(s, 128.0) + 128) as u8
    }
    fn from_f32_dithered(s: f32, noise: f32) -> Self {
        (to_int_dithered(s, 128.0, noise) + 128) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_output_never_wraps_and_rounds() {
        assert_eq!(cpal::I24::from_f32(1.0).inner(), 8_388_607);
        assert_eq!(cpal::I24::from_f32(1.5).inner(), 8_388_607);
        assert_eq!(cpal::I24::from_f32(-1.0).inner(), -8_388_608);
        assert_eq!(i16::from_f32(1.0), i16::MAX);
        assert_eq!(i16::from_f32(-1.0), i16::MIN);
        assert_eq!(i32::from_f32(1.0), i32::MAX);
        assert_eq!(u16::from_f32(0.0), 32_768);
        assert_eq!(u8::from_f32(1.0), 255);
        assert_eq!(i16::from_f32(f32::NAN), 0);
        assert_eq!(f32::from_f32(f32::INFINITY), 0.0);
        // Arrondi au plus proche, symétrique.
        assert_eq!(i16::from_f32(0.6 / 32_768.0), 1);
        assert_eq!(i16::from_f32(-0.6 / 32_768.0), -1);
        // Un fichier 16 bits ressort au bit près.
        for v in [-32_768i16, -12_345, -1, 0, 1, 23_456, 32_767] {
            assert_eq!(i16::from_f32(v as f32 / 32_768.0), v);
            assert_eq!(cpal::I24::from_f32(v as f32 / 32_768.0).inner(), v as i32 * 256);
        }
    }

    #[test]
    fn dither_is_tpdf_and_unbiased() {
        let mut d = Dither(1);
        let n = 200_000;
        let (mut sum, mut max) = (0f64, 0f32);
        for _ in 0..n {
            let x = d.tpdf();
            sum += x as f64;
            max = max.max(x.abs());
        }
        assert!(max < 1.0, "au plus un pas de quantification : {max}");
        assert!((sum / n as f64).abs() < 0.01, "sans biais : {}", sum / n as f64);
        // Un niveau d'un tiers de pas, invisible sans dither, ressort en moyenne.
        let level = 0.3 / 32_768.0;
        assert_eq!(i16::from_f32(level), 0);
        let mean: f64 = (0..n).map(|_| i16::from_f32_dithered(level, d.tpdf()) as f64).sum::<f64>() / n as f64;
        assert!((mean - 0.3).abs() < 0.02, "moyenne {mean}");
        // Les grands niveaux restent bornés.
        assert_eq!(i16::from_f32_dithered(1.0, 0.99), i16::MAX);
        assert_eq!(i16::from_f32_dithered(-1.0, -0.99), i16::MIN);
    }
}
