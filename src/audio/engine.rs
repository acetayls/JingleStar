//! Gestion du périphérique audio : un thread dédié possède le flux cpal
//! (non `Send` sur certaines plateformes) et y installe un nouveau `Mixer`
//! à chaque (ré)ouverture.

use std::sync::Arc;
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SizedSample, FromSample, StreamConfig};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use super::buffer::AudioBuffer;
use crate::i18n::tr;
use super::mixer::{Command, Event, Mixer, Telemetry};

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
        let (cmd_tx, cmd_rx) = unbounded::<Command>();
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

    pub fn send(&self, cmd: Command) {
        let _ = self.commands.send(cmd);
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
    T: SizedSample + FromSample<f32>,
{
    let mut scratch = vec![0f32; 16384];
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                if scratch.len() < data.len() {
                    scratch.resize(data.len(), 0.0);
                }
                let buf = &mut scratch[..data.len()];
                mixer.process(buf);
                for (o, s) in data.iter_mut().zip(buf.iter()) {
                    *o = T::from_sample(s.clamp(-1.0, 1.0));
                }
            },
            on_err,
            None,
        )
        .map_err(|e| format!("{} : {e}", tr("ouverture du flux impossible", "cannot open stream")))
}
