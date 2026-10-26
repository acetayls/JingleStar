//! Mixeur temps réel : tourne dans le callback audio, sans allocation ni verrou.
//!
//! Chaque module mixe ses voix dans un bus stéréo, applique son volume, puis
//! écrit ce bus sur la paire de sorties choisie (1-2, 3-4, …) du périphérique.
//! Chaque jingle passe par son propre égaliseur avant le bus, et chaque bus par
//! l'égaliseur du module avant le volume. Un limiteur de sécurité protège enfin
//! les sorties de l'écrêtage quand plusieurs sons se cumulent.
//!
//! Aucun changement n'est appliqué en marche d'escalier : volumes lissés, EQ et
//! changement de sortie en fondu, et un son interrompu par un nouveau départ
//! s'éteint en fondu court sur une voix « de traîne ».

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};

use super::buffer::AudioBuffer;
use super::eq::{EqCoeffs, EqProcessor};
use super::limiter::Limiter;

pub const MAX_MODULES: usize = 4;
pub const JINGLE_SLOTS: usize = 12;
/// Deux emplacements de voix réservés à la playlist de chaque module,
/// utilisés en alternance pour permettre le fondu enchaîné.
pub const PLAYLIST_SLOTS: [usize; 2] = [JINGLE_SLOTS, JINGLE_SLOTS + 1];
pub const SLOTS: usize = JINGLE_SLOTS + 2;
/// Voix de traîne par module (sons interrompus qui finissent leur fondu).
const TAILS: usize = 6;

pub fn is_playlist_slot(slot: usize) -> bool {
    PLAYLIST_SLOTS.contains(&slot)
}

/// Position « rien ne joue » dans la télémétrie.
pub const NOT_PLAYING: u64 = u64::MAX;

const FADE_OUT_SECS: f32 = 0.010;
const GAIN_SMOOTH_SECS: f32 = 0.005;
/// Rampe minimale quand un son démarre ou s'arrête au milieu du fichier.
const DECLICK_SECS: f32 = 0.003;
/// Fondu enchaîné au point de boucle (masque un raccord imparfait).
const LOOP_XFADE_SECS: f32 = 0.004;
/// Fondu entre ancien et nouveau réglage d'égaliseur.
const EQ_XFADE_SECS: f32 = 0.020;
/// Taille de bloc préparée d'avance ; au-delà, les tampons grandissent une fois.
pub const MAX_BLOCK: usize = 8192;

/// Morceau en attente d'enchaînement.
pub struct Queued {
    pub buffer: Arc<AudioBuffer>,
    pub gain: f32,
    pub id: u64,
    /// Durée du fondu enchaîné en images ; 0 = enchaînement en fin de fichier.
    pub crossfade: usize,
}

/// Son à lancer sur un autre emplacement, à l'image près, à la fin naturelle d'une voix
/// (cue « Enchaîner » de la Cuelist).
pub struct FollowOn {
    pub slot: usize,
    pub id: u64,
    pub buffer: Arc<AudioBuffer>,
    pub gain: f32,
    pub region: Region,
    pub eq: EqCoeffs,
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

    /// Ramène la zone dans le fichier ; points inversés ou boucle vide sont neutralisés.
    fn clamp(mut self, frames: usize, declick: usize) -> Self {
        self.end = self.end.min(frames);
        self.start = self.start.min(self.end);
        self.looping = self.looping.and_then(|(a, b)| {
            let a = a.max(self.start);
            let b = b.min(self.end);
            (b > a).then_some((a, b))
        });
        // Couper au milieu d'une forme d'onde fait un clic : rampe minimale.
        if self.start > 0 {
            self.fade_in = self.fade_in.max(declick);
        }
        if self.end < frames {
            self.fade_out = self.fade_out.max(declick);
        }
        self
    }
}

pub enum Command {
    Play { module: usize, slot: usize, id: u64, buffer: Arc<AudioBuffer>, gain: f32, region: Region, eq: EqCoeffs },
    /// Modifie la zone d'un son en cours (réglage des points de boucle pendant la lecture).
    SetRegion { module: usize, slot: usize, region: Region },
    Stop { module: usize, slot: usize },
    StopModule { module: usize },
    StopAll,
    /// Arrêt d'urgence des jingles de tous les modules (Échap) : fondu court, playlists intactes.
    StopJingles,
    SetSlotGain { module: usize, slot: usize, gain: f32 },
    SetModuleGain { module: usize, gain: f32 },
    SetModuleOutput { module: usize, pair: usize },
    /// Égaliseur d'un emplacement (jingle), appliqué aussi au son en cours.
    SetSlotEq { module: usize, slot: usize, eq: EqCoeffs },
    SetModuleEq { module: usize, eq: EqCoeffs },
    /// Déplace la lecture d'un son en cours à l'image `frame`, avec un micro-fondu (sortie puis entrée).
    Seek { module: usize, slot: usize, frame: usize },
    /// Morceau à enchaîner sans blanc quand le morceau de playlist en cours se termine.
    QueueNext { module: usize, next: Option<Queued> },
    /// Met en pause (ou reprend) la voix d'un emplacement : fondu court, puis tête figée.
    SetPaused { module: usize, slot: usize, paused: bool },
    /// À la fin naturelle de la voix `after_id` (emplacement `after_slot`), lance `next`
    /// sur la même image. Ignoré si cette voix ne joue plus ou s'arrête déjà.
    PlayAfter { module: usize, after_slot: usize, after_id: u64, next: FollowOn },
    /// Retire le son prévu à la fin de la voix de l'emplacement.
    CancelFollow { module: usize, slot: usize },
}

impl Command {
    /// Commande qui lance du son : périmée si elle attend depuis l'ouverture précédente.
    fn starts_sound(&self) -> bool {
        matches!(
            self,
            Command::Play { .. } | Command::QueueNext { .. } | Command::SetRegion { .. } | Command::Seek { .. } | Command::PlayAfter { .. }
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// Une voix s'est terminée (fin du fichier ou arrêt). `follow` : le son prévu par
    /// `PlayAfter` est parti sur cet emplacement, avec cet identifiant.
    Ended { module: usize, slot: usize, id: u64, follow: Option<(usize, u64)> },
    /// La playlist est passée au morceau mis en file avec `QueueNext`.
    Advanced { module: usize, id: u64, slot: usize },
}

/// État partagé lu par l'interface (positions de lecture et crêtes).
pub struct Telemetry {
    pub pos: [[AtomicU64; SLOTS]; MAX_MODULES],
    pub len: [[AtomicU64; SLOTS]; MAX_MODULES],
    /// Crête depuis la dernière lecture, en bits de f32 (positifs, donc ordonnés comme des u32).
    pub peak: [[AtomicU32; 2]; MAX_MODULES],
    /// Vrai pendant un fondu de sortie audible : arrêt en fondu ou fondu de fin réglé.
    pub fading: [[AtomicBool; SLOTS]; MAX_MODULES],
    /// Nombre d'appels du callback audio : s'il ne bouge plus, le flux est arrêté.
    pub callbacks: AtomicU64,
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            pos: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(NOT_PLAYING))),
            len: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
            peak: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            fading: std::array::from_fn(|_| std::array::from_fn(|_| AtomicBool::new(false))),
            callbacks: AtomicU64::new(0),
        }
    }

    /// Lit la position (en images) d'un emplacement, `None` s'il ne joue pas.
    pub fn position(&self, module: usize, slot: usize) -> Option<(u64, u64)> {
        let p = self.pos[module][slot].load(Ordering::Relaxed);
        (p != NOT_PLAYING).then(|| (p, self.len[module][slot].load(Ordering::Relaxed)))
    }

    /// L'emplacement est-il en train de s'éteindre en fondu ?
    pub fn is_fading(&self, module: usize, slot: usize) -> bool {
        self.fading[module][slot].load(Ordering::Relaxed)
    }

    pub fn take_peak(&self, module: usize) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.peak[module][c].swap(0, Ordering::Relaxed)))
    }

    fn reset(&self) {
        for m in 0..MAX_MODULES {
            for s in 0..SLOTS {
                self.pos[m][s].store(NOT_PLAYING, Ordering::Relaxed);
                self.fading[m][s].store(false, Ordering::Relaxed);
            }
        }
    }
}

/// Durées des rampes anti-clic, en images, au taux du périphérique.
#[derive(Debug, Clone, Copy)]
struct Ramps {
    stop: usize,
    declick: usize,
    loop_xfade: usize,
    eq_xfade: usize,
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
    /// Fondu au point de boucle : avant la fin de boucle (avec ce qui précède le
    /// début de boucle) ou après le retour (avec ce qui suit la fin de boucle).
    loop_pre: usize,
    loop_post: usize,
    wrapped: bool,
    /// Déplacement demandé : la voix descend à zéro en `seek_len` images, saute, puis remonte.
    seek: Option<usize>,
    seek_left: usize,
    seek_in: usize,
    seek_len: usize,
    /// Pause : la voix s'éteint en fondu court puis ne joue plus sans perdre sa place,
    /// et remonte en fondu court à la reprise.
    paused: bool,
    pause_gain: f32,
    pause_step: f32,
    /// Images de silence avant le départ, dans le premier bloc (son enchaîné à l'image près).
    delay: usize,
}

impl Voice {
    fn new(id: u64, buffer: Arc<AudioBuffer>, gain: f32, region: Region, ramps: Ramps) -> Self {
        let mut v = Self {
            id,
            region,
            buffer,
            pos: 0,
            gain,
            current_gain: gain,
            played: 0,
            fade_left: None,
            fade_len: 1,
            loop_pre: 0,
            loop_post: 0,
            wrapped: false,
            seek: None,
            seek_left: 0,
            seek_in: 0,
            seek_len: ramps.stop,
            paused: false,
            pause_gain: 1.0,
            pause_step: 1.0 / ramps.stop as f32,
            delay: 0,
        };
        v.set_region(region, ramps);
        v.pos = v.region.start;
        v
    }

    fn set_region(&mut self, region: Region, ramps: Ramps) {
        let frames = self.buffer.frames;
        self.region = region.clamp(frames, ramps.declick);
        (self.loop_pre, self.loop_post) = match self.region.looping {
            Some((ls, le)) => {
                let max = ramps.loop_xfade.min((le - ls) / 2);
                let pre = max.min(ls);
                let post = max.min(frames - le);
                if pre >= post { (pre, 0) } else { (0, post) }
            }
            None => (0, 0),
        };
    }

    /// Lance (ou raccourcit) le fondu d'arrêt pour qu'il dure au plus `max` images,
    /// en partant du niveau actuel.
    fn fade_within(&mut self, max: usize) {
        let max = max.max(1);
        match self.fade_left {
            None => {
                self.fade_len = max;
                self.fade_left = Some(max);
            }
            Some(left) if left > max => {
                let level = left as f32 / self.fade_len as f32;
                self.fade_left = Some(max);
                self.fade_len = ((max as f32 / level.max(1e-6)).round() as usize).max(max);
            }
            Some(_) => {}
        }
    }

    /// Fondu de sortie en cours, plus long que le micro-fondu anti-clic : arrêt en fondu,
    /// ou entrée dans le fondu de fin d'un son joué une fois.
    fn is_fading_out(&self, declick: usize) -> bool {
        match self.fade_left {
            Some(_) => self.fade_len > declick,
            None => {
                self.region.looping.is_none()
                    && self.region.fade_out > declick
                    && self.pos + self.region.fade_out >= self.region.end
            }
        }
    }

    /// Échantillon stéréo à la position `p`, avec le fondu de boucle éventuel.
    fn frame(&self, p: usize) -> (f32, f32) {
        let d = &self.buffer.samples;
        let (l, r) = (d[p * 2], d[p * 2 + 1]);
        let Some((ls, le)) = self.region.looping else { return (l, r) };
        let (other, t) = if self.loop_pre > 0 && p < le && p + self.loop_pre >= le {
            let k = p + self.loop_pre - le;
            (p - (le - ls), (k as f32 + 0.5) / self.loop_pre as f32)
        } else if self.loop_post > 0 && self.wrapped && p >= ls && p < ls + self.loop_post {
            let k = p - ls;
            (le + k, 1.0 - (k as f32 + 0.5) / self.loop_post as f32)
        } else {
            return (l, r);
        };
        (l + (d[other * 2] - l) * t, r + (d[other * 2 + 1] - r) * t)
    }
}

/// Son interrompu qui termine son fondu, avec son égaliseur.
struct Tail {
    voice: Voice,
    eq: EqProcessor,
}

impl ModuleState {
    /// Rien ne joue sur ce module (ni voix, ni traîne).
    fn is_idle(&self) -> bool {
        self.voices.iter().all(|v| v.is_none()) && self.tails.iter().all(|t| t.is_none())
    }
}

struct ModuleState {
    gain: f32,
    current_gain: f32,
    pair: usize,
    /// Changement de sortie en cours : ancienne paire et images restantes du fondu.
    prev_pair: usize,
    route_left: usize,
    voices: [Option<Voice>; SLOTS],
    tails: [Option<Tail>; TAILS],
    next: Option<Queued>,
    /// Son à enchaîner à la fin naturelle de la voix de chaque emplacement.
    follow: [Option<FollowOn>; SLOTS],
    eq: EqProcessor,
    slot_eq: [EqProcessor; SLOTS],
}

pub struct Mixer {
    modules: Vec<ModuleState>,
    channels: usize,
    ramps: Ramps,
    smooth: f32,
    bus: Vec<f32>,
    /// Bus d'une voix seule, quand elle a un égaliseur.
    voice_bus: Vec<f32>,
    /// Tampon de travail des fondus d'égaliseur.
    eq_scratch: Vec<f32>,
    limiter: Option<Limiter>,
    commands: Receiver<Command>,
    events: Sender<Event>,
    /// Les buffers terminés sont renvoyés hors du thread audio pour y être libérés.
    garbage: Sender<Arc<AudioBuffer>>,
    telemetry: Arc<Telemetry>,
}

/// Lissage exponentiel du gain, qui finit exactement sur la cible
/// (sans quoi un volume revenu à 0 dB resterait à 0,99999994).
#[inline]
fn smooth_towards(current: &mut f32, target: f32, k: f32) {
    let d = target - *current;
    let next = *current + d * k;
    // Quand le pas n'avance plus (arrondi du f32), l'écart restant est inaudible.
    *current = if next == *current || d.abs() < 1e-6 { target } else { next };
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
        let secs = |s: f32| ((s * sr) as usize).max(1);
        let ramps = Ramps {
            stop: secs(FADE_OUT_SECS),
            declick: secs(DECLICK_SECS),
            loop_xfade: secs(LOOP_XFADE_SECS),
            eq_xfade: secs(EQ_XFADE_SECS),
        };
        let channels = channels.max(1);
        let flat = EqProcessor::new(EqCoeffs::flat(), ramps.eq_xfade);
        let mut mixer = Self {
            modules: (0..MAX_MODULES)
                .map(|_| ModuleState {
                    gain: 1.0,
                    current_gain: 1.0,
                    pair: 0,
                    prev_pair: 0,
                    route_left: 0,
                    voices: std::array::from_fn(|_| None),
                    tails: std::array::from_fn(|_| None),
                    next: None,
                    follow: std::array::from_fn(|_| None),
                    eq: flat,
                    slot_eq: [flat; SLOTS],
                })
                .collect(),
            channels,
            ramps,
            smooth: 1.0 - (-1.0 / (GAIN_SMOOTH_SECS * sr)).exp(),
            bus: vec![0.0; MAX_BLOCK * 2],
            voice_bus: vec![0.0; MAX_BLOCK * 2],
            eq_scratch: vec![0.0; MAX_BLOCK * 2],
            limiter: Some(Limiter::new(channels, sample_rate)),
            commands,
            events,
            garbage,
            telemetry,
        };
        // Des départs restés en file pendant que le périphérique était fermé
        // partiraient tous ensemble : on ne garde que les réglages.
        while let Ok(cmd) = mixer.commands.try_recv() {
            if !cmd.starts_sound() {
                mixer.handle(cmd);
            }
        }
        mixer
    }

    /// Retard ajouté par le limiteur de sortie, en images.
    #[cfg(test)]
    pub fn latency(&self) -> usize {
        self.limiter.as_ref().map_or(0, |l| l.latency())
    }

    fn dispose(&self, buffer: Arc<AudioBuffer>) {
        let _ = self.garbage.try_send(buffer);
    }

    /// Retire la voix d'un emplacement sans couper net : elle finit en fondu court
    /// sur une voix de traîne, avec l'état de son égaliseur.
    fn retire(&mut self, m: usize, slot: usize, new_eq: EqCoeffs) {
        retire(&mut self.modules[m], slot, new_eq, self.ramps, &self.garbage);
    }

    fn handle(&mut self, cmd: Command) {
        let ramps = self.ramps;
        match cmd {
            Command::Play { module, slot, id, buffer, gain, region, eq } => {
                let next = FollowOn { slot, id, buffer, gain, region, eq };
                start_voice(&mut self.modules[module], &self.telemetry, module, next, ramps, &self.garbage);
            }
            Command::PlayAfter { module, after_slot, after_id, next } => {
                let st = &mut self.modules[module];
                let ok = next.slot != after_slot
                    && next.slot < SLOTS
                    && st.voices[after_slot].as_ref().is_some_and(|v| v.id == after_id && v.fade_left.is_none());
                let old = if ok { st.follow[after_slot].replace(next) } else { Some(next) };
                if let Some(old) = old {
                    self.dispose(old.buffer);
                }
            }
            Command::CancelFollow { module, slot } => {
                if let Some(old) = self.modules[module].follow[slot].take() {
                    self.dispose(old.buffer);
                }
            }
            Command::SetRegion { module, slot, region } => {
                if let Some(v) = self.modules[module].voices[slot].as_mut() {
                    let old_end = v.region.end;
                    v.set_region(region, ramps);
                    v.pos = v.pos.max(v.region.start);
                    if v.region.looping.is_none() && v.pos >= v.region.end {
                        // Fin déplacée avant la tête de lecture : on s'arrête en fondu
                        // sur les images qui suivent plutôt que de couper net.
                        if v.pos < old_end {
                            v.region.end = old_end;
                            v.region.fade_out = 0;
                            v.fade_within(ramps.stop);
                        } else {
                            v.pos = v.region.end;
                        }
                    }
                }
            }
            Command::Seek { module, slot, frame } => {
                if let Some(v) = self.modules[module].voices[slot].as_mut().filter(|v| v.fade_left.is_none()) {
                    let frame = frame.clamp(v.region.start, v.region.end.saturating_sub(1).max(v.region.start));
                    // Déjà en train de descendre : on garde la descente en cours, seule la cible change.
                    if v.seek.is_none() {
                        v.seek_left = if v.seek_in > 0 { v.seek_len - v.seek_in } else { v.seek_len };
                        v.seek_in = 0;
                    }
                    v.seek = Some(frame);
                }
            }
            Command::Stop { module, slot } => self.fade(module, slot, true),
            Command::StopModule { module } => {
                self.clear_next(module);
                self.clear_follows(module);
                (0..SLOTS).for_each(|s| self.fade(module, s, false));
            }
            Command::StopAll => {
                for m in 0..MAX_MODULES {
                    self.clear_next(m);
                    self.clear_follows(m);
                    (0..SLOTS).for_each(|s| self.fade(m, s, false));
                }
            }
            Command::StopJingles => {
                for m in 0..MAX_MODULES {
                    self.clear_follows(m);
                    (0..JINGLE_SLOTS).for_each(|s| self.fade(m, s, false));
                }
            }
            Command::SetSlotGain { module, slot, gain } => {
                if let Some(v) = self.modules[module].voices[slot].as_mut() {
                    v.gain = gain;
                }
            }
            Command::SetModuleGain { module, gain } => self.modules[module].gain = gain,
            Command::SetModuleOutput { module, pair } => {
                let st = &mut self.modules[module];
                if pair != st.pair {
                    // Un fondu d'aiguillage déjà en cours se termine net (10 ms au plus).
                    st.prev_pair = st.pair;
                    st.pair = pair;
                    st.route_left = if st.is_idle() { 0 } else { ramps.stop };
                }
            }
            Command::SetSlotEq { module, slot, eq } => self.modules[module].slot_eq[slot].set(eq),
            Command::SetModuleEq { module, eq } => self.modules[module].eq.set(eq),
            Command::SetPaused { module, slot, paused } => {
                if let Some(v) = self.modules[module].voices[slot].as_mut() {
                    v.paused = paused;
                }
            }
            Command::QueueNext { module, next } => {
                let old = std::mem::replace(&mut self.modules[module].next, next);
                if let Some(q) = old {
                    self.dispose(q.buffer);
                }
            }
        }
    }

    fn clear_follows(&mut self, module: usize) {
        for slot in 0..SLOTS {
            if let Some(f) = self.modules[module].follow[slot].take() {
                self.dispose(f.buffer);
            }
        }
    }

    fn clear_next(&mut self, module: usize) {
        if let Some(q) = self.modules[module].next.take() {
            self.dispose(q.buffer);
        }
    }

    /// Arrêt en fondu. Un son en boucle n'a pas d'autre fin que l'arrêt : une fois dans la
    /// boucle, son fondu de sortie est appliqué en entier (`own_fade`). Sinon, et pour les arrêts groupés (module,
    /// tout arrêter, Échap), le fondu de sortie est borné entre 10 ms et 1 s pour rester franc.
    fn fade(&mut self, module: usize, slot: usize, own_fade: bool) {
        let frames = self.ramps.stop;
        if let Some(v) = self.modules[module].voices[slot].as_mut() {
            if v.fade_left.is_none() {
                // Son en boucle arrêté pendant son intro (avant d'avoir atteint la boucle) :
                // arrêt immédiat, avec le seul micro-fondu anti-clic.
                let intro = v.region.looping.is_some_and(|(ls, _)| !v.wrapped && v.pos < ls);
                let full = own_fade && v.region.looping.is_some();
                v.fade_len = if intro {
                    frames
                } else if full {
                    v.region.fade_out
                } else {
                    v.region.fade_out.min(frames * 100)
                }
                .max(frames);
                v.fade_left = Some(v.fade_len);
            }
        }
    }

    /// Fondu enchaîné : quand le morceau en cours arrive à `crossfade` images de sa fin,
    /// le suivant démarre sur l'autre emplacement en fondu d'entrée.
    fn start_crossfade(&mut self, m: usize) {
        let ramps = self.ramps;
        let st = &mut self.modules[m];
        let Some(xfade) = st.next.as_ref().map(|q| q.crossfade).filter(|&x| x > 0) else { return };
        for (i, &slot) in PLAYLIST_SLOTS.iter().enumerate() {
            let other = PLAYLIST_SLOTS[1 - i];
            let st = &mut self.modules[m];
            let Some(v) = st.voices[slot].as_mut() else { continue };
            if v.fade_left.is_some() || v.paused {
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
            self.retire(m, other, EqCoeffs::flat());
            self.modules[m].voices[other] = Some(Voice::new(id, q.buffer, q.gain, region, ramps));
            self.telemetry.len[m][other].store(len as u64, Ordering::Relaxed);
            self.telemetry.pos[m][other].store(0, Ordering::Relaxed);
            let _ = self.events.try_send(Event::Advanced { module: m, id, slot: other });
            return;
        }
    }

    /// Remplit `out` (entrelacé, `channels` canaux).
    pub fn process(&mut self, out: &mut [f32]) {
        self.telemetry.callbacks.fetch_add(1, Ordering::Relaxed);
        while let Ok(cmd) = self.commands.try_recv() {
            self.handle(cmd);
        }
        out.fill(0.0);
        let ch = self.channels;
        let frames = out.len() / ch;
        if self.bus.len() < frames * 2 {
            self.bus.resize(frames * 2, 0.0);
            self.voice_bus.resize(frames * 2, 0.0);
            self.eq_scratch.resize(frames * 2, 0.0);
        }
        let ramps = self.ramps;
        let smooth = self.smooth;

        for m in 0..MAX_MODULES {
            let st = &mut self.modules[m];
            if st.is_idle() {
                // Laisse le gain rejoindre sa cible pour éviter un saut au prochain départ.
                st.current_gain = st.gain;
                st.route_left = 0;
                st.eq.reset();
                continue;
            }
            self.start_crossfade(m);
            let bus = &mut self.bus[..frames * 2];
            let voice_bus = &mut self.voice_bus[..frames * 2];
            let scratch = &mut self.eq_scratch[..frames * 2];
            bus.fill(0.0);

            // Voix de traîne : fin de fondu des sons interrompus.
            for t in 0..TAILS {
                let st = &mut self.modules[m];
                let Some(tail) = st.tails[t].as_mut() else { continue };
                let bypass = tail.eq.is_bypassed();
                let target: &mut [f32] = if bypass { &mut *bus } else { voice_bus.fill(0.0); &mut *voice_bus };
                let done = render_voice(&mut tail.voice, target, 0, frames, smooth).is_some();
                if !bypass {
                    tail.eq.process(voice_bus, scratch);
                    bus.iter_mut().zip(voice_bus.iter()).for_each(|(b, x)| *b += *x);
                }
                if done {
                    if let Some(old) = st.tails[t].take() {
                        let _ = self.garbage.try_send(old.voice.buffer);
                    }
                }
            }

            // Deux passages : un son enchaîné à l'image près sur un emplacement déjà rendu
            // dans ce bloc y est rendu au second passage, après son silence de départ.
            let mut revisit = [false; SLOTS];
            for pass in 0..2 {
                if pass == 1 && !revisit.iter().any(|r| *r) {
                    break;
                }
                for slot in 0..SLOTS {
                    if pass == 1 && !revisit[slot] {
                        continue;
                    }
                    let mut ended = false;
                    let mut natural_end = false;
                    let mut ended_at = 0;
                    let mut advanced = false;
                    let ended_id;
                    {
                        let st = &mut self.modules[m];
                        let Some(v) = st.voices[slot].as_mut() else { continue };
                        ended_id = v.id;
                        // Avec un égaliseur, la voix est rendue à part, filtrée, puis ajoutée au bus.
                        let has_eq = !st.slot_eq[slot].is_bypassed();
                        let target: &mut [f32] = if has_eq {
                            voice_bus.fill(0.0);
                            &mut *voice_bus
                        } else {
                            &mut *bus
                        };
                        let mut from = std::mem::take(&mut v.delay).min(frames);
                        while let Some(stop_at) = render_voice(v, target, from, frames, smooth) {
                            // Fin naturelle d'un morceau de playlist : on enchaîne sur la même
                            // image si un suivant est prêt, sinon la voix se termine.
                            let natural = is_playlist_slot(slot) && v.fade_left.is_none();
                            match st.next.take().filter(|_| natural) {
                                Some(q) => {
                                    let region = Region::full(&q.buffer);
                                    let old = std::mem::replace(v, Voice::new(q.id, q.buffer, q.gain, region, ramps));
                                    // Pause demandée juste avant la fin : le suivant reste en pause.
                                    (v.paused, v.pause_gain) = (old.paused, old.pause_gain);
                                    let _ = self.garbage.try_send(old.buffer);
                                    advanced = true;
                                    from = stop_at;
                                }
                                None => {
                                    ended = true;
                                    natural_end = v.fade_left.is_none();
                                    ended_at = stop_at;
                                    break;
                                }
                            }
                        }
                        if has_eq {
                            st.slot_eq[slot].process(voice_bus, scratch);
                            bus.iter_mut().zip(voice_bus.iter()).for_each(|(b, x)| *b += *x);
                        }
                    }
                    let tel = &self.telemetry;
                    if ended {
                        let st = &mut self.modules[m];
                        if let Some(old) = st.voices[slot].take() {
                            let _ = self.garbage.try_send(old.buffer);
                        }
                        st.slot_eq[slot].reset();
                        tel.pos[m][slot].store(NOT_PLAYING, Ordering::Relaxed);
                        tel.fading[m][slot].store(false, Ordering::Relaxed);
                        // Son prévu à la fin de celui-ci : il part sur la même image.
                        let mut follow = None;
                        match st.follow[slot].take() {
                            Some(f) if natural_end => {
                                let (ns, nid) = (f.slot, f.id);
                                start_voice(st, tel, m, f, ramps, &self.garbage);
                                // Rendu plus loin dans ce bloc (ou au second passage) : départ à l'image de fin.
                                let later = ns > slot || pass == 0;
                                if let Some(v) = st.voices[ns].as_mut() {
                                    v.delay = if later { ended_at } else { 0 };
                                }
                                if (pass == 0 && ns < slot) || (pass == 1 && ns > slot) {
                                    revisit[ns] = true;
                                }
                                follow = Some((ns, nid));
                            }
                            Some(f) => {
                                let _ = self.garbage.try_send(f.buffer);
                            }
                            None => {}
                        }
                        let _ = self.events.try_send(Event::Ended { module: m, slot, id: ended_id, follow });
                    } else if let Some(v) = self.modules[m].voices[slot].as_ref() {
                        if advanced {
                            tel.len[m][slot].store(v.buffer.frames as u64, Ordering::Relaxed);
                            let _ = self.events.try_send(Event::Advanced { module: m, id: v.id, slot });
                        }
                        tel.pos[m][slot].store(v.pos as u64, Ordering::Relaxed);
                        tel.fading[m][slot].store(v.is_fading_out(ramps.stop), Ordering::Relaxed);
                    }
                }
            }

            // Égaliseur et volume du module, crêtes, puis écriture sur la paire de sorties.
            let st = &mut self.modules[m];
            st.eq.process(bus, scratch);
            let pair_base = |pair: usize| if pair * 2 + 1 < ch { pair * 2 } else { 0 };
            let base = pair_base(st.pair);
            let prev = pair_base(st.prev_pair);
            let route_len = ramps.stop as f32;
            let mut peak = [0f32; 2];
            for f in 0..frames {
                smooth_towards(&mut st.current_gain, st.gain, smooth);
                let l = bus[f * 2] * st.current_gain;
                let r = bus[f * 2 + 1] * st.current_gain;
                peak[0] = peak[0].max(l.abs());
                peak[1] = peak[1].max(r.abs());
                if ch == 1 {
                    out[f] += (l + r) * 0.5;
                } else if st.route_left > 0 && prev != base {
                    // Changement de sortie : l'ancienne paire s'éteint pendant que la nouvelle monte.
                    let t = st.route_left as f32 / route_len;
                    st.route_left -= 1;
                    out[f * ch + prev] += l * t;
                    out[f * ch + prev + 1] += r * t;
                    out[f * ch + base] += l * (1.0 - t);
                    out[f * ch + base + 1] += r * (1.0 - t);
                } else {
                    st.route_left = 0;
                    out[f * ch + base] += l;
                    out[f * ch + base + 1] += r;
                }
            }
            for (c, p) in peak.iter().enumerate() {
                self.telemetry.peak[m][c].fetch_max(p.to_bits(), Ordering::Relaxed);
            }
        }

        if let Some(lim) = self.limiter.as_mut() {
            lim.process(out);
        }
    }
}

/// Mixe une voix dans le bus stéréo à partir de l'image `from`.
/// Renvoie l'image où la voix s'est terminée, ou `None` si elle continue.
/// Le son de l'emplacement s'éteint en fondu court sur une voix de traîne ; son égaliseur
/// passe au réglage `new_eq` et le son qui devait suivre est abandonné.
fn retire(st: &mut ModuleState, slot: usize, new_eq: EqCoeffs, ramps: Ramps, garbage: &Sender<Arc<AudioBuffer>>) {
    if let Some(f) = st.follow[slot].take() {
        let _ = garbage.try_send(f.buffer);
    }
    let eq = std::mem::replace(&mut st.slot_eq[slot], EqProcessor::new(new_eq, ramps.eq_xfade));
    let Some(mut voice) = st.voices[slot].take() else { return };
    voice.fade_within(ramps.stop);
    let tail = Tail { voice, eq };
    // Plus de place : on remplace la traîne la plus avancée dans son fondu.
    let i = st.tails.iter().position(|t| t.is_none()).unwrap_or_else(|| {
        (0..TAILS).min_by_key(|&i| st.tails[i].as_ref().map_or(0, |t| t.voice.fade_left.unwrap_or(0))).unwrap_or(0)
    });
    if let Some(old) = st.tails[i].replace(tail) {
        let _ = garbage.try_send(old.voice.buffer);
    }
}

/// Lance un son sur un emplacement (l'ancien s'éteint en fondu court).
fn start_voice(st: &mut ModuleState, tel: &Telemetry, m: usize, f: FollowOn, ramps: Ramps, garbage: &Sender<Arc<AudioBuffer>>) {
    tel.len[m][f.slot].store(f.buffer.frames as u64, Ordering::Relaxed);
    tel.pos[m][f.slot].store(f.region.start.min(f.buffer.frames) as u64, Ordering::Relaxed);
    if st.is_idle() {
        // Premier son du module : le volume réglé juste avant s'applique d'emblée.
        st.current_gain = st.gain;
        st.route_left = 0;
    }
    retire(st, f.slot, f.eq, ramps, garbage);
    st.voices[f.slot] = Some(Voice::new(f.id, f.buffer, f.gain, f.region, ramps));
}

fn render_voice(v: &mut Voice, bus: &mut [f32], from: usize, frames: usize, smooth: f32) -> Option<usize> {
    for f in from..frames {
        // Micro-fondu de déplacement : descente, saut, remontée.
        let mut dip = 1.0;
        if let Some(target) = v.seek {
            if v.seek_left == 0 {
                v.pos = target;
                v.seek = None;
                v.seek_in = v.seek_len;
            } else {
                dip = v.seek_left as f32 / v.seek_len as f32;
                v.seek_left -= 1;
            }
        }
        if v.seek_in > 0 {
            dip = 1.0 - v.seek_in as f32 / v.seek_len as f32;
            v.seek_in -= 1;
        }
        // La boucle continue aussi pendant le fondu d'arrêt : le son s'éteint sans
        // jamais buter sur la fin du fichier.
        if let Some((ls, le)) = v.region.looping {
            if v.pos >= le {
                v.pos = ls;
                v.wrapped = true;
            }
        }
        if v.pos >= v.region.end {
            return Some(f);
        }
        if v.paused {
            v.pause_gain = (v.pause_gain - v.pause_step).max(0.0);
            if v.pause_gain <= 0.0 {
                // En pause et silencieuse : un arrêt la termine aussitôt, sinon elle attend.
                return v.fade_left.is_some().then_some(f);
            }
        } else if v.pause_gain < 1.0 {
            v.pause_gain = (v.pause_gain + v.pause_step).min(1.0);
        }
        let mut fade = dip * v.pause_gain;
        if v.played < v.region.fade_in {
            fade *= v.played as f32 / v.region.fade_in as f32;
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
        smooth_towards(&mut v.current_gain, v.gain, smooth);
        let g = v.current_gain * fade;
        let (l, r) = v.frame(v.pos);
        bus[f * 2] += l * g;
        bus[f * 2 + 1] += r * g;
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
        // Sans le limiteur (retard de prévision) pour vérifier les positions à l'image près.
        m.limiter = None;
        (m, ctx, erx)
    }

    #[test]
    fn routes_module_to_its_output_pair() {
        let (mut mx, tx, _) = setup(4);
        let buf = ramp(100);
        tx.send(Command::SetModuleOutput { module: 0, pair: 1 }).unwrap();
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region: Region::full(&buf), buffer: buf, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
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
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region: Region::full(&buf), buffer: buf, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        let mut out = vec![0.0; 2 * 50];
        mx.process(&mut out);
        assert!(out[20] > 0.0);
    }

    #[test]
    fn loop_region_repeats() {
        let (mut mx, tx, ev) = setup(2);
        let buf = ramp(100);
        let region = Region { start: 10, end: 100, looping: Some((20, 30)), fade_in: 0, fade_out: 0 };
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region, buffer: buf, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        let mut out = vec![0.0; 2 * 500];
        mx.process(&mut out);
        assert_eq!(out[0], 0.0, "rampe anti-clic : le début est au milieu du fichier");
        assert!((out[3 * 2] - 0.13).abs() < 1e-6, "commence au point de début");
        // Après 10 + 10 images, retour au début de boucle.
        assert!((out[20 * 2] - 0.20).abs() < 1e-6);
        assert!(out.iter().all(|s| *s < 0.30 + 1e-6), "ne dépasse jamais la fin de boucle");
        assert!(ev.try_recv().is_err(), "la boucle ne se termine pas");
    }

    #[test]
    fn seek_jumps_with_micro_fade() {
        let (mut mx, tx, _) = setup(2);
        let buf = ramp(1000);
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region: Region::full(&buf), buffer: buf, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        let mut out = vec![0.0; 2 * 100];
        mx.process(&mut out);
        tx.send(Command::Seek { module: 0, slot: 0, frame: 800 }).unwrap();
        mx.process(&mut out);
        // Sans fondu, on sauterait de 0,1 à 0,8 d'un coup.
        let steps = out.chunks(2).map(|f| f[0]).collect::<Vec<_>>();
        let max_step = steps.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(max_step < 0.1, "pas de saut : {max_step}");
        assert_eq!(steps[10], 0.0, "le saut se fait au silence");
        assert!((steps[30] - 0.82).abs() < 1e-6, "lecture reprise à la cible : {}", steps[30]);
    }

    #[test]
    fn gapless_playlist_advance() {
        let (mut mx, tx, ev) = setup(2);
        let a = ramp(10);
        let b = ramp(10);
        let slot = PLAYLIST_SLOTS[0];
        tx.send(Command::Play { module: 0, slot, id: 1, region: Region::full(&a), buffer: a, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        tx.send(Command::QueueNext { module: 0, next: Some(Queued { buffer: b, gain: 1.0, id: 2, crossfade: 0 }) }).unwrap();
        let mut out = vec![0.0; 2 * 15];
        mx.process(&mut out);
        assert!(matches!(ev.try_recv(), Ok(Event::Advanced { id: 2, .. })));
        // Le second morceau démarre immédiatement après le premier.
        assert_eq!(out[10 * 2], 0.0);
        assert!((out[11 * 2] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn follow_on_starts_on_the_same_frame() {
        // Le cue suivant part à l'image qui suit la fin du premier, quel que soit l'ordre
        // des emplacements (avant ou après dans le bloc).
        for (first, second) in [(0usize, 3usize), (5, 1)] {
            let (mut mx, tx, ev) = setup(2);
            let a = Arc::new(AudioBuffer::from_planar(vec![0.5; 30], vec![0.5; 30], 1000));
            let b = Arc::new(AudioBuffer::from_planar(vec![0.25; 30], vec![0.25; 30], 1000));
            tx.send(Command::Play { module: 0, slot: first, id: 1, region: Region::full(&a), buffer: a, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
            let next = FollowOn { slot: second, id: 2, region: Region::full(&b), buffer: b, gain: 1.0, eq: EqCoeffs::flat() };
            tx.send(Command::PlayAfter { module: 0, after_slot: first, after_id: 1, next }).unwrap();
            let mut out = vec![0.0; 2 * 64];
            mx.process(&mut out);
            assert!(matches!(ev.try_recv(), Ok(Event::Ended { id: 1, follow: Some((s, 2)), .. }) if s == second));
            let l: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
            assert!((l[29] - 0.5).abs() < 1e-6, "dernière image du premier ({first} → {second})");
            assert!((l[30] - 0.25).abs() < 1e-6, "première image du second, sans blanc ({first} → {second}) : {}", l[30]);
            assert!((l[59] - 0.25).abs() < 1e-6);
            assert_eq!(l[60], 0.0);
        }
    }

    #[test]
    fn stopped_voice_does_not_follow() {
        let (mut mx, tx, ev) = setup(2);
        let a = Arc::new(AudioBuffer::from_planar(vec![0.5; 300], vec![0.5; 300], 1000));
        let b = Arc::new(AudioBuffer::from_planar(vec![0.25; 30], vec![0.25; 30], 1000));
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region: Region::full(&a), buffer: a, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        let next = FollowOn { slot: 1, id: 2, region: Region::full(&b), buffer: b, gain: 1.0, eq: EqCoeffs::flat() };
        tx.send(Command::PlayAfter { module: 0, after_slot: 0, after_id: 1, next }).unwrap();
        let mut out = vec![0.0; 2 * 64];
        mx.process(&mut out);
        tx.send(Command::Stop { module: 0, slot: 0 }).unwrap();
        mx.process(&mut out);
        mx.process(&mut out);
        assert!(matches!(ev.try_recv(), Ok(Event::Ended { id: 1, follow: None, .. })));
        assert!(mx.telemetry.position(0, 1).is_none(), "le suivant n'est pas parti");
        // Une voix déjà terminée n'accepte pas de suite.
        let b = Arc::new(AudioBuffer::from_planar(vec![0.25; 30], vec![0.25; 30], 1000));
        let next = FollowOn { slot: 1, id: 3, region: Region::full(&b), buffer: b, gain: 1.0, eq: EqCoeffs::flat() };
        tx.send(Command::PlayAfter { module: 0, after_slot: 0, after_id: 1, next }).unwrap();
        mx.process(&mut out);
        assert!(mx.modules[0].follow.iter().all(|f| f.is_none()));
    }

    #[test]
    fn crossfade_overlaps_tracks() {
        let (mut mx, tx, ev) = setup(2);
        let a = Arc::new(AudioBuffer::from_planar(vec![0.5; 100], vec![0.5; 100], 1000));
        let b = Arc::new(AudioBuffer::from_planar(vec![0.5; 100], vec![0.5; 100], 1000));
        let slot = PLAYLIST_SLOTS[0];
        tx.send(Command::Play { module: 0, slot, id: 1, region: Region::full(&a), buffer: a, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
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
    fn slot_and_module_eq_are_applied() {
        use crate::audio::eq::EqSettings;
        let mut eq = EqSettings::default();
        eq.low_cut.on = true;
        eq.low_cut.freq = 50.0;
        // Signal continu : un coupe-bas le fait tendre vers 0.
        let buf = Arc::new(AudioBuffer::from_planar(vec![0.5; 1000], vec![0.5; 1000], 1000));
        let region = Region::full(&buf);
        for on_module in [false, true] {
            let (mut mx, tx, _) = setup(2);
            let slot_eq = if on_module { EqCoeffs::flat() } else { eq.coeffs(1000) };
            if on_module {
                tx.send(Command::SetModuleEq { module: 0, eq: eq.coeffs(1000) }).unwrap();
            }
            tx.send(Command::Play { module: 0, slot: 0, id: 1, region, buffer: buf.clone(), gain: 1.0, eq: slot_eq }).unwrap();
            let mut out = vec![0.0; 2 * 500];
            mx.process(&mut out);
            assert!(out[0] > 0.3, "la transitoire passe");
            assert!(out[499 * 2].abs() < 0.01, "continu retiré (EQ du module : {on_module}) : {}", out[499 * 2]);
        }
    }

    #[test]
    fn pause_freezes_and_resumes_without_click() {
        let (mut mx, tx, ev) = setup(2);
        let buf = Arc::new(AudioBuffer::from_planar(vec![1.0; 1000], vec![1.0; 1000], 1000));
        let tel = mx.telemetry.clone();
        let slot = PLAYLIST_SLOTS[0];
        tx.send(Command::Play { module: 0, slot, id: 1, region: Region::full(&buf), buffer: buf, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        let mut out = vec![0.0; 2 * 100];
        mx.process(&mut out);
        tx.send(Command::SetPaused { module: 0, slot, paused: true }).unwrap();
        mx.process(&mut out);
        // Fondu de 10 images (10 ms à 1 kHz) : décroissance progressive, puis silence.
        assert!(out[0] < 1.0 && out[0] > 0.8, "début du fondu : {}", out[0]);
        assert!(out.windows(2).step_by(2).all(|w| w[1] <= w[0] + 1e-6), "décroissance monotone");
        assert!(out[20 * 2..].iter().all(|s| *s == 0.0), "silence en pause");
        let frozen = tel.position(0, slot).unwrap().0;
        mx.process(&mut out);
        assert_eq!(tel.position(0, slot).unwrap().0, frozen, "la tête ne bouge plus");
        assert!(ev.try_recv().is_err(), "la voix ne se termine pas");
        tx.send(Command::SetPaused { module: 0, slot, paused: false }).unwrap();
        mx.process(&mut out);
        assert!(out[0] > 0.0 && out[0] < 0.2, "reprise en fondu : {}", out[0]);
        assert!((out[50 * 2] - 1.0).abs() < 1e-6);
        assert_eq!(tel.position(0, slot).unwrap().0, frozen + 100, "reprend au même endroit");
        // Arrêt pendant la pause : la voix se termine.
        tx.send(Command::SetPaused { module: 0, slot, paused: true }).unwrap();
        mx.process(&mut out);
        tx.send(Command::Stop { module: 0, slot }).unwrap();
        mx.process(&mut out);
        assert!(matches!(ev.try_recv(), Ok(Event::Ended { id: 1, .. })));
    }

    #[test]
    fn fade_in_and_out() {
        let (mut mx, tx, _) = setup(2);
        let buf = Arc::new(AudioBuffer::from_planar(vec![1.0; 100], vec![1.0; 100], 1000));
        let region = Region { start: 0, end: 100, looping: None, fade_in: 10, fade_out: 20 };
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region, buffer: buf, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        let mut out = vec![0.0; 2 * 100];
        mx.process(&mut out);
        assert_eq!(out[0], 0.0);
        assert!((out[5 * 2] - 0.5).abs() < 1e-6);
        assert!((out[50 * 2] - 1.0).abs() < 1e-6);
        assert!((out[90 * 2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn telemetry_flags_audible_fade_outs() {
        let (mut mx, tx, _) = setup(2);
        let tel = mx.telemetry.clone();
        let buf = ramp(1000);
        // Son joué une fois : en fondu dès qu'il entre dans son fondu de fin.
        let region = Region { start: 0, end: 1000, looping: None, fade_in: 0, fade_out: 200 };
        tx.send(Command::Play { module: 0, slot: 0, id: 1, region, buffer: buf.clone(), gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        let mut out = vec![0.0; 2 * 500];
        mx.process(&mut out);
        assert!(!tel.is_fading(0, 0));
        mx.process(&mut out[..2 * 350]);
        assert!(tel.is_fading(0, 0));

        // Boucle arrêtée après son intro : son fondu de sortie entier est signalé.
        let region = Region { start: 0, end: 1000, looping: Some((100, 900)), fade_in: 0, fade_out: 300 };
        tx.send(Command::Play { module: 0, slot: 1, id: 2, region, buffer: buf.clone(), gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        mx.process(&mut out[..2 * 200]);
        assert!(!tel.is_fading(0, 1));
        tx.send(Command::Stop { module: 0, slot: 1 }).unwrap();
        mx.process(&mut out[..2 * 10]);
        assert!(tel.is_fading(0, 1));

        // Arrêt pendant l'intro : simple micro-fondu, pas de repère.
        let region = Region { start: 0, end: 1000, looping: Some((500, 900)), fade_in: 0, fade_out: 300 };
        tx.send(Command::Play { module: 0, slot: 2, id: 3, region, buffer: buf, gain: 1.0, eq: EqCoeffs::flat() }).unwrap();
        mx.process(&mut out[..2 * 100]);
        tx.send(Command::Stop { module: 0, slot: 2 }).unwrap();
        mx.process(&mut out[..2 * 2]);
        assert!(!tel.is_fading(0, 2));
    }
}
