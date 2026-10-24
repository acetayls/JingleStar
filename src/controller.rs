//! Logique de l'application, appelée depuis le thread de l'interface :
//! projet, chargement des sons en arrière-plan, déclenchement des jingles et
//! enchaînement des playlists.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::audio::buffer::{db_to_gain, AudioBuffer};
use crate::audio::decode::{analyze_file, decode_file};
use crate::audio::loudness::{analyze_interleaved, Loudness, LoudnessCache};
use crate::audio::engine::{AudioEngine, DeviceInfo};
use crate::audio::eq::{EqCoeffs, EqSettings};
use crate::audio::mixer::{is_playlist_slot, Command, Event, Queued, Region, MAX_MODULES, PLAYLIST_SLOTS};
use crate::i18n::{set_lang, tr, Lang};
use crate::midi::MidiListener;
use crate::remote::{Flag, InputEvent, Mapping};
use crate::osc::{self, OscCommand, OscServer};
use crate::project::{
    autosave_path, loudness_cache_path, AudioSettings, JingleSlot, MidiSettings, ModuleConfig, ModuleMode, OscSettings, Project, Transition, DEFAULT_COLORS,
};

#[derive(Debug, Clone, Copy)]
enum LoadKind {
    Jingle,
    /// `start` : à lancer dès que prêt ; sinon à mettre en file pour l'enchaînement.
    Playlist { module: usize, index: usize, generation: u64, start: bool },
}

struct LoadJob {
    path: PathBuf,
    rate: u32,
    kind: LoadKind,
    /// Mesure aussi le niveau du son décodé (morceau lancé avant la fin de son analyse).
    analyze: bool,
}

struct LoadDone {
    path: PathBuf,
    rate: u32,
    kind: LoadKind,
    result: Result<Arc<AudioBuffer>, String>,
    loudness: Option<Loudness>,
}

struct AnalysisDone {
    path: PathBuf,
    result: Result<Loudness, String>,
}

/// Mesure du niveau d'un morceau de playlist.
#[derive(Debug, Clone)]
pub enum Analysis {
    Pending,
    Done(Loudness),
    Failed(String),
}

/// Gain d'équilibrage d'un morceau, pour l'affichage.
#[derive(Debug, Clone)]
pub enum TrackGain {
    /// Normalisation désactivée.
    Off,
    Pending,
    /// Gain appliqué (dB), réduit ou non par la limite de crête vraie, et la mesure.
    Ready { db: f32, limited: bool, loudness: Loudness },
    /// Son muet ou trop court : pas de gain calculable.
    Unmeasurable(Loudness),
    Failed(String),
}

impl TrackGain {
    /// Gain linéaire à appliquer à la lecture.
    pub fn linear(&self) -> f32 {
        match self {
            TrackGain::Ready { db, .. } => 10f32.powf(db / 20.0),
            _ => 1.0,
        }
    }
}

/// État de lecture d'une playlist (non sauvegardé).
#[derive(Debug, Default, Clone)]
pub struct PlaylistRuntime {
    pub current: Option<usize>,
    pub playing: bool,
    /// Identifiant de la voix en cours dans le mixeur, et son emplacement.
    voice: Option<u64>,
    pub slot: usize,
    /// Morceaux déjà joués dans ce cycle (lecture aléatoire).
    played: HashSet<usize>,
    /// Morceau en file dans le mixeur (index, identifiant de voix).
    queued: Option<(usize, u64)>,
    /// Un chargement est en cours pour cette playlist.
    loading: bool,
    generation: u64,
    pub duration: Option<f64>,
}

pub struct Controller {
    pub project: Project,
    pub engine: AudioEngine,
    pub device: Option<DeviceInfo>,
    pub device_error: Option<String>,
    pub status: Option<String>,
    pub playlists: [PlaylistRuntime; MAX_MODULES],
    /// Sons de jingles décodés au taux du périphérique.
    cache: HashMap<PathBuf, Arc<AudioBuffer>>,
    loading: HashSet<PathBuf>,
    pub load_errors: HashMap<PathBuf, String>,
    /// Jingles cliqués avant la fin de leur chargement.
    pending_play: HashSet<(usize, usize)>,
    jobs: Sender<LoadJob>,
    done: Receiver<LoadDone>,
    next_voice: u64,
    dirty: bool,
    last_change: Instant,
    pub project_path: Option<PathBuf>,
    keep_awake: Option<keepawake::KeepAwake>,
    keep_awake_failed: Option<Instant>,
    rng: u64,
    repaint: Arc<dyn Fn() + Send + Sync>,
    /// Mesures de niveau des morceaux de playlist, et leur cache disque.
    analysis: HashMap<PathBuf, Analysis>,
    loudness_cache: LoudnessCache,
    cache_dirty: bool,
    last_cache_save: Instant,
    analysis_jobs: Sender<PathBuf>,
    analysis_done: Receiver<AnalysisDone>,
    /// Pilotage OSC, s'il est activé dans les préférences.
    pub osc: Option<OscServer>,
    last_osc_publish: Instant,
    /// Entrée MIDI ouverte, ou l'erreur d'ouverture.
    pub midi: Option<MidiListener>,
    pub midi_error: Option<String>,
    midi_retry: Instant,
    /// Correspondance en attente d'apprentissage.
    pub learn: Option<usize>,
    /// Dernier message reçu d'une télécommande, pour les préférences.
    pub last_input: Option<String>,
    /// Dernier déclenchement de chaque jingle, pour le délai anti double-GO.
    last_trigger: HashMap<(usize, usize), Instant>,
}

impl Controller {
    pub fn new(repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let project = autosave_path()
            .filter(|p| p.exists())
            .and_then(|p| Project::load(&p).ok())
            .unwrap_or_default();
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(repaint);
        let (jobs, done) = spawn_loaders(repaint.clone());
        let (analysis_jobs, analysis_done) = spawn_analyzers(repaint.clone());
        let mut c = Self {
            project,
            engine: AudioEngine::new(),
            device: None,
            device_error: None,
            status: None,
            playlists: Default::default(),
            cache: HashMap::new(),
            loading: HashSet::new(),
            load_errors: HashMap::new(),
            pending_play: HashSet::new(),
            jobs,
            done,
            next_voice: 1,
            dirty: false,
            last_change: Instant::now(),
            project_path: None,
            keep_awake: None,
            keep_awake_failed: None,
            rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64 | 1)
                .unwrap_or(0x9E37_79B9_7F4A_7C15),
            repaint,
            analysis: HashMap::new(),
            loudness_cache: loudness_cache_path().map(|p| LoudnessCache::load(&p)).unwrap_or_default(),
            cache_dirty: false,
            last_cache_save: Instant::now(),
            analysis_jobs,
            analysis_done,
            osc: None,
            last_osc_publish: Instant::now(),
            midi: None,
            midi_error: None,
            midi_retry: Instant::now(),
            learn: None,
            last_input: None,
            last_trigger: HashMap::new(),
        };
        for rt in c.playlists.iter_mut() {
            rt.slot = PLAYLIST_SLOTS[0];
        }
        set_lang(c.project.preferences.language);
        c.open_device();
        c.restart_osc();
        c.restart_midi();
        c.analyze_playlists();
        c
    }

    fn rate(&self) -> u32 {
        self.device.as_ref().map(|d| d.sample_rate).unwrap_or(48_000)
    }

    pub fn output_pairs(&self) -> usize {
        self.device.as_ref().map(|d| (d.channels / 2).max(1)).unwrap_or(1)
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_change = Instant::now();
    }

    fn voice_id(&mut self) -> u64 {
        self.next_voice += 1;
        self.next_voice
    }

    // ---------------------------------------------------------------- audio

    /// (Ré)ouvre le périphérique selon les réglages du projet.
    pub fn open_device(&mut self) {
        self.stop_all();
        let a = self.project.audio.clone();
        let old_rate = self.device.as_ref().map(|d| d.sample_rate);
        match self.engine.open(a.host, a.device, a.buffer_size) {
            Ok(info) => {
                self.device = Some(info);
                self.device_error = None;
            }
            Err(e) => {
                self.device = None;
                self.device_error = Some(e);
            }
        }
        if old_rate != Some(self.rate()) {
            self.cache.clear();
            self.loading.clear();
        }
        self.apply_mixer_params();
        self.preload_jingles();
    }

    pub fn set_audio_settings(&mut self, settings: AudioSettings) {
        self.project.audio = settings;
        self.mark_dirty();
        self.open_device();
    }

    fn apply_mixer_params(&self) {
        for (m, cfg) in self.project.modules.iter().enumerate() {
            self.engine.send(Command::SetModuleGain { module: m, gain: db_to_gain(cfg.volume_db) });
            self.engine.send(Command::SetModuleOutput { module: m, pair: cfg.output_pair });
            self.engine.send(Command::SetModuleEq { module: m, eq: cfg.eq.coeffs(self.rate()) });
        }
    }

    fn preload_jingles(&mut self) {
        let paths: Vec<PathBuf> = self
            .project
            .modules
            .iter()
            .flat_map(|m| m.jingles.iter().flatten().map(|j| j.path.clone()))
            .collect();
        for p in paths {
            self.request_jingle_load(&p);
        }
    }

    fn request_jingle_load(&mut self, path: &Path) {
        if self.cache.contains_key(path) || self.loading.contains(path) {
            return;
        }
        self.loading.insert(path.to_path_buf());
        self.load_errors.remove(path);
        let _ = self.jobs.send(LoadJob { path: path.to_path_buf(), rate: self.rate(), kind: LoadKind::Jingle, analyze: false });
    }

    pub fn is_loading(&self, path: &Path) -> bool {
        self.loading.contains(path)
    }

    pub fn buffer_of(&self, path: &Path) -> Option<Arc<AudioBuffer>> {
        self.cache.get(path).cloned()
    }

    pub fn duration_of(&self, path: &Path) -> Option<f64> {
        self.cache.get(path).map(|b| b.duration_secs())
    }

    /// À appeler à chaque image : résultats de chargement, événements du mixeur, sauvegarde.
    pub fn poll(&mut self) {
        while let Ok(done) = self.done.try_recv() {
            self.on_loaded(done);
        }
        while let Ok(done) = self.analysis_done.try_recv() {
            self.on_analyzed(done.path, done.result);
        }
        if self.cache_dirty && self.last_cache_save.elapsed() > Duration::from_secs(3) {
            self.save_loudness_cache();
        }
        while let Ok(ev) = self.engine.events.try_recv() {
            self.on_event(ev);
        }
        while let Ok(err) = self.engine.stream_errors.try_recv() {
            self.device_error = Some(err);
        }
        self.poll_osc();
        self.poll_remote();
        self.update_keep_awake();
        if self.dirty && self.last_change.elapsed() > Duration::from_millis(800) {
            self.dirty = false;
            if let Some(p) = autosave_path() {
                if let Err(e) = self.project.save(&p) {
                    self.status = Some(format!("{} : {e}", tr("Sauvegarde automatique impossible", "Autosave failed")));
                }
            }
        }
    }

    fn on_loaded(&mut self, done: LoadDone) {
        if let Some(l) = done.loudness {
            self.on_analyzed(done.path.clone(), Ok(l));
        }
        if done.rate != self.rate() {
            // Périphérique changé entre-temps : on relance au bon taux.
            if let LoadKind::Jingle = done.kind {
                self.loading.remove(&done.path);
                self.request_jingle_load(&done.path);
            } else if let LoadKind::Playlist { module, index, generation, start } = done.kind {
                if generation == self.playlists[module].generation {
                    self.load_track(module, index, start);
                }
            }
            return;
        }
        match done.kind {
            LoadKind::Jingle => {
                self.loading.remove(&done.path);
                match done.result {
                    Ok(buf) => {
                        self.cache.insert(done.path.clone(), buf);
                        let pending: Vec<_> = self.pending_play.drain().collect();
                        for (m, s) in pending {
                            if self.jingle(m, s).map(|j| j.path == done.path).unwrap_or(false) {
                                self.play_jingle(m, s);
                            } else {
                                self.pending_play.insert((m, s));
                            }
                        }
                    }
                    Err(e) => {
                        self.pending_play.retain(|&(m, s)| {
                            self.project.modules[m].jingles[s].as_ref().map(|j| j.path != done.path).unwrap_or(true)
                        });
                        self.load_errors.insert(done.path, e);
                    }
                }
            }
            LoadKind::Playlist { module, index, generation, start } => {
                let rt = &mut self.playlists[module];
                if generation != rt.generation {
                    return;
                }
                rt.loading = false;
                let buf = match done.result {
                    Ok(b) => b,
                    Err(e) => {
                        self.status = Some(format!(
                            "{} : {} ({e})",
                            tr("Lecture impossible", "Cannot play"),
                            done.path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                        // On saute le morceau illisible.
                        if let Some(next) = self.next_index(module, index) {
                            self.load_track(module, next, start || self.playlists[module].voice.is_none());
                        } else if self.playlists[module].voice.is_none() {
                            self.playlists[module].playing = false;
                        }
                        return;
                    }
                };
                if !rt.playing {
                    return;
                }
                let gain = self.track_gain(module, index).linear();
                let id = self.voice_id();
                let rt = &mut self.playlists[module];
                if start || rt.voice.is_none() {
                    rt.current = Some(index);
                    rt.voice = Some(id);
                    rt.played.insert(index);
                    rt.duration = Some(buf.duration_secs());
                    let region = Region::full(&buf);
                    let slot = rt.slot;
                    self.engine.send(Command::Play { module, slot, id, buffer: buf, gain, region, eq: EqCoeffs::flat() });
                    self.preload_next(module);
                } else {
                    rt.queued = Some((index, id));
                    let pl = &self.project.modules[module].playlist;
                    let crossfade = match pl.transition {
                        Transition::Gapless => 0,
                        Transition::Crossfade => (pl.crossfade_secs.max(0.0) as f64 * buf.sample_rate as f64) as usize,
                    };
                    let next = Queued { buffer: buf, gain, id, crossfade };
                    self.engine.send(Command::QueueNext { module, next: Some(next) });
                }
            }
        }
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Ended { module, slot, id } if is_playlist_slot(slot) => {
                let rt = &mut self.playlists[module];
                if rt.voice != Some(id) {
                    return;
                }
                rt.voice = None;
                // Si le suivant est encore en chargement, il démarrera à son arrivée.
                if !rt.loading {
                    rt.playing = false;
                    rt.duration = None;
                }
            }
            Event::Ended { .. } => {}
            Event::Advanced { module, id, slot } => {
                let rt = &mut self.playlists[module];
                if let Some((index, qid)) = rt.queued.take() {
                    if qid == id {
                        rt.current = Some(index);
                        rt.voice = Some(id);
                        rt.slot = slot;
                        rt.played.insert(index);
                        rt.duration = None;
                        self.preload_next(module);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------- modules

    /// Ajoute un module vierge à la fin. Renvoie son index.
    pub fn add_module(&mut self) -> Option<usize> {
        let n = self.project.module_count;
        if n >= MAX_MODULES {
            return None;
        }
        self.stop_module(n);
        self.project.modules[n] = ModuleConfig::numbered(n);
        self.project.module_count = n + 1;
        self.playlists[n] = PlaylistRuntime { slot: PLAYLIST_SLOTS[0], ..Default::default() };
        self.apply_mixer_params();
        self.mark_dirty();
        Some(n)
    }

    /// Supprime un module ; les suivants remontent d'un rang.
    pub fn remove_module(&mut self, m: usize) {
        let n = self.project.module_count;
        if n <= 1 || m >= n {
            return;
        }
        // Les modules décalés changent d'index dans le mixeur : on les arrête.
        for i in m..n {
            self.stop_module(i);
        }
        self.project.modules.remove(m);
        self.project.modules.push(ModuleConfig::numbered(MAX_MODULES - 1));
        self.project.module_count = n - 1;
        for i in m..MAX_MODULES {
            self.playlists[i] = PlaylistRuntime {
                slot: PLAYLIST_SLOTS[0],
                generation: self.playlists[i].generation + 1,
                ..Default::default()
            };
        }
        self.apply_mixer_params();
        self.mark_dirty();
    }

    pub fn set_module_volume(&mut self, m: usize, db: f32) {
        self.project.modules[m].volume_db = db;
        self.engine.send(Command::SetModuleGain { module: m, gain: db_to_gain(db) });
        self.mark_dirty();
    }

    pub fn set_module_output(&mut self, m: usize, pair: usize) {
        self.project.modules[m].output_pair = pair;
        self.engine.send(Command::SetModuleOutput { module: m, pair });
        self.mark_dirty();
    }

    pub fn set_module_eq(&mut self, m: usize, eq: EqSettings) {
        self.project.modules[m].eq = eq;
        self.engine.send(Command::SetModuleEq { module: m, eq: eq.coeffs(self.rate()) });
        self.mark_dirty();
    }

    /// Taux utilisé pour tracer les courbes d'égaliseur.
    pub fn sample_rate(&self) -> u32 {
        self.rate()
    }

    pub fn set_module_mode(&mut self, m: usize, mode: ModuleMode) {
        if self.project.modules[m].mode != mode {
            self.stop_module(m);
            self.project.modules[m].mode = mode;
            self.mark_dirty();
        }
    }

    pub fn rename_module(&mut self, m: usize, name: String) {
        self.project.modules[m].name = name;
        self.mark_dirty();
    }

    pub fn stop_module(&mut self, m: usize) {
        self.engine.send(Command::StopModule { module: m });
        self.pending_play.retain(|&(pm, _)| pm != m);
        let rt = &mut self.playlists[m];
        rt.generation += 1;
        rt.playing = false;
        rt.loading = false;
        rt.queued = None;
    }

    pub fn stop_all(&mut self) {
        self.engine.send(Command::StopAll);
        for m in 0..MAX_MODULES {
            self.stop_module(m);
        }
    }

    /// Arrête tous les jingles, sans toucher aux playlists.
    pub fn stop_all_jingles(&mut self) {
        for m in 0..MAX_MODULES {
            for slot in 0..crate::audio::mixer::JINGLE_SLOTS {
                self.engine.send(Command::Stop { module: m, slot });
            }
        }
        self.pending_play.clear();
    }

    /// Touche Échap : les jingles, et les playlists si l'option est active.
    pub fn escape(&mut self) {
        if self.project.preferences.escape_stops_playlist {
            self.stop_all();
        } else {
            self.stop_all_jingles();
        }
    }

    pub fn set_escape_stops_playlist(&mut self, on: bool) {
        self.project.preferences.escape_stops_playlist = on;
        self.mark_dirty();
    }

    pub fn set_double_go_ms(&mut self, ms: u32) {
        self.project.preferences.double_go_ms = ms;
        self.mark_dirty();
    }

    // -------------------------------------------------------------- jingles

    pub fn jingle(&self, m: usize, s: usize) -> Option<&JingleSlot> {
        self.project.modules[m].jingles[s].as_ref()
    }

    pub fn is_jingle_playing(&self, m: usize, s: usize) -> bool {
        self.engine.telemetry.position(m, s).is_some()
    }

    /// Clic sur un bouton : lance le son, ou l'arrête s'il joue déjà.
    pub fn trigger_jingle(&mut self, m: usize, s: usize) {
        // Anti double-GO : un second déclenchement trop rapproché est ignoré.
        let guard = Duration::from_millis(self.project.preferences.double_go_ms as u64);
        if self.last_trigger.get(&(m, s)).is_some_and(|t| t.elapsed() < guard) {
            return;
        }
        self.last_trigger.insert((m, s), Instant::now());
        if self.is_jingle_playing(m, s) {
            self.engine.send(Command::Stop { module: m, slot: s });
        } else if self.pending_play.remove(&(m, s)) {
            // Second clic pendant le chargement : on annule.
        } else {
            self.play_jingle(m, s);
        }
    }

    /// Lance le son depuis le début, même s'il joue déjà.
    pub fn start_jingle(&mut self, m: usize, s: usize) {
        self.pending_play.remove(&(m, s));
        self.play_jingle(m, s);
    }

    pub fn stop_jingle(&mut self, m: usize, s: usize) {
        self.pending_play.remove(&(m, s));
        self.engine.send(Command::Stop { module: m, slot: s });
    }

    fn play_jingle(&mut self, m: usize, s: usize) {
        let Some(j) = self.jingle(m, s).cloned() else { return };
        if let Some(buf) = self.cache.get(&j.path).cloned() {
            let id = self.voice_id();
            let region = jingle_region(&j, &buf);
            let eq = j.eq.coeffs(self.rate());
            self.engine.send(Command::Play { module: m, slot: s, id, buffer: buf, gain: db_to_gain(j.volume_db), region, eq });
        } else {
            self.pending_play.insert((m, s));
            self.request_jingle_load(&j.path);
        }
    }

    pub fn assign_jingle(&mut self, m: usize, s: usize, path: PathBuf) {
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let color = self.project.modules[m].jingles[s]
            .as_ref()
            .map(|j| j.color)
            .unwrap_or(DEFAULT_COLORS[s % DEFAULT_COLORS.len()]);
        self.engine.send(Command::Stop { module: m, slot: s });
        self.request_jingle_load(&path);
        self.project.modules[m].jingles[s] = Some(JingleSlot::new(path, name, color));
        self.mark_dirty();
    }

    pub fn clear_jingle(&mut self, m: usize, s: usize) {
        self.engine.send(Command::Stop { module: m, slot: s });
        self.project.modules[m].jingles[s] = None;
        self.mark_dirty();
    }

    pub fn update_jingle(&mut self, m: usize, s: usize, f: impl FnOnce(&mut JingleSlot)) {
        let rate = self.rate();
        if let Some(j) = self.project.modules[m].jingles[s].as_mut() {
            let old_eq = j.eq;
            f(j);
            let gain = db_to_gain(j.volume_db);
            self.engine.send(Command::SetSlotGain { module: m, slot: s, gain });
            if j.eq != old_eq {
                self.engine.send(Command::SetSlotEq { module: m, slot: s, eq: j.eq.coeffs(rate) });
            }
            if let Some(buf) = self.cache.get(&j.path) {
                let region = jingle_region(j, buf);
                self.engine.send(Command::SetRegion { module: m, slot: s, region });
            }
            self.mark_dirty();
        }
    }

    // ------------------------------------------------------------- playlist

    pub fn set_playlist_folder(&mut self, m: usize, folder: PathBuf) {
        self.stop_module(m);
        let pl = &mut self.project.modules[m].playlist;
        pl.folder = Some(folder);
        pl.rescan();
        self.playlists[m].current = None;
        self.mark_dirty();
        self.analyze_playlist(m);
    }

    /// Ajoute des fichiers à la playlist ; un dossier ajoute ses fichiers audio (sous-dossiers compris).
    /// La liste devient une liste libre, sans dossier de référence.
    pub fn add_playlist_files(&mut self, m: usize, paths: Vec<PathBuf>) {
        let mut added = Vec::new();
        for p in paths {
            if p.is_dir() {
                collect_audio(&p, &mut added);
            } else if crate::audio::decode::is_audio_file(&p) {
                added.push(p);
            }
        }
        let pl = &mut self.project.modules[m].playlist;
        pl.folder = None;
        for p in added {
            if !pl.tracks.contains(&p) {
                pl.tracks.push(p);
            }
        }
        self.mark_dirty();
        self.analyze_playlist(m);
    }

    pub fn remove_track(&mut self, m: usize, index: usize) {
        let rt = &self.playlists[m];
        if rt.playing && rt.current == Some(index) {
            self.playlist_stop(m);
        }
        let pl = &mut self.project.modules[m].playlist;
        if index < pl.tracks.len() {
            pl.tracks.remove(index);
            pl.folder = None;
            let rt = &mut self.playlists[m];
            rt.current = rt.current.and_then(|c| match c.cmp(&index) {
                std::cmp::Ordering::Less => Some(c),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(c - 1),
            });
            rt.played.clear();
            self.mark_dirty();
        }
    }

    pub fn clear_playlist(&mut self, m: usize) {
        self.stop_module(m);
        let pl = &mut self.project.modules[m].playlist;
        pl.tracks.clear();
        pl.folder = None;
        self.playlists[m].current = None;
        self.mark_dirty();
    }

    pub fn rescan_playlist(&mut self, m: usize) {
        // Les fichiers ont pu changer : on revérifie les mesures (le cache disque reste valable
        // pour ceux qui n'ont pas bougé).
        for p in &self.project.modules[m].playlist.tracks {
            if !matches!(self.analysis.get(p), Some(Analysis::Pending)) {
                self.analysis.remove(p);
            }
        }
        self.project.modules[m].playlist.rescan();
        self.mark_dirty();
        self.analyze_playlist(m);
    }

    /// Modifie les options de lecture de la playlist (enchaînement, boucle, aléatoire, transition).
    pub fn update_playlist(&mut self, m: usize, f: impl FnOnce(&mut crate::project::PlaylistConfig)) {
        let pl = &mut self.project.modules[m].playlist;
        let old_norm = pl.normalize.clone();
        f(pl);
        let norm_changed = pl.normalize != old_norm;
        self.mark_dirty();
        if norm_changed {
            self.analyze_playlist(m);
            // Le morceau en cours prend tout de suite son nouveau gain (lissé par le mixeur).
            let rt = &self.playlists[m];
            if let (Some(cur), Some(_)) = (rt.current, rt.voice) {
                let gain = self.track_gain(m, cur).linear();
                self.engine.send(Command::SetSlotGain { module: m, slot: rt.slot, gain });
            }
        }
        // Recalcule le morceau en file selon les nouvelles options.
        let rt = &mut self.playlists[m];
        if rt.playing && rt.voice.is_some() {
            rt.generation += 1;
            rt.queued = None;
            rt.loading = false;
            self.engine.send(Command::QueueNext { module: m, next: None });
            self.preload_next(m);
        }
    }

    // -------------------------------------------------------- normalisation

    /// Lance la mesure des morceaux de toutes les playlists dont la normalisation est active.
    fn analyze_playlists(&mut self) {
        for m in 0..self.project.module_count {
            self.analyze_playlist(m);
        }
    }

    /// Mesure en arrière-plan les morceaux pas encore connus (cache disque d'abord).
    fn analyze_playlist(&mut self, m: usize) {
        let pl = &self.project.modules[m].playlist;
        if !pl.normalize.enabled {
            return;
        }
        for path in pl.tracks.clone() {
            if self.analysis.contains_key(&path) {
                continue;
            }
            match self.loudness_cache.get(&path) {
                Some(l) => {
                    self.analysis.insert(path, Analysis::Done(l));
                }
                None => {
                    self.analysis.insert(path.clone(), Analysis::Pending);
                    let _ = self.analysis_jobs.send(path);
                }
            }
        }
    }

    fn on_analyzed(&mut self, path: PathBuf, result: Result<Loudness, String>) {
        match result {
            Ok(l) => {
                self.loudness_cache.insert(&path, l);
                self.cache_dirty = true;
                self.analysis.insert(path, Analysis::Done(l));
            }
            // Une mesure déjà obtenue (au chargement du morceau) prime sur un échec.
            Err(e) => {
                if !matches!(self.analysis.get(&path), Some(Analysis::Done(_))) {
                    self.analysis.insert(path, Analysis::Failed(e));
                }
            }
        }
    }

    fn save_loudness_cache(&mut self) {
        self.cache_dirty = false;
        self.last_cache_save = Instant::now();
        if let Some(p) = loudness_cache_path() {
            if let Err(e) = self.loudness_cache.save(&p) {
                self.status = Some(format!("{} : {e}", tr("Cache des niveaux non enregistré", "Loudness cache not saved")));
            }
        }
    }

    /// Gain d'équilibrage du morceau `index` de la playlist du module `m`.
    pub fn track_gain(&self, m: usize, index: usize) -> TrackGain {
        let pl = &self.project.modules[m].playlist;
        if !pl.normalize.enabled {
            return TrackGain::Off;
        }
        let Some(path) = pl.tracks.get(index) else { return TrackGain::Off };
        match self.analysis.get(path) {
            None | Some(Analysis::Pending) => TrackGain::Pending,
            Some(Analysis::Failed(e)) => TrackGain::Failed(e.clone()),
            Some(Analysis::Done(l)) => match pl.normalize.gain_db(l) {
                Some((db, limited)) => TrackGain::Ready { db, limited, loudness: *l },
                None => TrackGain::Unmeasurable(*l),
            },
        }
    }

    /// Nombre de morceaux encore en cours de mesure dans la playlist du module `m`.
    pub fn pending_analyses(&self, m: usize) -> usize {
        let pl = &self.project.modules[m].playlist;
        if !pl.normalize.enabled {
            return 0;
        }
        pl.tracks.iter().filter(|p| !matches!(self.analysis.get(*p), Some(Analysis::Done(_) | Analysis::Failed(_)))).count()
    }

    fn random(&mut self) -> u64 {
        // xorshift64 : suffisant pour mélanger une playlist.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn next_index(&mut self, m: usize, index: usize) -> Option<usize> {
        let pl = &self.project.modules[m].playlist;
        if !pl.auto_advance || pl.tracks.is_empty() {
            return None;
        }
        let (len, looping) = (pl.tracks.len(), pl.looping);
        if pl.shuffle {
            let rt = &mut self.playlists[m];
            if rt.played.len() >= len {
                if !looping {
                    return None;
                }
                rt.played.clear();
            }
            let left: Vec<usize> =
                (0..len).filter(|i| !rt.played.contains(i) && (len == 1 || *i != index)).collect();
            if left.is_empty() {
                return looping.then_some(index);
            }
            let r = self.random() as usize % left.len();
            return Some(left[r]);
        }
        if index + 1 < len {
            Some(index + 1)
        } else if looping {
            Some(0)
        } else {
            None
        }
    }

    fn load_track(&mut self, m: usize, index: usize, start: bool) {
        let Some(path) = self.project.modules[m].playlist.tracks.get(index).cloned() else { return };
        let analyze = self.project.modules[m].playlist.normalize.enabled
            && !matches!(self.analysis.get(&path), Some(Analysis::Done(_) | Analysis::Failed(_)));
        let rt = &mut self.playlists[m];
        rt.loading = true;
        let kind = LoadKind::Playlist { module: m, index, generation: rt.generation, start };
        let _ = self.jobs.send(LoadJob { path, rate: self.rate(), kind, analyze });
    }

    fn preload_next(&mut self, m: usize) {
        if let Some(cur) = self.playlists[m].current {
            if let Some(next) = self.next_index(m, cur) {
                self.load_track(m, next, false);
            }
        }
    }

    pub fn playlist_play(&mut self, m: usize, index: usize) {
        if index >= self.project.modules[m].playlist.tracks.len() {
            return;
        }
        let rt = &mut self.playlists[m];
        rt.generation += 1;
        if !rt.playing {
            rt.played.clear();
        }
        rt.playing = true;
        rt.queued = None;
        rt.current = Some(index);
        rt.duration = None;
        self.engine.send(Command::QueueNext { module: m, next: None });
        self.load_track(m, index, true);
    }

    /// Lecture : reprend au morceau sélectionné, ou au premier.
    pub fn playlist_go(&mut self, m: usize) {
        let idx = self.playlists[m].current.unwrap_or(0);
        self.playlist_play(m, idx);
    }

    pub fn playlist_stop(&mut self, m: usize) {
        for slot in PLAYLIST_SLOTS {
            self.engine.send(Command::Stop { module: m, slot });
        }
        self.engine.send(Command::QueueNext { module: m, next: None });
        let rt = &mut self.playlists[m];
        rt.generation += 1;
        rt.playing = false;
        rt.loading = false;
        rt.queued = None;
    }

    pub fn playlist_step(&mut self, m: usize, delta: isize) {
        let len = self.project.modules[m].playlist.tracks.len();
        if len == 0 {
            return;
        }
        let cur = self.playlists[m].current.unwrap_or(0) as isize;
        let idx = (cur + delta).rem_euclid(len as isize) as usize;
        if self.playlists[m].playing {
            self.playlist_play(m, idx);
        } else {
            self.playlists[m].current = Some(idx);
        }
    }

    pub fn select_track(&mut self, m: usize, index: usize) {
        if !self.playlists[m].playing {
            self.playlists[m].current = Some(index);
        }
    }

    pub fn playlist_position(&self, m: usize) -> Option<(u64, u64)> {
        let rt = &self.playlists[m];
        rt.voice.and_then(|_| self.engine.telemetry.position(m, rt.slot))
    }

    pub fn anything_playing(&self) -> bool {
        (0..MAX_MODULES).any(|m| (0..crate::audio::mixer::SLOTS).any(|s| self.engine.telemetry.position(m, s).is_some()))
    }

    pub fn set_language(&mut self, lang: Lang) {
        self.project.preferences.language = lang;
        set_lang(lang);
        self.mark_dirty();
    }

    pub fn set_module_width(&mut self, width: Option<f32>) {
        self.project.preferences.module_width = width;
        self.mark_dirty();
    }

    pub fn set_prevent_sleep(&mut self, on: bool) {
        self.project.preferences.prevent_sleep = on;
        self.mark_dirty();
    }

    /// Bloque la mise en veille tant qu'un son joue (si l'option est active).
    fn update_keep_awake(&mut self) {
        let want = self.project.preferences.prevent_sleep && self.anything_playing();
        // Après un échec (pas de D-Bus, etc.), on ne réessaie qu'au bout de 30 s :
        // la tentative est lente et bloquerait l'interface à chaque image.
        let may_retry = self.keep_awake_failed.map(|t| t.elapsed() > Duration::from_secs(30)).unwrap_or(true);
        if want && self.keep_awake.is_none() && may_retry {
            self.keep_awake_failed = Some(Instant::now());
            self.keep_awake = keepawake::Builder::default()
                .idle(true)
                .sleep(true)
                .reason("Lecture audio en cours")
                .app_name("JingleStar")
                .app_reverse_domain("com.acetayls.jinglestar")
                .create()
                .map_err(|e| self.status = Some(format!("{} : {e}", tr("Anti-veille indisponible", "Sleep prevention unavailable"))))
                .ok();
            if self.keep_awake.is_some() {
                self.keep_awake_failed = None;
            }
        } else if !want {
            self.keep_awake = None;
        }
    }

    // ------------------------------------------------------------------ OSC

    pub fn set_osc_settings(&mut self, settings: OscSettings) {
        if self.project.preferences.osc != settings {
            self.project.preferences.osc = settings;
            self.mark_dirty();
            self.restart_osc();
        }
    }

    /// (Re)démarre les serveurs OSC selon les préférences.
    pub fn restart_osc(&mut self) {
        // Libère d'abord les ports de l'ancien serveur.
        self.osc = None;
        let settings = self.project.preferences.osc.clone();
        if !settings.enabled {
            return;
        }
        let server = OscServer::start(settings, self.repaint.clone());
        if let Some(e) = server.startup_errors.first() {
            self.status = Some(e.clone());
        }
        server.publish(self.osc_snapshot());
        self.osc = Some(server);
    }

    fn poll_osc(&mut self) {
        let Some(server) = self.osc.as_ref() else { return };
        let commands: Vec<OscCommand> = server.commands.try_iter().collect();
        if let Some(e) = server.errors.try_iter().last() {
            self.status = Some(format!("OSC : {e}"));
        }
        let changed = !commands.is_empty();
        for cmd in commands {
            self.handle_command("OSC", cmd);
        }
        if changed || self.last_osc_publish.elapsed() > Duration::from_millis(200) {
            self.last_osc_publish = Instant::now();
            let snap = self.osc_snapshot();
            if let Some(server) = self.osc.as_ref() {
                server.publish(snap);
            }
        }
    }

    /// Exécute une commande reçue de l'OSC ou d'une télécommande (`source` sert aux messages).
    fn handle_command(&mut self, source: &str, cmd: OscCommand) {
        let n = self.project.module_count;
        let mode = |c: &Self, m: usize| c.project.modules[m].mode;
        match cmd {
            OscCommand::StopAll => self.stop_all(),
            OscCommand::ModuleStop(m) if m < n => self.stop_module(m),
            OscCommand::ModuleVolume(m, db) if m < n => self.set_module_volume(m, db),
            OscCommand::ModuleMode(m, md) if m < n => self.set_module_mode(m, md),
            OscCommand::JinglePlay(m, s) if m < n && mode(self, m) == ModuleMode::Jingle => self.start_jingle(m, s),
            OscCommand::JingleStop(m, s) if m < n && mode(self, m) == ModuleMode::Jingle => self.stop_jingle(m, s),
            OscCommand::JingleToggle(m, s) if m < n && mode(self, m) == ModuleMode::Jingle => self.trigger_jingle(m, s),
            OscCommand::JingleVolume(m, s, db) if m < n => self.update_jingle(m, s, |j| j.volume_db = db),
            OscCommand::PlaylistPlay(m, track) if m < n && mode(self, m) == ModuleMode::Playlist => match track {
                Some(i) => self.playlist_play(m, i),
                None => self.playlist_go(m),
            },
            OscCommand::PlaylistStop(m) if m < n && mode(self, m) == ModuleMode::Playlist => self.playlist_stop(m),
            OscCommand::PlaylistStep(m, d) if m < n && mode(self, m) == ModuleMode::Playlist => self.playlist_step(m, d),
            OscCommand::PlaylistShuffle(m, on) if m < n => self.update_playlist(m, |p| p.shuffle = on),
            OscCommand::PlaylistLoop(m, on) if m < n => self.update_playlist(m, |p| p.looping = on),
            OscCommand::PlaylistAutoAdvance(m, on) if m < n => self.update_playlist(m, |p| p.auto_advance = on),
            other => self.status = Some(format!("{source} : {} ({other:?})", tr("commande ignorée", "command ignored"))),
        }
    }

    // ------------------------------------------------------ MIDI et console

    pub fn set_midi_settings(&mut self, settings: MidiSettings) {
        if self.project.preferences.midi != settings {
            self.project.preferences.midi = settings;
            self.mark_dirty();
            self.restart_midi();
        }
    }

    /// (Ré)ouvre le port MIDI selon les préférences.
    pub fn restart_midi(&mut self) {
        self.midi = None;
        self.midi_error = None;
        self.midi_retry = Instant::now();
        let settings = self.project.preferences.midi.clone();
        if !settings.enabled {
            return;
        }
        match MidiListener::open(settings.port.as_deref(), self.repaint.clone()) {
            Ok(l) => self.midi = Some(l),
            Err(e) => self.midi_error = Some(e),
        }
    }

    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.project.preferences.mappings.push(mapping);
        self.mark_dirty();
    }

    pub fn update_mapping(&mut self, i: usize, mapping: Mapping) {
        if let Some(m) = self.project.preferences.mappings.get_mut(i) {
            if *m != mapping {
                *m = mapping;
                self.mark_dirty();
            }
        }
    }

    pub fn remove_mapping(&mut self, i: usize) {
        if i < self.project.preferences.mappings.len() {
            self.project.preferences.mappings.remove(i);
            self.learn = match self.learn {
                Some(l) if l == i => None,
                Some(l) if l > i => Some(l - 1),
                other => other,
            };
            self.mark_dirty();
        }
    }

    fn poll_remote(&mut self) {
        let mut events = Vec::new();
        if let Some(l) = self.midi.as_ref() {
            events.extend(l.events.try_iter().map(|e| ("MIDI", e)));
        }
        // Port débranché puis rebranché : on retente régulièrement.
        if self.project.preferences.midi.enabled && self.midi_retry.elapsed() > Duration::from_secs(3) {
            self.midi_retry = Instant::now();
            let ports = crate::midi::port_names();
            match self.midi.as_ref().map(|l| l.port.clone()) {
                Some(port) if !ports.contains(&port) => {
                    self.midi = None;
                    self.midi_error = Some(format!("{} : {port}", tr("port MIDI débranché", "MIDI port unplugged")));
                }
                None if !ports.is_empty() => self.restart_midi(),
                _ => {}
            }
        }
        for (source, ev) in events {
            self.on_input(source, ev);
        }
    }

    fn on_input(&mut self, source: &str, ev: InputEvent) {
        self.last_input = Some(format!("{source} · {}", ev.label()));
        if let Some(i) = self.learn {
            if ev.learnable() {
                if let Some(m) = self.project.preferences.mappings.get(i) {
                    let learned = Mapping { trigger: ev.trigger(), ..m.clone() };
                    self.update_mapping(i, learned);
                }
                self.learn = None;
            }
            return;
        }
        let commands: Vec<OscCommand> = self
            .project
            .preferences
            .mappings
            .iter()
            .filter(|m| m.matches(&ev))
            .filter_map(|m| {
                m.command(&ev, |module, flag| {
                    let pl = &self.project.modules[module].playlist;
                    match flag {
                        Flag::Shuffle => pl.shuffle,
                        Flag::Loop => pl.looping,
                        Flag::AutoAdvance => pl.auto_advance,
                    }
                })
            })
            .collect();
        for cmd in commands {
            self.handle_command(source, cmd);
        }
    }

    /// État courant, publié par OSCQuery.
    pub fn osc_snapshot(&self) -> osc::Snapshot {
        let modules = (0..self.project.module_count)
            .map(|m| {
                let cfg = &self.project.modules[m];
                let rt = &self.playlists[m];
                osc::ModuleState {
                    name: cfg.name.clone(),
                    mode: cfg.mode,
                    volume_db: cfg.volume_db,
                    jingles: cfg
                        .jingles
                        .iter()
                        .enumerate()
                        .map(|(s, j)| {
                            j.as_ref().map(|j| osc::JingleState {
                                name: j.name.clone(),
                                volume_db: j.volume_db,
                                playing: self.is_jingle_playing(m, s),
                            })
                        })
                        .collect(),
                    playlist: osc::PlaylistState {
                        tracks: cfg.playlist.tracks.len(),
                        current: rt.current,
                        playing: rt.playing,
                        shuffle: cfg.playlist.shuffle,
                        looping: cfg.playlist.looping,
                        auto_advance: cfg.playlist.auto_advance,
                    },
                }
            })
            .collect();
        osc::Snapshot { modules }
    }

    // --------------------------------------------------------------- projet

    pub fn open_project(&mut self, path: PathBuf) {
        match Project::load(&path) {
            Ok(p) => {
                self.stop_all();
                let audio_changed = p.audio != self.project.audio;
                let osc_changed = p.preferences.osc != self.project.preferences.osc;
                let midi_changed = p.preferences.midi != self.project.preferences.midi;
                self.project = p;
                set_lang(self.project.preferences.language);
                if osc_changed {
                    self.restart_osc();
                }
                if midi_changed {
                    self.restart_midi();
                }
                self.learn = None;
                self.project_path = Some(path);
                self.playlists = Default::default();
                self.pending_play.clear();
                self.mark_dirty();
                if audio_changed {
                    self.open_device();
                } else {
                    self.apply_mixer_params();
                    self.preload_jingles();
                }
                self.analyze_playlists();
                self.status = Some(tr("Projet ouvert", "Project opened").into());
            }
            Err(e) => self.status = Some(format!("{} : {e}", tr("Ouverture impossible", "Cannot open"))),
        }
    }

    pub fn save_project_as(&mut self, path: PathBuf) {
        match self.project.save(&path) {
            Ok(()) => {
                self.status = Some(format!("{} : {}", tr("Projet enregistré", "Project saved"), path.display()));
                self.project_path = Some(path);
            }
            Err(e) => self.status = Some(format!("{} : {e}", tr("Enregistrement impossible", "Cannot save"))),
        }
    }

    /// Sauvegarde immédiate (à la fermeture).
    pub fn flush(&mut self) {
        if let Some(p) = autosave_path() {
            let _ = self.project.save(&p);
        }
        self.dirty = false;
        if self.cache_dirty {
            self.save_loudness_cache();
        }
    }
}

/// Convertit les points du jingle (secondes) en zone de lecture (images).
pub fn jingle_region(j: &JingleSlot, buf: &AudioBuffer) -> Region {
    let sr = buf.sample_rate as f64;
    let (start, ls, le, end) = j.markers(buf.duration_secs());
    let f = |t: f64| (t * sr).round() as usize;
    Region {
        start: f(start),
        end: f(end),
        looping: j.looping.then(|| (f(ls), f(le))),
        fade_in: f(j.fade_in.max(0.0)),
        fade_out: f(j.fade_out.max(0.0)),
    }
}

/// Fichiers audio d'un dossier et de ses sous-dossiers, triés par nom dans chaque dossier.
fn collect_audio(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    for p in entries {
        if p.is_dir() {
            collect_audio(&p, out);
        } else if crate::audio::decode::is_audio_file(&p) {
            out.push(p);
        }
    }
}

/// Pool de threads de décodage.
fn spawn_loaders(repaint: Arc<dyn Fn() + Send + Sync>) -> (Sender<LoadJob>, Receiver<LoadDone>) {
    let (job_tx, job_rx) = unbounded::<LoadJob>();
    let (done_tx, done_rx) = unbounded::<LoadDone>();
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
    for i in 0..workers {
        let job_rx = job_rx.clone();
        let done_tx = done_tx.clone();
        let repaint = repaint.clone();
        std::thread::Builder::new()
            .name(format!("decoder-{i}"))
            .spawn(move || {
                for job in job_rx {
                    let result = decode_file(&job.path, job.rate).map(Arc::new).map_err(|e| e.to_string());
                    let loudness = match (&result, job.analyze) {
                        (Ok(buf), true) => Some(analyze_interleaved(&buf.samples, buf.sample_rate)),
                        _ => None,
                    };
                    let _ = done_tx.send(LoadDone { path: job.path, rate: job.rate, kind: job.kind, result, loudness });
                    repaint();
                }
            })
            .expect("thread de décodage");
    }
    (job_tx, done_rx)
}

/// Threads de mesure du niveau des morceaux, séparés du décodage pour ne jamais
/// retarder le chargement d'un son à jouer.
fn spawn_analyzers(repaint: Arc<dyn Fn() + Send + Sync>) -> (Sender<PathBuf>, Receiver<AnalysisDone>) {
    let (job_tx, job_rx) = unbounded::<PathBuf>();
    let (done_tx, done_rx) = unbounded::<AnalysisDone>();
    let workers = std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(1).clamp(1, 2);
    for i in 0..workers {
        let job_rx = job_rx.clone();
        let done_tx = done_tx.clone();
        let repaint = repaint.clone();
        std::thread::Builder::new()
            .name(format!("loudness-{i}"))
            .spawn(move || {
                for path in job_rx {
                    let result = analyze_file(&path).map_err(|e| e.to_string());
                    let _ = done_tx.send(AnalysisDone { path, result });
                    repaint();
                }
            })
            .expect("thread de mesure");
    }
    (job_tx, done_rx)
}
