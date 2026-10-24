//! Gestion du périphérique audio : un thread dédié possède le flux cpal
//! (non `Send` sur certaines plateformes) et y installe un nouveau `Mixer`
//! à chaque (ré)ouverture.

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
                        HostMsg::Open { host, device, buffer_size, reply } => {
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

        Self { commands: cmd_tx, events: ev_rx, stream_errors: err_rx, telemetry, host_tx }
    }

    /// N'attend jamais : si le flux est arrêté et la file pleine, la commande est perdue
    /// (les réglages sont renvoyés à la réouverture, les départs périmés sont ignorés).
    pub fn send(&self, cmd: Command) {
        let _ = self.commands.try_send(cmd);
    }

    /// Ouvre (ou rouvre) le périphérique. Bloquant le temps de l'ouverture.
    pub fn open(
        &self,
        host: Option<String>,
        device: Option<String>,
        buffer_size: Option<u32>,
    ) -> Result<DeviceInfo, String> {
        let (tx, rx) = bounded(1);
        self.host_tx
            .send(HostMsg::Open { host, device, buffer_size, reply: tx })
            .map_err(|_| tr("moteur audio arrêté", "audio engine stopped").to_string())?;
        rx.recv().map_err(|_| tr("moteur audio arrêté", "audio engine stopped").to_string())?
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
    let device = device_name
        .and_then(|n| host.output_devices().ok()?.find(|d| d.to_string() == n))
        .or_else(|| host.default_output_device())
        .ok_or_else(|| tr("aucun périphérique de sortie", "no output device").to_string())?;
    let (mut config, format) = pick_config(&device)?;
    let info = DeviceInfo {
        host: host.id().name().to_string(),
        name: device.to_string(),
        channels: config.channels as usize,
        sample_rate: config.sample_rate,
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
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                if scratch.len() < data.len() {
                    scratch.resize(data.len(), 0.0);
                }
                let buf = &mut scratch[..data.len()];
                mixer.process(buf);
                for (o, &s) in data.iter_mut().zip(buf.iter()) {
                    *o = T::from_f32(s);
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
    fn from_f32(s: f32) -> Self;
}

#[inline]
fn to_int(s: f32, scale: f64) -> i64 {
    let s = if s.is_finite() { s as f64 } else { 0.0 };
    (s * scale).round().clamp(-scale, scale - 1.0) as i64
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
    fn from_f32(s: f32) -> Self {
        to_int(s, 32_768.0) as i16
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
    fn from_f32(s: f32) -> Self {
        (to_int(s, 32_768.0) + 32_768) as u16
    }
}

impl OutputSample for u8 {
    fn from_f32(s: f32) -> Self {
        (to_int(s, 128.0) + 128) as u8
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
}
