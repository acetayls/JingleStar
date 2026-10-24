//! Mixeur temps réel : tourne dans le callback audio, sans allocation ni verrou.
//!
//! Chaque module mixe ses voix dans un bus stéréo, applique son volume, puis
//! écrit ce bus sur la paire de sorties choisie (1-2, 3-4, …) du périphérique.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};

use super::buffer::AudioBuffer;

pub const MAX_MODULES: usize = 8;
pub const JINGLE_SLOTS: usize = 12;
/// Deux emplacements de voix réservés à la playlist de chaque module,
/// utilisés en alternance pour permettre le fondu enchaîné.
pub const PLAYLIST_SLOTS: [usize; 2] = [JINGLE_SLOTS, JINGLE_SLOTS + 1];
pub const SLOTS: usize = JINGLE_SLOTS + 2;

pub fn is_playlist_slot(slot: usize) -> bool {
    PLAYLIST_SLOTS.contains(&slot)
}

/// Position « rien ne joue » dans la télémétrie.
pub const NOT_PLAYING: u64 = u64::MAX;

const FADE_OUT_SECS: f32 = 0.010;
const GAIN_SMOOTH_SECS: f32 = 0.005;

/// Morceau en attente d'enchaînement.
pub struct Queued {
    pub buffer: Arc<AudioBuffer>,
    pub gain: f32,
    pub id: u64,
    /// Durée du fondu enchaîné en images ; 0 = enchaînement en fin de fichier.
    pub crossfade: usize,
}

/// Zone de lecture d'un son, en images : début, fin, et boucle éventuelle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    pub looping: Option<(usize, usize)>,
    /// Durées de fondu d'entrée et de sortie, en images.
    pub fade_in: usize,
    pub fade_out: usize,
}

impl Region {
    pub fn full(buffer: &AudioBuffer) -> Self {
        Self { start: 0, end: buffer.frames, looping: None, fade_in: 0, fade_out: 0 }
    }

    fn clamp(mut self, frames: usize) -> Self {
        self.end = self.end.min(frames);
        self.start = self.start.min(self.end);
        self.looping = self.looping.and_then(|(a, b)| {
            let a = a.max(self.start);
            let b = b.min(self.end);
            (b > a).then_some((a, b))
        });
        self
    }
}

pub enum Command {
    Play { module: usize, slot: usize, id: u64, buffer: Arc<AudioBuffer>, gain: f32, region: Region },
    /// Modifie la zone d'un son en cours (réglage des points de boucle pendant la lecture).
    SetRegion { module: usize, slot: usize, region: Region },
    Stop { module: usize, slot: usize },
    StopModule { module: usize },
    StopAll,
    SetSlotGain { module: usize, slot: usize, gain: f32 },
    SetModuleGain { module: usize, gain: f32 },
    SetModuleOutput { module: usize, pair: usize },
    /// Morceau à enchaîner sans blanc quand le morceau de playlist en cours se termine.
    QueueNext { module: usize, next: Option<Queued> },
}

#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// Une voix s'est terminée (fin du fichier ou arrêt).
    Ended { module: usize, slot: usize, id: u64 },
    /// La playlist est passée au morceau mis en file avec `QueueNext`.
    Advanced { module: usize, id: u64, slot: usize },
}

/// État partagé lu par l'interface (positions de lecture et crêtes).
pub struct Telemetry {
    pub pos: [[AtomicU64; SLOTS]; MAX_MODULES],
    pub len: [[AtomicU64; SLOTS]; MAX_MODULES],
    /// Crête depuis la dernière lecture, en bits de f32 (positifs, donc ordonnés comme des u32).
    pub peak: [[AtomicU32; 2]; MAX_MODULES],
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            pos: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(NOT_PLAYING))),
            len: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
            peak: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
        }
    }

    /// Lit la position (en images) d'un emplacement, `None` s'il ne joue pas.
    pub fn position(&self, module: usize, slot: usize) -> Option<(u64, u64)> {
        let p = self.pos[module][slot].load(Ordering::Relaxed);
        (p != NOT_PLAYING).then(|| (p, self.len[module][slot].load(Ordering::Relaxed)))
    }

    pub fn take_peak(&self, module: usize) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.peak[module][c].swap(0, Ordering::Relaxed)))
    }

    fn reset(&self) {
        for m in 0..MAX_MODULES {
            for s in 0..SLOTS {
                self.pos[m][s].store(NOT_PLAYING, Ordering::Relaxed);
            }
        }
    }
}

struct Voice {
    id: u64,
    region: Region,
    buffer: Arc<AudioBuffer>,
    pos: usize,
    gain: f32,
    current_gain: f32,
    /// Images jouées depuis le départ (pour le fondu d'entrée).
    played: usize,
    /// Images restantes du fondu de sortie, si la voix est en cours d'arrêt.
    fade_left: Option<usize>,
    fade_len: usize,
}

impl Voice {
    fn new(id: u64, buffer: Arc<AudioBuffer>, gain: f32, region: Region) -> Self {
        let region = region.clamp(buffer.frames);
        Self { id, region, buffer, pos: region.start, gain, current_gain: gain, played: 0, fade_left: None, fade_len: 1 }
    }
}

struct ModuleState {
    gain: f32,
    current_gain: f32,
    pair: usize,
    voices: [Option<Voice>; SLOTS],
    next: Option<Queued>,
}

pub struct Mixer {
    modules: [ModuleState; MAX_MODULES],
    channels: usize,
    fade_frames: usize,
    smooth: f32,
    bus: Vec<f32>,
    commands: Receiver<Command>,
    events: Sender<Event>,
    /// Les buffers terminés sont renvoyés hors du thread audio pour y être libérés.
    garbage: Sender<Arc<AudioBuffer>>,
    telemetry: Arc<Telemetry>,
}

impl Mixer {
    pub fn new(
        channels: usize,
        sample_rate: u32,
        commands: Receiver<Command>,
        events: Sender<Event>,
        garbage: Sender<Arc<AudioBuffer>>,
        telemetry: Arc<Telemetry>,
    ) -> Self {
        telemetry.reset();
        let sr = sample_rate.max(1) as f32;
        Self {
            modules: std::array::from_fn(|_| ModuleState {
                gain: 1.0,
                current_gain: 1.0,
                pair: 0,
                voices: std::array::from_fn(|_| None),
                next: None,
            }),
            channels: channels.max(1),
            fade_frames: ((FADE_OUT_SECS * sr) as usize).max(1),
            smooth: 1.0 - (-1.0 / (GAIN_SMOOTH_SECS * sr)).exp(),
            bus: vec![0.0; 8192 * 2],
            commands,
            events,
            garbage,
            telemetry,
        }
    }

    fn dispose(&self, voice: Option<Voice>) {
        if let Some(v) = voice {
            let _ = self.garbage.try_send(v.buffer);
        }
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Play { module, slot, id, buffer, gain, region } => {
                self.telemetry.len[module][slot].store(buffer.frames as u64, Ordering::Relaxed);
                self.telemetry.pos[module][slot].store(region.start as u64, Ordering::Relaxed);
                let old = self.modules[module].voices[slot].replace(Voice::new(id, buffer, gain, region));
                self.dispose(old);
            }
            Command::SetRegion { module, slot, region } => {
                if let Some(v) = self.modules[module].voices[slot].as_mut() {
                    v.region = region.clamp(v.buffer.frames);
                    v.pos = v.pos.clamp(v.region.start, v.region.end);
                }
            }
            Command::Stop { module, slot } => self.fade(module, slot),
            Command::StopModule { module } => {
                self.clear_next(module);
                (0..SLOTS).for_each(|s| self.fade(module, s));
            }
            Command::StopAll => {
                for m in 0..MAX_MODULES {
                    self.clear_next(m);
                    (0..SLOTS).for_each(|s| self.fade(m, s));
                }
            }
            Command::SetSlotGain { module, slot, gain } => {
                if let Some(v) = self.modules[module].voices[slot].as_mut() {
                    v.gain = gain;
                }
            }
            Command::SetModuleGain { module, gain } => self.modules[module].gain = gain,
            Command::SetModuleOutput { module, pair } => self.modules[module].pair = pair,
            Command::QueueNext { module, next } => {
                let old = std::mem::replace(&mut self.modules[module].next, next);
                if let Some(q) = old {
                    let _ = self.garbage.try_send(q.buffer);
                }
            }
        }
    }

    fn clear_next(&mut self, module: usize) {
        if let Some(q) = self.modules[module].next.take() {
            let _ = self.garbage.try_send(q.buffer);
        }
    }

    fn fade(&mut self, module: usize, slot: usize) {
        let frames = self.fade_frames;
        if let Some(v) = self.modules[module].voices[slot].as_mut() {
            if v.fade_left.is_none() {
                // Arrêt manuel : on reprend le fondu de sortie du son, 10 ms au minimum.
                v.fade_len = v.region.fade_out.max(frames);
                v.fade_left = Some(v.fade_len);
            }
        }
    }

    /// Fondu enchaîné : quand le morceau en cours arrive à `crossfade` images de sa fin,
    /// le suivant démarre sur l'autre emplacement en fondu d'entrée.
    fn start_crossfade(&mut self, m: usize) {
        let st = &mut self.modules[m];
        let Some(xfade) = st.next.as_ref().map(|q| q.crossfade).filter(|&x| x > 0) else { return };
        for (i, &slot) in PLAYLIST_SLOTS.iter().enumerate() {
            let other = PLAYLIST_SLOTS[1 - i];
            let Some(v) = st.voices[slot].as_mut() else { continue };
            if v.fade_left.is_some() {
                continue;
            }
            let remaining = v.region.end.saturating_sub(v.pos);
            if remaining > xfade {
                continue;
            }
            v.fade_len = remaining.max(1);
            v.fade_left = Some(v.fade_len);
            let q = st.next.take().unwrap();
            let mut region = Region::full(&q.buffer);
            region.fade_in = xfade.min(q.buffer.frames);
            let (id, len) = (q.id, q.buffer.frames);
            if let Some(old) = st.voices[other].replace(Voice::new(id, q.buffer, q.gain, region)) {
                let _ = self.garbage.try_send(old.buffer);
            }
            self.telemetry.len[m][other].store(len as u64, Ordering::Relaxed);
            self.telemetry.pos[m][other].store(0, Ordering::Relaxed);
            let _ = self.events.try_send(Event::Advanced { module: m, id, slot: other });
            return;
        }
    }

    /// Remplit `out` (entrelacé, `channels` canaux).
    pub fn process(&mut self, out: &mut [f32]) {
        while let Ok(cmd) = self.commands.try_recv() {
            self.handle(cmd);
        }
        out.fill(0.0);
        let ch = self.channels;
        let frames = out.len() / ch;
        if self.bus.len() < frames * 2 {
            self.bus.resize(frames * 2, 0.0);
        }

        for m in 0..MAX_MODULES {
            let active = self.modules[m].voices.iter().any(|v| v.is_some());
            if !active {
                // Laisse le gain rejoindre sa cible pour éviter un saut au prochain départ.
                let st = &mut self.modules[m];
                st.current_gain = st.gain;
                continue;
            }
            self.start_crossfade(m);
            let bus = &mut self.bus[..frames * 2];
            bus.fill(0.0);

            for slot in 0..SLOTS {
                let mut ended = false;
                let mut advanced = false;
                let ended_id;
                {
                    let st = &mut self.modules[m];
                    let Some(v) = st.voices[slot].as_mut() else { continue };
                    ended_id = v.id;
                    let mut from = 0;
                    while let Some(stop_at) = render_voice(v, bus, from, frames, self.smooth) {
                        // Fin naturelle d'un morceau de playlist : on enchaîne sur la même
                        // image si un suivant est prêt, sinon la voix se termine.
                        let natural = is_playlist_slot(slot) && v.fade_left.is_none();
                        match st.next.take().filter(|_| natural) {
                            Some(q) => {
                                let region = Region::full(&q.buffer);
                                let old = std::mem::replace(v, Voice::new(q.id, q.buffer, q.gain, region));
                                let _ = self.garbage.try_send(old.buffer);
                                advanced = true;
                                from = stop_at;
                            }
                            None => {
                                ended = true;
                                break;
                            }
                        }
                    }
                }
                let tel = &self.telemetry;
                if ended {
                    if let Some(old) = self.modules[m].voices[slot].take() {
                        let _ = self.garbage.try_send(old.buffer);
                    }
                    tel.pos[m][slot].store(NOT_PLAYING, Ordering::Relaxed);
                    let _ = self.events.try_send(Event::Ended { module: m, slot, id: ended_id });
                } else if let Some(v) = self.modules[m].voices[slot].as_ref() {
                    if advanced {
                        tel.len[m][slot].store(v.buffer.frames as u64, Ordering::Relaxed);
                        let _ = self.events.try_send(Event::Advanced { module: m, id: v.id, slot });
                    }
                    tel.pos[m][slot].store(v.pos as u64, Ordering::Relaxed);
                }
            }

            // Volume du module, crêtes, puis écriture sur la paire de sorties.
            let st = &mut self.modules[m];
            let base = if st.pair * 2 + 1 < ch { st.pair * 2 } else { 0 };
            let mut peak = [0f32; 2];
            for f in 0..frames {
                st.current_gain += (st.gain - st.current_gain) * self.smooth;
                let l = bus[f * 2] * st.current_gain;
                let r = bus[f * 2 + 1] * st.current_gain;
                peak[0] = peak[0].max(l.abs());
                peak[1] = peak[1].max(r.abs());
                if ch == 1 {
                    out[f] += (l + r) * 0.5;
                } else {
                    out[f * ch + base] += l;
                    out[f * ch + base + 1] += r;
                }
            }
            for (c, p) in peak.iter().enumerate() {
                self.telemetry.peak[m][c].fetch_max(p.to_bits(), Ordering::Relaxed);
            }
        }
    }
}

/// Mixe une voix dans le bus stéréo à partir de l'image `from`.
/// Renvoie l'image où la voix s'est terminée, ou `None` si elle continue.
fn render_voice(v: &mut Voice, bus: &mut [f32], from: usize, frames: usize, smooth: f32) -> Option<usize> {
    let data = &v.buffer.samples;
    for f in from..frames {
        if let Some((ls, le)) = v.region.looping {
            if v.pos >= le && v.fade_left.is_none() {
                v.pos = ls;
            }
        }
        if v.pos >= v.region.end {
            return Some(f);
        }
        let mut fade = 1.0;
        if v.played < v.region.fade_in {
            fade = v.played as f32 / v.region.fade_in as f32;
        }
        if v.region.looping.is_none() && v.region.fade_out > 0 {
            let remaining = v.region.end - v.pos;
            if remaining < v.region.fade_out {
                fade *= remaining as f32 / v.region.fade_out as f32;
            }
        }
        if let Some(left) = v.fade_left.as_mut() {
            if *left == 0 {
                return Some(f);
            }
            fade *= *left as f32 / v.fade_len as f32;
            *left -= 1;
        }
        v.played += 1;
        v.current_gain += (v.gain - v.current_gain) * smooth;
        let g = v.current_gain * fade;
        bus[f * 2] += data[v.pos * 2] * g;
        bus[f * 2 + 1] += data[v.pos * 2 + 1] * g;
        v.pos += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::{bounded, unbounded};

    fn ramp(frames: usize) -> Arc<AudioBuffer> {
        let v: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
        Arc::new(AudioBuffer::from_planar(v.clone(), v, 1000))
    }

    fn setup(channels: usize) -> (Mixer, crossbeam_channel::Sender<Command>, crossbeam_channel::Receiver<Event>) {
        let (ctx, crx) = unbounded();
        let (etx, erx) = bounded(64);
        let (gtx, _grx) = bounded(64);
        // Taux élevé pour un fondu d'arrêt court ; lissage du gain quasi immédiat.
        let mut m = Mixer::new(channels, 1000, crx, etx, gtx, Arc::new(Telemetry::new()));
        m.smooth = 1.0;
        (m, ctx, erx)
    }

    #[test]
    fn routes_module_to_its_output_pair() {
        let (mut mx, tx, _) = setup(4);
        let buf = ramp(100);
        tx.send(Command::SetModuleOutput { module: 0, pair: 1 }).unwrap();
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region: Region::full(&buf), buffer: buf, gain: 1.0 }).unwrap();
        let mut out = vec![0.0; 4 * 50];
        mx.process(&mut out);
        let f = 10;
        assert_eq!(out[f * 4], 0.0);
        assert_eq!(out[f * 4 + 1], 0.0);
        assert!((out[f * 4 + 2] - 0.10).abs() < 1e-6);
        assert!((out[f * 4 + 3] - 0.10).abs() < 1e-6);
    }

    #[test]
    fn missing_pair_falls_back_to_1_2() {
        let (mut mx, tx, _) = setup(2);
        let buf = ramp(100);
        tx.send(Command::SetModuleOutput { module: 0, pair: 3 }).unwrap();
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region: Region::full(&buf), buffer: buf, gain: 1.0 }).unwrap();
        let mut out = vec![0.0; 2 * 50];
        mx.process(&mut out);
        assert!(out[20] > 0.0);
    }

    #[test]
    fn loop_region_repeats() {
        let (mut mx, tx, ev) = setup(2);
        let buf = ramp(100);
        let region = Region { start: 10, end: 100, looping: Some((20, 30)), fade_in: 0, fade_out: 0 };
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region, buffer: buf, gain: 1.0 }).unwrap();
        let mut out = vec![0.0; 2 * 500];
        mx.process(&mut out);
        assert!((out[0] - 0.10).abs() < 1e-6, "commence au point de début");
        // Après 10 + 10 images, retour au début de boucle.
        assert!((out[20 * 2] - 0.20).abs() < 1e-6);
        assert!(out.iter().all(|s| *s < 0.30 + 1e-6), "ne dépasse jamais la fin de boucle");
        assert!(ev.try_recv().is_err(), "la boucle ne se termine pas");
    }

    #[test]
    fn gapless_playlist_advance() {
        let (mut mx, tx, ev) = setup(2);
        let a = ramp(10);
        let b = ramp(10);
        let slot = PLAYLIST_SLOTS[0];
        tx.send(Command::Play { module: 0, slot, id: 1, region: Region::full(&a), buffer: a, gain: 1.0 }).unwrap();
        tx.send(Command::QueueNext { module: 0, next: Some(Queued { buffer: b, gain: 1.0, id: 2, crossfade: 0 }) }).unwrap();
        let mut out = vec![0.0; 2 * 15];
        mx.process(&mut out);
        assert!(matches!(ev.try_recv(), Ok(Event::Advanced { id: 2, .. })));
        // Le second morceau démarre immédiatement après le premier.
        assert_eq!(out[10 * 2], 0.0);
        assert!((out[11 * 2] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn crossfade_overlaps_tracks() {
        let (mut mx, tx, ev) = setup(2);
        let a = Arc::new(AudioBuffer::from_planar(vec![0.5; 100], vec![0.5; 100], 1000));
        let b = Arc::new(AudioBuffer::from_planar(vec![0.5; 100], vec![0.5; 100], 1000));
        let slot = PLAYLIST_SLOTS[0];
        tx.send(Command::Play { module: 0, slot, id: 1, region: Region::full(&a), buffer: a, gain: 1.0 }).unwrap();
        tx.send(Command::QueueNext { module: 0, next: Some(Queued { buffer: b, gain: 1.0, id: 2, crossfade: 40 }) }).unwrap();
        let mut out = vec![0.0; 2 * 64];
        // Premier bloc : 64 images, le fondu n'a pas encore commencé (reste 100 > 40).
        mx.process(&mut out);
        assert!(ev.try_recv().is_err());
        // Bloc suivant : il reste 36 images < 40, le fondu démarre.
        mx.process(&mut out);
        match ev.try_recv() {
            Ok(Event::Advanced { id: 2, slot, .. }) => assert_eq!(slot, PLAYLIST_SLOTS[1]),
            other => panic!("attendu Advanced, reçu {other:?}"),
        }
        // Somme des deux fondus linéaires ≈ niveau constant.
        assert!((out[10 * 2] - 0.5).abs() < 0.05, "niveau pendant le fondu : {}", out[10 * 2]);
    }

    #[test]
    fn fade_in_and_out() {
        let (mut mx, tx, _) = setup(2);
        let buf = Arc::new(AudioBuffer::from_planar(vec![1.0; 100], vec![1.0; 100], 1000));
        let region = Region { start: 0, end: 100, looping: None, fade_in: 10, fade_out: 20 };
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region, buffer: buf, gain: 1.0 }).unwrap();
        let mut out = vec![0.0; 2 * 100];
        mx.process(&mut out);
        assert_eq!(out[0], 0.0);
        assert!((out[5 * 2] - 0.5).abs() < 1e-6);
        assert!((out[50 * 2] - 1.0).abs() < 1e-6);
        assert!((out[90 * 2] - 0.5).abs() < 1e-6);
    }
}
