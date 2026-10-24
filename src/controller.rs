//! Logique de l'application, appelée depuis le thread de l'interface :
//! projet, chargement des sons en arrière-plan, déclenchement des jingles et
//! enchaînement des playlists.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::audio::buffer::{db_to_gain, AudioBuffer};
use crate::audio::decode::decode_file;
use crate::audio::engine::{AudioEngine, DeviceInfo};
use crate::audio::eq::{EqCoeffs, EqSettings};
use crate::i18n::{set_lang, tr, Lang};
use crate::audio::mixer::{is_playlist_slot, Command, Event, Queued, Region, MAX_MODULES, PLAYLIST_SLOTS};
use crate::project::{autosave_path, AudioSettings, JingleSlot, ModuleConfig, ModuleMode, Project, Transition, DEFAULT_COLORS};

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
}

struct LoadDone {
    path: PathBuf,
    rate: u32,
    kind: LoadKind,
    result: Result<Arc<AudioBuffer>, String>,
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
    rng: u64,
}

impl Controller {
    pub fn new(repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let project = autosave_path()
            .filter(|p| p.exists())
            .and_then(|p| Project::load(&p).ok())
            .unwrap_or_default();
        let (jobs, done) = spawn_loaders(Arc::new(repaint));
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
            rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64 | 1)
                .unwrap_or(0x9E37_79B9_7F4A_7C15),
        };
        for rt in c.playlists.iter_mut() {
            rt.slot = PLAYLIST_SLOTS[0];
        }
        set_lang(c.project.preferences.language);
        c.open_device();
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
        let _ = self.jobs.send(LoadJob { path: path.to_path_buf(), rate: self.rate(), kind: LoadKind::Jingle });
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
        while let Ok(ev) = self.engine.events.try_recv() {
            self.on_event(ev);
        }
        while let Ok(err) = self.engine.stream_errors.try_recv() {
            self.device_error = Some(err);
        }
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
                let id = self.voice_id();
                let rt = &mut self.playlists[module];
                if start || rt.voice.is_none() {
                    rt.current = Some(index);
                    rt.voice = Some(id);
                    rt.played.insert(index);
                    rt.duration = Some(buf.duration_secs());
                    let region = Region::full(&buf);
                    let slot = rt.slot;
                    self.engine.send(Command::Play { module, slot, id, buffer: buf, gain: 1.0, region, eq: EqCoeffs::flat() });
                    self.preload_next(module);
                } else {
                    rt.queued = Some((index, id));
                    let pl = &self.project.modules[module].playlist;
                    let crossfade = match pl.transition {
                        Transition::Gapless => 0,
                        Transition::Crossfade => (pl.crossfade_secs.max(0.0) as f64 * buf.sample_rate as f64) as usize,
                    };
                    let next = Queued { buffer: buf, gain: 1.0, id, crossfade };
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

    // -------------------------------------------------------------- jingles

    pub fn jingle(&self, m: usize, s: usize) -> Option<&JingleSlot> {
        self.project.modules[m].jingles[s].as_ref()
    }

    pub fn is_jingle_playing(&self, m: usize, s: usize) -> bool {
        self.engine.telemetry.position(m, s).is_some()
    }

    /// Clic sur un bouton : lance le son, ou l'arrête s'il joue déjà.
    pub fn trigger_jingle(&mut self, m: usize, s: usize) {
        if self.is_jingle_playing(m, s) {
            self.engine.send(Command::Stop { module: m, slot: s });
        } else if self.pending_play.remove(&(m, s)) {
            // Second clic pendant le chargement : on annule.
        } else {
            self.play_jingle(m, s);
        }
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
    }

    pub fn rescan_playlist(&mut self, m: usize) {
        self.project.modules[m].playlist.rescan();
        self.mark_dirty();
    }

    /// Modifie les options de lecture de la playlist (enchaînement, boucle, aléatoire, transition).
    pub fn update_playlist(&mut self, m: usize, f: impl FnOnce(&mut crate::project::PlaylistConfig)) {
        let pl = &mut self.project.modules[m].playlist;
        f(pl);
        self.mark_dirty();
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
        let rt = &mut self.playlists[m];
        rt.loading = true;
        let kind = LoadKind::Playlist { module: m, index, generation: rt.generation, start };
        let _ = self.jobs.send(LoadJob { path, rate: self.rate(), kind });
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

    pub fn set_prevent_sleep(&mut self, on: bool) {
        self.project.preferences.prevent_sleep = on;
        self.mark_dirty();
    }

    /// Bloque la mise en veille tant qu'un son joue (si l'option est active).
    fn update_keep_awake(&mut self) {
        let want = self.project.preferences.prevent_sleep && self.anything_playing();
        if want && self.keep_awake.is_none() {
            self.keep_awake = keepawake::Builder::default()
                .idle(true)
                .sleep(true)
                .reason("Lecture audio en cours")
                .app_name("JingleStar")
                .app_reverse_domain("com.acetayls.jinglestar")
                .create()
                .map_err(|e| self.status = Some(format!("{} : {e}", tr("Anti-veille indisponible", "Sleep prevention unavailable"))))
                .ok();
        } else if !want {
            self.keep_awake = None;
        }
    }

    // --------------------------------------------------------------- projet

    pub fn open_project(&mut self, path: PathBuf) {
        match Project::load(&path) {
            Ok(p) => {
                self.stop_all();
                let audio_changed = p.audio != self.project.audio;
                self.project = p;
                set_lang(self.project.preferences.language);
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
                    let _ = done_tx.send(LoadDone { path: job.path, rate: job.rate, kind: job.kind, result });
                    repaint();
                }
            })
            .expect("thread de décodage");
    }
    (job_tx, done_rx)
}
