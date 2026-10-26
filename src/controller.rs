//! Logique de l'application : projet, chargement des sons en arrière-plan,
//! déclenchement des jingles et enchaînement des playlists. Appelée par l'interface,
//! et par un thread de fond qui traite événements du mixeur et télécommandes même
//! quand l'interface ne dessine pas (fenêtre déplacée, réduite…).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::audio::buffer::{db_to_gain, AudioBuffer};
use crate::audio::decode::{analyze_file, decode, Decoded};
use crate::audio::loudness::{Loudness, LoudnessCache};
use crate::audio::engine::{AudioEngine, DeviceInfo};
use crate::audio::eq::{EqCoeffs, EqSettings};
use crate::audio::mixer::{is_playlist_slot, Command, Event, Queued, Region, MAX_MODULES, PLAYLIST_SLOTS};
use crate::i18n::{set_lang, tr, Lang};
use crate::console::ConsoleClient;
use crate::m32::M32Client;
use crate::midi::MidiListener;
use crate::remote::{Flag, InputEvent, Mapping, Pickup};
use crate::osc::{self, OscCommand, OscServer};
mod cues;
pub use cues::GoRuntime;

use crate::project::{
    autosave_path, load_session, loudness_cache_path, snapshot_session, AudioSettings, ConsoleSettings, JingleSlot, M32Settings, MidiSettings, ModuleConfig, ModuleMode, OscSettings, Project, SoundSettings, Transition, DEFAULT_COLORS,
};

/// Sons des jingles et des cues : combien sont prêts en mémoire, lesquels sont illisibles.
#[derive(Debug, Default, Clone)]
pub struct MemoryStatus {
    pub total: usize,
    pub ready: usize,
    pub failed: Vec<String>,
    /// Sons chargés avec un avertissement (fichier abîmé ou tronqué, son très long).
    pub warnings: Vec<String>,
    /// Mémoire des sons décodés (jingles, cues et morceaux de playlist préchargés), en octets.
    pub bytes: usize,
    /// La mémoire dépasse le seuil d'alerte des préférences.
    pub over_limit: bool,
}

/// Au-delà, un jingle ou un cue est signalé : tout le son reste en mémoire.
const LONG_SOUND_SECS: f64 = 20.0 * 60.0;

#[derive(Debug, Clone, Copy)]
enum LoadKind {
    Jingle,
    /// `start` : à lancer dès que prêt ; sinon à mettre en file pour l'enchaînement.
    Playlist { module: usize, index: usize, generation: u64, start: bool },
    /// Préchargement du morceau que lancera le prochain GO, pour un départ immédiat.
    Arm { module: usize },
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
    warning: Option<String>,
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
    /// Gain réglé à la main (dB), prioritaire sur l'équilibrage.
    Manual(f32),
}

impl TrackGain {
    /// Gain linéaire à appliquer à la lecture.
    pub fn linear(&self) -> f32 {
        match self {
            TrackGain::Ready { db, .. } | TrackGain::Manual(db) => 10f32.powf(db / 20.0),
            _ => 1.0,
        }
    }
}

/// État de lecture d'une playlist (non sauvegardé).
#[derive(Debug, Default, Clone)]
pub struct PlaylistRuntime {
    pub current: Option<usize>,
    pub playing: bool,
    /// En pause : le morceau garde sa place et reprend sans clic.
    pub paused: bool,
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
    /// Morceau déjà décodé que lancera le prochain GO (chemin, taux, son).
    armed: Option<(PathBuf, u32, Arc<AudioBuffer>)>,
    /// Préchargement en cours (chemin).
    arming: Option<PathBuf>,
    /// Échecs de lecture consécutifs : au bout d'un tour de liste, la playlist s'arrête.
    failures: usize,
}

pub struct Controller {
    pub project: Project,
    pub engine: AudioEngine,
    pub device: Option<DeviceInfo>,
    pub device_error: Option<String>,
    /// Sortie audio perdue (flux arrêté) : reconnexion automatique en cours.
    pub device_lost: bool,
    /// Ouverture du périphérique en cours, en tâche de fond ; `true` : reconnexion automatique.
    opening: Option<(Receiver<Result<DeviceInfo, String>>, bool)>,
    next_reconnect: Instant,
    /// Dernier compteur d'appels du callback audio vu, et quand il a bougé.
    heartbeat: (u64, Instant),
    pub status: Option<String>,
    /// Message important, affiché en bandeau jusqu'à ce qu'on le ferme (session réparée…).
    pub alert: Option<String>,
    /// Fichier de session (sauvegarde automatique) ; aucun pendant les tests.
    autosave: Option<PathBuf>,
    pub playlists: [PlaylistRuntime; MAX_MODULES],
    /// Conduites des modules GO.
    pub gos: [GoRuntime; MAX_MODULES],
    /// Sons de jingles décodés au taux du périphérique.
    cache: HashMap<PathBuf, Arc<AudioBuffer>>,
    loading: HashSet<PathBuf>,
    pub load_errors: HashMap<PathBuf, String>,
    pub load_warnings: HashMap<PathBuf, String>,
    /// Date de modification des sons chargés : un fichier remplacé sur le disque est relu.
    stamps: HashMap<PathBuf, Option<SystemTime>>,
    last_file_check: Instant,
    /// Jingles cliqués avant la fin de leur chargement.
    pending_play: HashSet<(usize, usize)>,
    jobs: Sender<LoadJob>,
    /// File prioritaire : sons attendus tout de suite (clic, GO, départ de playlist),
    /// servie avant les préchargements.
    urgent: Sender<LoadJob>,
    /// Sons déjà envoyés dans la file prioritaire.
    urgent_sent: HashSet<PathBuf>,
    done: Receiver<LoadDone>,
    next_voice: u64,
    dirty: bool,
    last_change: Instant,
    pub project_path: Option<PathBuf>,
    /// Modifications du projet pas encore enregistrées dans `project_path`.
    pub modified: bool,
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
    /// Écoute de la console Yamaha.
    pub console: Option<ConsoleClient>,
    /// Écoute de la console Midas M32 / Behringer X32.
    pub m32: Option<M32Client>,
    /// Reprise en douceur des volumes, par correspondance.
    pickup: HashMap<usize, Pickup>,
    /// Correspondance en attente d'apprentissage.
    pub learn: Option<usize>,
    /// Dernier message reçu d'une télécommande, pour les préférences.
    pub last_input: Option<String>,
    /// Dernier déclenchement de chaque jingle, pour le délai anti double-GO.
    last_trigger: HashMap<(usize, usize), Instant>,
}

impl Controller {
    pub fn new(repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let autosave = autosave_path();
        let (project, alert) = match autosave.as_deref() {
            Some(p) => {
                let s = load_session(p);
                if s.alert.is_none() {
                    snapshot_session(p);
                }
                (s.project, s.alert)
            }
            None => (Project::default(), None),
        };
        let mut project = project;
        // Préférences de la machine (carte son, télécommandes…) et dernier projet enregistré.
        let machine = autosave.as_deref().and_then(|p| MachinePrefs::load(&machine_path(p)));
        let mut last_project = None;
        if let Some(m) = machine {
            project.audio = m.audio;
            project.preferences = m.preferences;
            last_project = m.project_path;
        }
        let mut c = Self::with_project(project, autosave, repaint);
        c.project_path = last_project;
        c.alert = alert;
        c.open_device();
        c.restart_osc();
        c.restart_midi();
        c.restart_console();
        c.restart_m32();
        c.analyze_playlists();
        c
    }

    /// Contrôleur sans périphérique ni télécommande ouverts (tests, et base de `new`).
    fn with_project(project: Project, autosave: Option<PathBuf>, repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(repaint);
        let (urgent, jobs, done) = spawn_loaders(repaint.clone());
        let (analysis_jobs, analysis_done) = spawn_analyzers(repaint.clone());
        let mut c = Self {
            project,
            engine: AudioEngine::new(),
            device: None,
            device_error: None,
            device_lost: false,
            opening: None,
            next_reconnect: Instant::now(),
            heartbeat: (0, Instant::now()),
            status: None,
            alert: None,
            autosave,
            playlists: Default::default(),
            gos: Default::default(),
            cache: HashMap::new(),
            loading: HashSet::new(),
            load_errors: HashMap::new(),
            load_warnings: HashMap::new(),
            stamps: HashMap::new(),
            last_file_check: Instant::now(),
            pending_play: HashSet::new(),
            jobs,
            urgent,
            urgent_sent: HashSet::new(),
            done,
            next_voice: 1,
            dirty: false,
            last_change: Instant::now(),
            project_path: None,
            modified: false,
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
            console: None,
            m32: None,
            pickup: HashMap::new(),
            learn: None,
            last_input: None,
            last_trigger: HashMap::new(),
        };
        for rt in c.playlists.iter_mut() {
            rt.slot = PLAYLIST_SLOTS[0];
        }
        set_lang(c.project.preferences.language);
        c
    }

    fn rate(&self) -> u32 {
        self.device.as_ref().map(|d| d.sample_rate).unwrap_or(48_000)
    }

    pub fn output_pairs(&self) -> usize {
        self.device.as_ref().map(|d| (d.channels / 2).max(1)).unwrap_or(1)
    }

    /// Le projet a changé : sauvegarde de la session, et projet ouvert marqué modifié.
    fn mark_dirty(&mut self) {
        self.mark_prefs_dirty();
        self.modified = true;
    }

    /// Une préférence de la machine a changé : sauvegarde de la session seulement.
    fn mark_prefs_dirty(&mut self) {
        self.dirty = true;
        self.last_change = Instant::now();
    }

    fn voice_id(&mut self) -> u64 {
        self.next_voice += 1;
        self.next_voice
    }

    // ---------------------------------------------------------------- audio

    /// (Ré)ouvre le périphérique selon les réglages du projet. Tout s'arrête ; l'ouverture
    /// se fait en tâche de fond (un pilote lent ne fige pas l'interface).
    pub fn open_device(&mut self) {
        self.stop_all();
        self.request_open(false);
    }

    /// `auto` : reconnexion après une perte du flux, sur le périphérique choisi seulement
    /// (jamais de bascule silencieuse sur les haut-parleurs de l'ordinateur).
    fn request_open(&mut self, auto: bool) {
        let a = self.project.audio.clone();
        let rx = self.engine.request_open(a.host, a.device, a.buffer_size, auto);
        self.opening = Some((rx, auto));
    }

    /// Ouverture en cours.
    pub fn is_opening_device(&self) -> bool {
        self.opening.as_ref().is_some_and(|(_, auto)| !auto)
    }

    /// Bouton « Reconnecter » : nouvelle tentative tout de suite.
    pub fn reconnect_now(&mut self) {
        if self.opening.is_none() {
            self.request_open(true);
        }
    }

    fn poll_device(&mut self) {
        if let Some((rx, auto)) = &self.opening {
            let auto = *auto;
            match rx.try_recv() {
                Ok(res) => {
                    self.opening = None;
                    self.on_device_opened(res, auto);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return,
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.opening = None,
            }
        }
        while let Ok(err) = self.engine.stream_errors.try_recv() {
            crate::log::write(&format!("Erreur du flux audio : {err}"));
            self.device_error = Some(err);
        }
        // Flux mort : le callback audio n'est plus appelé (carte débranchée, pilote arrêté).
        let count = self.engine.telemetry.callbacks.load(std::sync::atomic::Ordering::Relaxed);
        if count != self.heartbeat.0 {
            self.heartbeat = (count, Instant::now());
        } else if self.device.is_some() && !self.device_lost && self.heartbeat.1.elapsed() > Duration::from_millis(1500) {
            self.device_lost = true;
            self.next_reconnect = Instant::now();
            let name = self.device.as_ref().map(|d| d.name.clone()).unwrap_or_default();
            crate::log::write(&format!("Sortie audio perdue : {name}"));
            self.device_error.get_or_insert_with(|| format!("{} : {name}", tr("Sortie audio perdue", "Audio output lost")));
        }
        if self.opening.is_none() && Instant::now() >= self.next_reconnect {
            // Sortie perdue : nouvel essai toutes les 2 s.
            if self.device_lost {
                self.next_reconnect = Instant::now() + Duration::from_secs(2);
                self.request_open(true);
            // Interface choisie absente au lancement : on la reprend dès qu'elle revient,
            // si rien ne joue sur la sortie de secours.
            } else if self.device.as_ref().is_some_and(|d| d.wanted_missing.is_some()) && !self.anything_playing() {
                self.next_reconnect = Instant::now() + Duration::from_secs(5);
                self.request_open(true);
            }
        }
    }

    fn on_device_opened(&mut self, res: Result<DeviceInfo, String>, auto: bool) {
        let old_rate = self.device.as_ref().map(|d| d.sample_rate);
        match res {
            Ok(info) => {
                if auto {
                    crate::log::write(&format!("Sortie audio rétablie : {}", info.name));
                    if self.device_lost {
                        self.status = Some(format!("{} : {}", tr("Sortie audio rétablie", "Audio output restored"), info.name));
                    }
                }
                self.device = Some(info);
                self.device_error = None;
                self.device_lost = false;
                self.heartbeat = (self.engine.telemetry.callbacks.load(std::sync::atomic::Ordering::Relaxed), Instant::now());
            }
            Err(e) => {
                if auto {
                    // Le périphérique n'est pas revenu : on garde l'état et on réessaie plus tard.
                    if self.device_lost {
                        self.device_error = Some(format!("{} ({e})", tr("Sortie audio perdue, reconnexion…", "Audio output lost, reconnecting…")));
                    }
                    return;
                }
                crate::log::write(&format!("Ouverture de la sortie audio impossible : {e}"));
                self.device = None;
                self.device_lost = false;
                self.device_error = Some(e);
            }
        }
        // Nouveau mixeur : plus aucun son ne joue, l'état de lecture repart de zéro.
        self.reset_playback();
        if old_rate != Some(self.rate()) {
            self.cache.clear();
            self.loading.clear();
        }
        self.apply_mixer_params();
        self.preload_jingles();
    }

    /// Le mixeur a été recréé : playlists et cues ne jouent plus.
    fn reset_playback(&mut self) {
        self.pending_play.clear();
        for m in 0..MAX_MODULES {
            self.reset_go_voices(m);
            let rt = &mut self.playlists[m];
            rt.generation += 1;
            rt.playing = false;
            rt.paused = false;
            rt.loading = false;
            rt.queued = None;
            rt.voice = None;
            rt.duration = None;
        }
    }

    pub fn set_audio_settings(&mut self, settings: AudioSettings) {
        self.project.audio = settings;
        self.mark_prefs_dirty();
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
            .flat_map(|m| m.jingles.iter().flatten().map(|j| j.path.clone()).chain(m.cues.iter().map(|c| c.path.clone())))
            .collect();
        for p in paths {
            self.request_jingle_load(&p);
        }
    }

    /// Son attendu tout de suite : il passe devant les préchargements (même s'il y est déjà).
    pub(super) fn request_urgent_load(&mut self, path: &Path) {
        if self.cache.contains_key(path) || self.urgent_sent.contains(path) {
            return;
        }
        self.urgent_sent.insert(path.to_path_buf());
        self.loading.insert(path.to_path_buf());
        self.load_errors.remove(path);
        let _ = self.urgent.send(LoadJob { path: path.to_path_buf(), rate: self.rate(), kind: LoadKind::Jingle, analyze: false });
    }

    fn request_jingle_load(&mut self, path: &Path) {
        if self.cache.contains_key(path) || self.loading.contains(path) {
            return;
        }
        self.loading.insert(path.to_path_buf());
        self.load_errors.remove(path);
        self.load_warnings.remove(path);
        let _ = self.jobs.send(LoadJob { path: path.to_path_buf(), rate: self.rate(), kind: LoadKind::Jingle, analyze: false });
    }

    /// État du préchargement des jingles et des cues, et mémoire occupée par les sons décodés.
    pub fn memory_status(&self) -> MemoryStatus {
        let mut paths: Vec<&Path> = self
            .project
            .modules
            .iter()
            .take(self.project.module_count)
            .flat_map(|m| m.jingles.iter().flatten().map(|j| j.path.as_path()).chain(m.cues.iter().map(|c| c.path.as_path())))
            .collect();
        paths.sort();
        paths.dedup();
        let mut st = MemoryStatus { total: paths.len(), ..Default::default() };
        let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for p in paths {
            if self.cache.contains_key(p) {
                st.ready += 1;
                if let Some(w) = self.load_warnings.get(p) {
                    st.warnings.push(format!("{} : {w}", name(p)));
                }
            } else if self.load_errors.contains_key(p) {
                st.failed.push(name(p));
            }
        }
        let size = |b: &AudioBuffer| (b.samples.len() + b.overview.len() + b.detail.len()) * std::mem::size_of::<f32>();
        st.bytes = self.cache.values().map(|b| size(b)).sum::<usize>()
            + self.playlists.iter().filter_map(|rt| rt.armed.as_ref()).map(|(_, _, b)| size(b)).sum::<usize>();
        let limit = self.project.preferences.memory_warn_mb as usize * 1_000_000;
        st.over_limit = limit > 0 && st.bytes > limit;
        st
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
        self.poll_device();
        let dropped = self.engine.take_dropped();
        if dropped > 0 {
            let msg = format!(
                "{dropped} {}",
                tr("commande(s) audio perdue(s) : la sortie audio ne répond pas", "audio command(s) lost: the audio output is not responding")
            );
            crate::log::write(&msg);
            self.status = Some(msg);
        }
        for m in 0..self.project.module_count {
            self.ensure_armed(m);
        }
        self.poll_osc();
        self.poll_remote();
        self.update_keep_awake();
        if self.last_file_check.elapsed() > Duration::from_secs(5) {
            self.last_file_check = Instant::now();
            self.check_files();
        }
        if self.dirty && self.last_change.elapsed() > Duration::from_millis(800) {
            self.dirty = false;
            if let Some(p) = self.autosave.clone() {
                if let Err(e) = self.save_session(&p) {
                    let msg = format!("{} : {e}", tr("Sauvegarde automatique impossible", "Autosave failed"));
                    crate::log::write(&msg);
                    self.status = Some(msg);
                }
            }
        }
    }

    /// Sons introuvables revenus (disque rebranché) ou fichiers illisibles remplacés : nouvel
    /// essai. Sons modifiés sur le disque : relus. Un son en train de jouer garde sa version.
    fn check_files(&mut self) {
        let back: Vec<PathBuf> = self
            .load_errors
            .keys()
            .filter(|p| self.stamps.get(*p).is_none_or(|old| *old != modified(p)))
            .cloned()
            .collect();
        for p in back {
            self.load_errors.remove(&p);
            self.stamps.remove(&p);
            self.request_jingle_load(&p);
        }
        let changed: Vec<PathBuf> = self
            .cache
            .keys()
            .filter(|p| self.stamps.get(*p).is_some_and(|old| *old != modified(p)))
            .cloned()
            .collect();
        for p in changed {
            crate::log::write(&format!("Fichier modifié, relu : {}", p.display()));
            self.cache.remove(&p);
            self.stamps.remove(&p);
            self.request_jingle_load(&p);
        }
    }

    /// Cherche les sons introuvables dans `folder` et ses sous-dossiers, par nom de fichier.
    pub fn relocate_files(&mut self, folder: &Path) {
        let fixed = self.project.relocate(folder);
        let left = self.project.missing_files().len();
        self.status = Some(format!(
            "{fixed} {} ; {left} {}",
            tr("fichier(s) retrouvé(s)", "file(s) found"),
            tr("encore introuvable(s)", "still missing")
        ));
        if fixed > 0 {
            self.load_errors.clear();
            self.mark_dirty();
            self.preload_jingles();
            self.analyze_playlists();
        }
    }


    fn on_loaded(&mut self, done: LoadDone) {
        if let Some(l) = done.loudness {
            self.on_analyzed(done.path.clone(), Ok(l));
        }
        if let Some(w) = &done.warning {
            crate::log::write(&format!("{} : {w}", done.path.display()));
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
            } else if let LoadKind::Arm { module } = done.kind {
                // Le prochain passage de `poll` relance le préchargement au bon taux.
                self.playlists[module].arming = None;
            }
            return;
        }
        match done.kind {
            LoadKind::Jingle => {
                self.loading.remove(&done.path);
                self.urgent_sent.remove(&done.path);
                match done.result {
                    Ok(buf) => {
                        let long = buf.duration_secs() > LONG_SOUND_SECS;
                        let warning = done.warning.clone().or_else(|| {
                            long.then(|| {
                                format!(
                                    "{} ({:.0} min) : {}",
                                    tr("son très long", "very long sound"),
                                    buf.duration_secs() / 60.0,
                                    tr("tout le son reste en mémoire, préférez un module Playlist", "the whole sound stays in memory, prefer a Playlist module")
                                )
                            })
                        });
                        match warning {
                            Some(w) => self.load_warnings.insert(done.path.clone(), w),
                            None => self.load_warnings.remove(&done.path),
                        };
                        self.stamps.insert(done.path.clone(), modified(&done.path));
                        self.cache.insert(done.path.clone(), buf);
                        self.replan_all_follows();
                        self.start_pending_cues(&done.path, true);
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
                        // Gardé pour ne réessayer que si le fichier apparaît ou change.
                        self.stamps.insert(done.path.clone(), modified(&done.path));
                        self.start_pending_cues(&done.path, false);
                        self.pending_play.retain(|&(m, s)| {
                            self.project.modules[m].jingles[s].as_ref().map(|j| j.path != done.path).unwrap_or(true)
                        });
                        self.load_errors.insert(done.path, e);
                    }
                }
            }
            LoadKind::Arm { module } => {
                let rt = &mut self.playlists[module];
                if rt.arming.as_ref() != Some(&done.path) {
                    return;
                }
                rt.arming = None;
                if let Ok(buf) = done.result {
                    rt.armed = Some((done.path, done.rate, buf));
                }
            }
            LoadKind::Playlist { module, index, generation, start } => {
                let rt = &mut self.playlists[module];
                if generation != rt.generation {
                    return;
                }
                rt.loading = false;
                let buf = match done.result {
                    Ok(b) => {
                        rt.failures = 0;
                        b
                    }
                    Err(e) => {
                        let msg = format!(
                            "{} : {} ({e})",
                            tr("Lecture impossible", "Cannot play"),
                            done.path.file_name().unwrap_or_default().to_string_lossy()
                        );
                        crate::log::write(&msg);
                        self.status = Some(msg);
                        let rt = &mut self.playlists[module];
                        rt.failures += 1;
                        // Tout un tour de liste sans morceau lisible (clé USB retirée…) : on
                        // s'arrête au lieu de réessayer sans fin.
                        if rt.failures >= self.project.modules[module].playlist.tracks.len().max(1) {
                            rt.failures = 0;
                            if rt.voice.is_none() {
                                rt.playing = false;
                            }
                            self.status = Some(format!(
                                "{} {} : {}",
                                tr("Playlist du module", "Playlist of module"),
                                module + 1,
                                tr("aucun morceau lisible, lecture arrêtée", "no readable track, playback stopped")
                            ));
                            return;
                        }
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
                if start || self.playlists[module].voice.is_none() {
                    self.start_track_now(module, index, buf);
                } else {
                    let gain = self.track_gain(module, index).linear();
                    let id = self.voice_id();
                    let rt = &mut self.playlists[module];
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
            Event::Ended { module, slot, id, .. } if is_playlist_slot(slot) => {
                let rt = &mut self.playlists[module];
                if rt.voice != Some(id) {
                    return;
                }
                rt.voice = None;
                rt.paused = false;
                // Si le suivant est encore en chargement, il démarrera à son arrivée.
                if !rt.loading {
                    // Fin naturelle de la liste : le prochain GO la reprend au début
                    // (après un Stop, il relance le morceau arrêté).
                    if rt.playing {
                        rt.current = Some(0);
                    }
                    rt.playing = false;
                    rt.duration = None;
                }
            }
            Event::Ended { module, slot, id, follow } => self.on_cue_ended(module, slot, id, follow),
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
        self.gos[n] = GoRuntime::default();
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
            self.gos[i] = GoRuntime::default();
        }
        self.apply_mixer_params();
        self.mark_dirty();
    }

    pub fn set_module_volume(&mut self, m: usize, db: f32) {
        let Some(db) = clean_db(db, crate::osc::MODULE_DB) else { return };
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

    pub fn set_module_notes(&mut self, m: usize, notes: String) {
        self.project.modules[m].notes = notes;
        self.mark_dirty();
    }

    pub fn rename_module(&mut self, m: usize, name: String) {
        self.project.modules[m].name = name;
        self.mark_dirty();
    }

    pub fn stop_module(&mut self, m: usize) {
        self.engine.send(Command::StopModule { module: m });
        self.pending_play.retain(|&(pm, _)| pm != m);
        self.reset_go_voices(m);
        let rt = &mut self.playlists[m];
        rt.generation += 1;
        rt.playing = false;
        rt.paused = false;
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
        self.engine.send(Command::StopJingles);
        self.pending_play.clear();
        // Les cues arrêtés ainsi n'enchaînent pas sur le suivant.
        for m in 0..MAX_MODULES {
            self.mark_cues_stopping(m);
        }
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
        self.mark_prefs_dirty();
    }

    pub fn set_double_go_ms(&mut self, ms: u32) {
        self.project.preferences.double_go_ms = ms;
        self.mark_prefs_dirty();
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
            let region = sound_region(&j.sound, &buf);
            let eq = j.sound.eq.coeffs(self.rate());
            self.engine.send(Command::Play { module: m, slot: s, id, buffer: buf, gain: db_to_gain(j.sound.volume_db), region, eq });
        } else {
            self.pending_play.insert((m, s));
            self.request_urgent_load(&j.path);
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
            let old = j.sound.clone();
            f(j);
            // Valeur aberrante (NaN, infini) : le réglage précédent est gardé.
            if clean_db(j.sound.volume_db, crate::osc::JINGLE_DB).is_none() {
                j.sound.volume_db = old.volume_db;
            }
            let old_eq = old.eq;
            let gain = db_to_gain(j.sound.volume_db);
            self.engine.send(Command::SetSlotGain { module: m, slot: s, gain });
            if j.sound.eq != old_eq {
                self.engine.send(Command::SetSlotEq { module: m, slot: s, eq: j.sound.eq.coeffs(rate) });
            }
            if let Some(buf) = self.cache.get(&j.path) {
                let region = sound_region(&j.sound, buf);
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
        // Le dernier morceau a peut-être maintenant un suivant.
        self.requeue(m);
    }

    /// La liste a changé pendant la lecture : le morceau en file (et un chargement en cours)
    /// est recalculé d'après la nouvelle liste, au lieu de garder un index périmé.
    fn requeue(&mut self, m: usize) {
        let rt = &mut self.playlists[m];
        if !rt.playing {
            return;
        }
        rt.generation += 1;
        rt.queued = None;
        rt.loading = false;
        self.engine.send(Command::QueueNext { module: m, next: None });
        if self.playlists[m].voice.is_some() {
            self.preload_next(m);
        } else if let Some(c) = self.playlists[m].current {
            // Le morceau de départ était encore en chargement.
            self.load_track(m, c, true);
        }
    }

    /// Recale le morceau en cours d'après son chemin après un changement de la liste.
    fn relocate_current(&mut self, m: usize, current: Option<PathBuf>) {
        let tracks = &self.project.modules[m].playlist.tracks;
        let rt = &mut self.playlists[m];
        rt.current = current.and_then(|p| tracks.iter().position(|t| *t == p));
        rt.played.clear();
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
            self.requeue(m);
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
        let current = self.playlists[m].current.and_then(|c| self.project.modules[m].playlist.tracks.get(c).cloned());
        self.project.modules[m].playlist.rescan();
        self.relocate_current(m, current);
        self.mark_dirty();
        self.analyze_playlist(m);
        self.requeue(m);
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
        let Some(path) = pl.tracks.get(index) else { return TrackGain::Off };
        if let Some(db) = pl.manual_gains.get(path) {
            return TrackGain::Manual(*db);
        }
        if !pl.normalize.enabled {
            return TrackGain::Off;
        }
        match self.analysis.get(path) {
            None | Some(Analysis::Pending) => TrackGain::Pending,
            Some(Analysis::Failed(e)) => TrackGain::Failed(e.clone()),
            Some(Analysis::Done(l)) => match pl.normalize.gain_db(l) {
                Some((db, limited)) => TrackGain::Ready { db, limited, loudness: *l },
                None => TrackGain::Unmeasurable(*l),
            },
        }
    }

    /// Règle à la main le gain d'un morceau (`None` : retour au gain automatique).
    /// Le morceau en cours le prend tout de suite, lissé par le mixeur.
    pub fn set_track_gain(&mut self, m: usize, index: usize, db: Option<f32>) {
        let Some(path) = self.project.modules[m].playlist.tracks.get(index).cloned() else { return };
        self.update_playlist(m, |pl| match db {
            Some(db) => {
                pl.manual_gains.insert(path, db);
            }
            None => {
                pl.manual_gains.remove(&path);
            }
        });
        let rt = &self.playlists[m];
        if let (Some(cur), Some(_)) = (rt.current, rt.voice) {
            if cur == index {
                let gain = self.track_gain(m, cur).linear();
                self.engine.send(Command::SetSlotGain { module: m, slot: rt.slot, gain });
            }
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
        if pl.tracks.is_empty() {
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
        // Un départ attendu passe devant les préchargements ; le suivant en file, non.
        let queue = if start { &self.urgent } else { &self.jobs };
        let _ = queue.send(LoadJob { path, rate: self.rate(), kind, analyze });
    }

    /// Lance tout de suite un morceau déjà décodé, puis précharge le suivant.
    fn start_track_now(&mut self, module: usize, index: usize, buf: Arc<AudioBuffer>) {
        let gain = self.track_gain(module, index).linear();
        let id = self.voice_id();
        let rt = &mut self.playlists[module];
        rt.loading = false;
        rt.current = Some(index);
        rt.voice = Some(id);
        rt.played.insert(index);
        rt.duration = Some(buf.duration_secs());
        let region = Region::full(&buf);
        let slot = rt.slot;
        self.engine.send(Command::Play { module, slot, id, buffer: buf, gain, region, eq: EqCoeffs::flat() });
        self.preload_next(module);
    }

    /// À l'arrêt, garde décodé en mémoire le morceau que lancera le prochain GO :
    /// le départ ne dépend plus du décodage (une demi-seconde ou plus pour un long MP3).
    fn ensure_armed(&mut self, m: usize) {
        let rate = self.rate();
        let pl = &self.project.modules[m].playlist;
        let rt = &self.playlists[m];
        if self.project.modules[m].mode != ModuleMode::Playlist || rt.playing || rt.loading {
            return;
        }
        let Some(path) = pl.tracks.get(rt.current.unwrap_or(0)).cloned() else {
            self.playlists[m].armed = None;
            return;
        };
        if rt.armed.as_ref().is_some_and(|(p, r, _)| *p == path && *r == rate) || rt.arming.as_ref() == Some(&path) {
            return;
        }
        let analyze = pl.normalize.enabled && !matches!(self.analysis.get(&path), Some(Analysis::Done(_) | Analysis::Failed(_)));
        let rt = &mut self.playlists[m];
        // On libère tout de suite l'ancien son préchargé.
        rt.armed = None;
        rt.arming = Some(path.clone());
        let _ = self.urgent.send(LoadJob { path, rate, kind: LoadKind::Arm { module: m }, analyze });
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
        rt.paused = false;
        rt.queued = None;
        rt.current = Some(index);
        rt.duration = None;
        self.engine.send(Command::QueueNext { module: m, next: None });
        // Morceau préchargé : départ immédiat, sans attendre le décodage.
        let rate = self.rate();
        let path = self.project.modules[m].playlist.tracks.get(index);
        let armed = self.playlists[m].armed.take_if(|(p, r, _)| Some(&*p) == path && *r == rate);
        match armed {
            Some((_, _, buf)) => self.start_track_now(m, index, buf),
            None => {
                self.playlists[m].armed = None;
                self.load_track(m, index, true);
            }
        }
    }

    /// Lecture : reprend après une pause, sinon lance le morceau sélectionné, ou le premier.
    pub fn playlist_go(&mut self, m: usize) {
        if self.playlists[m].paused {
            self.playlist_set_paused(m, false);
            return;
        }
        let idx = self.playlists[m].current.unwrap_or(0);
        self.playlist_play(m, idx);
    }

    /// Pause ou reprise de la playlist en cours (les deux voix d'un fondu enchaîné comprises).
    pub fn playlist_set_paused(&mut self, m: usize, paused: bool) {
        let rt = &mut self.playlists[m];
        if paused == rt.paused || (paused && (!rt.playing || rt.voice.is_none())) {
            return;
        }
        rt.paused = paused;
        for slot in PLAYLIST_SLOTS {
            self.engine.send(Command::SetPaused { module: m, slot, paused });
        }
    }

    pub fn playlist_toggle_pause(&mut self, m: usize) {
        let paused = self.playlists[m].paused;
        self.playlist_set_paused(m, !paused);
    }

    pub fn playlist_stop(&mut self, m: usize) {
        for slot in PLAYLIST_SLOTS {
            self.engine.send(Command::Stop { module: m, slot });
        }
        self.engine.send(Command::QueueNext { module: m, next: None });
        let rt = &mut self.playlists[m];
        rt.generation += 1;
        rt.playing = false;
        rt.paused = false;
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

    /// Déplace la lecture du morceau en cours à `fraction` (0..1) de sa durée, avec un micro-fondu.
    pub fn playlist_seek(&mut self, m: usize, fraction: f64) {
        if let Some((_, len)) = self.playlist_position(m) {
            let frame = (fraction.clamp(0.0, 1.0) * len as f64) as usize;
            self.engine.send(Command::Seek { module: m, slot: self.playlists[m].slot, frame });
        }
    }

    /// Déplace la lecture d'un jingle en cours à l'image `frame`, avec un micro-fondu.
    pub fn jingle_seek(&mut self, m: usize, s: usize, frame: usize) {
        if self.is_jingle_playing(m, s) {
            self.engine.send(Command::Seek { module: m, slot: s, frame });
        }
    }

    pub fn anything_playing(&self) -> bool {
        (0..MAX_MODULES).any(|m| (0..crate::audio::mixer::SLOTS).any(|s| self.engine.telemetry.position(m, s).is_some()))
    }

    pub fn set_language(&mut self, lang: Lang) {
        self.project.preferences.language = lang;
        set_lang(lang);
        self.mark_prefs_dirty();
    }

    /// Parts de largeur des modules (réglées en glissant la séparation entre deux modules).
    pub fn set_module_widths(&mut self, widths: &[f32]) {
        for (m, w) in widths.iter().enumerate().take(MAX_MODULES) {
            self.project.modules[m].width = w.clamp(0.05, 20.0);
        }
        self.mark_dirty();
    }

    pub fn set_memory_warn_mb(&mut self, mb: u32) {
        self.project.preferences.memory_warn_mb = mb;
        self.mark_prefs_dirty();
    }

    pub fn set_prevent_sleep(&mut self, on: bool) {
        self.project.preferences.prevent_sleep = on;
        self.mark_prefs_dirty();
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
                .display(true)
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
            self.mark_prefs_dirty();
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
            OscCommand::JingleVolume(m, s, db) if m < n => self.update_jingle(m, s, |j| j.sound.volume_db = db),
            OscCommand::PlaylistPlay(m, track) if m < n && mode(self, m) == ModuleMode::Playlist => match track {
                Some(i) => self.playlist_play(m, i),
                None => self.playlist_go(m),
            },
            OscCommand::PlaylistStop(m) if m < n && mode(self, m) == ModuleMode::Playlist => self.playlist_stop(m),
            OscCommand::PlaylistPause(m, state) if m < n && mode(self, m) == ModuleMode::Playlist => match state {
                Some(p) => self.playlist_set_paused(m, p),
                None => self.playlist_toggle_pause(m),
            },
            OscCommand::PlaylistStep(m, d) if m < n && mode(self, m) == ModuleMode::Playlist => self.playlist_step(m, d),
            OscCommand::CueGo(Some(m)) if m < n && mode(self, m) == ModuleMode::Go => self.cue_go(m),
            OscCommand::CueGo(None) if self.first_go_module().is_some() => {
                let m = self.first_go_module().unwrap_or(0);
                self.cue_go(m);
            }
            OscCommand::CueStep(m, d) if m < n && mode(self, m) == ModuleMode::Go => self.cue_step(m, d),
            OscCommand::CueStandby(m, i) if m < n && mode(self, m) == ModuleMode::Go => self.set_standby(m, i),
            OscCommand::PlaylistShuffle(m, on) if m < n => self.update_playlist(m, |p| p.shuffle = on),
            OscCommand::PlaylistLoop(m, on) if m < n => self.update_playlist(m, |p| p.looping = on),
            other => self.status = Some(format!("{source} : {} ({other:?})", tr("commande ignorée", "command ignored"))),
        }
    }

    // ------------------------------------------------------ MIDI et console

    pub fn set_midi_settings(&mut self, settings: MidiSettings) {
        if self.project.preferences.midi != settings {
            self.project.preferences.midi = settings;
            self.mark_prefs_dirty();
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

    pub fn set_console_settings(&mut self, settings: ConsoleSettings) {
        if self.project.preferences.console != settings {
            self.project.preferences.console = settings;
            self.mark_prefs_dirty();
            self.restart_console();
        }
    }

    /// (Re)lance l'écoute de la console selon les préférences.
    pub fn restart_console(&mut self) {
        self.console = None;
        let settings = self.project.preferences.console.clone();
        if settings.enabled {
            self.console = Some(ConsoleClient::start(settings, self.repaint.clone()));
        }
    }

    pub fn set_m32_settings(&mut self, settings: M32Settings) {
        if self.project.preferences.m32 != settings {
            self.project.preferences.m32 = settings;
            self.mark_prefs_dirty();
            self.restart_m32();
        }
    }

    /// (Re)lance l'écoute de la M32 selon les préférences.
    pub fn restart_m32(&mut self) {
        self.m32 = None;
        let settings = self.project.preferences.m32.clone();
        if settings.enabled {
            self.m32 = Some(M32Client::start(settings, self.repaint.clone()));
        }
    }

    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.project.preferences.mappings.push(mapping);
        self.pickup.clear();
        self.mark_prefs_dirty();
    }

    pub fn update_mapping(&mut self, i: usize, mapping: Mapping) {
        if let Some(m) = self.project.preferences.mappings.get_mut(i) {
            if *m != mapping {
                *m = mapping;
                self.pickup.clear();
                self.mark_prefs_dirty();
            }
        }
    }

    pub fn remove_mapping(&mut self, i: usize) {
        if i < self.project.preferences.mappings.len() {
            self.project.preferences.mappings.remove(i);
            self.pickup.clear();
            self.learn = match self.learn {
                Some(l) if l == i => None,
                Some(l) if l > i => Some(l - 1),
                other => other,
            };
            self.mark_prefs_dirty();
        }
    }

    fn poll_remote(&mut self) {
        let mut events = Vec::new();
        if let Some(l) = self.midi.as_ref() {
            events.extend(l.events.try_iter().map(|e| ("MIDI", e)));
        }
        if let Some(c) = self.console.as_ref() {
            events.extend(c.events.try_iter().map(|e| ("QL", e)));
        }
        if let Some(c) = self.m32.as_ref() {
            events.extend(c.events.try_iter().map(|e| ("M32", e)));
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
        // Les déclencheurs des consoles portent déjà leur source (« QL · … », « M32 · … »).
        let labelled = matches!(ev, InputEvent::Console { .. } | InputEvent::Scene { .. });
        self.last_input = Some(if labelled { ev.label() } else { format!("{source} · {}", ev.label()) });
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
        let commands: Vec<(usize, OscCommand)> = self
            .project
            .preferences
            .mappings
            .iter()
            .enumerate()
            .filter(|(_, m)| m.matches(&ev))
            .filter_map(|(i, m)| {
                m.command(&ev, |module, flag| {
                    let pl = &self.project.modules[module].playlist;
                    match flag {
                        Flag::Shuffle => pl.shuffle,
                        Flag::Loop => pl.looping,
                    }
                })
                .map(|c| (i, c))
            })
            .collect();
        for (i, cmd) in commands {
            let current = match cmd {
                OscCommand::ModuleVolume(m, _) => Some(self.project.modules[m].volume_db),
                OscCommand::JingleVolume(m, s, _) => self.project.modules[m].jingles[s].as_ref().map(|j| j.sound.volume_db),
                _ => None,
            };
            let target = match cmd {
                OscCommand::ModuleVolume(_, db) | OscCommand::JingleVolume(_, _, db) => db,
                _ => 0.0,
            };
            if let Some(current) = current {
                if !self.pickup.entry(i).or_default().take(current, target) {
                    continue;
                }
            }
            self.handle_command(source, cmd);
        }
    }

    /// Volume que le fader de la correspondance `i` doit rejoindre avant d'agir.
    pub fn pickup_waiting(&self, i: usize) -> Option<f32> {
        self.pickup.get(&i).and_then(|p| p.waiting)
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
                                volume_db: j.sound.volume_db,
                                playing: self.is_jingle_playing(m, s),
                            })
                        })
                        .collect(),
                    playlist: osc::PlaylistState {
                        tracks: cfg.playlist.tracks.len(),
                        current: rt.current,
                        playing: rt.playing,
                        paused: rt.paused,
                        shuffle: cfg.playlist.shuffle,
                        looping: cfg.playlist.looping,
                    },
                    cues: osc::CueListState {
                        names: cfg.cues.iter().map(|c| c.name.clone()).collect(),
                        standby: self.gos[m].standby,
                        playing: (0..cfg.cues.len()).filter(|&i| self.cue_position(m, i).is_some()).count(),
                    },
                }
            })
            .collect();
        osc::Snapshot { modules }
    }

    // --------------------------------------------------------------- projet

    pub fn open_project(&mut self, path: PathBuf) {
        match Project::load_checked(&path) {
            Ok((mut p, repaired)) => {
                // Projet copié depuis un autre ordinateur : sons cherchés près du fichier projet.
                let moved = p.resolve_moved(&path);
                // Carte son et télécommandes restent celles de cette machine.
                p.audio = self.project.audio.clone();
                p.preferences = self.project.preferences.clone();
                self.replace_project(p, Some(path));
                self.modified = false;
                let mut msg = tr("Projet ouvert", "Project opened").to_string();
                if moved > 0 {
                    msg += &format!(" ({moved} {})", tr("son(s) retrouvé(s) près du projet", "sound(s) found next to the project"));
                }
                if repaired {
                    self.alert = Some(tr(
                        "Le projet contenait des valeurs invalides : elles ont repris leur réglage par défaut.",
                        "The project contained invalid values: they were reset to their defaults.",
                    ).into());
                }
                self.status = Some(msg);
            }
            Err(e) => self.status = Some(format!("{} : {e}", tr("Ouverture impossible", "Cannot open"))),
        }
    }

    /// Nouveau projet : un module Jingles et un module Playlist, vides. Les préférences
    /// (audio, télécommandes, langue…) sont celles de la machine et restent les mêmes.
    pub fn new_project(&mut self) {
        let mut p = Project { audio: self.project.audio.clone(), preferences: self.project.preferences.clone(), ..Project::default() };
        p.module_count = 2;
        p.modules[0].mode = ModuleMode::Jingle;
        p.modules[1].mode = ModuleMode::Playlist;
        self.replace_project(p, None);
        self.modified = false;
        self.status = Some(tr("Nouveau projet", "New project").into());
    }

    /// Remplace le projet en cours : tout s'arrête, puis le moteur et les télécommandes suivent.
    fn replace_project(&mut self, p: Project, path: Option<PathBuf>) {
        self.stop_all();
        let audio_changed = p.audio != self.project.audio;
        let osc_changed = p.preferences.osc != self.project.preferences.osc;
        let midi_changed = p.preferences.midi != self.project.preferences.midi;
        let console_changed = p.preferences.console != self.project.preferences.console;
        let m32_changed = p.preferences.m32 != self.project.preferences.m32;
        self.project = p;
        set_lang(self.project.preferences.language);
        if osc_changed {
            self.restart_osc();
        }
        if midi_changed {
            self.restart_midi();
        }
        if console_changed {
            self.restart_console();
        }
        if m32_changed {
            self.restart_m32();
        }
        // Les sons de l'ancien projet libèrent la mémoire ; ceux qui servent encore restent prêts.
        let used: std::collections::HashSet<PathBuf> = self
            .project
            .modules
            .iter()
            .flat_map(|m| m.jingles.iter().flatten().map(|j| j.path.clone()).chain(m.cues.iter().map(|c| c.path.clone())))
            .collect();
        self.cache.retain(|p, _| used.contains(p));
        self.learn = None;
        self.project_path = path;
        self.playlists = Default::default();
        self.gos = Default::default();
        self.pending_play.clear();
        self.mark_dirty();
        if audio_changed {
            self.open_device();
        } else {
            self.apply_mixer_params();
            self.preload_jingles();
        }
        self.analyze_playlists();
    }

    /// « Enregistrer » : dans le projet ouvert. `false` s'il n'y en a pas (il faut « Enregistrer sous… »).
    pub fn save_project(&mut self) -> bool {
        match self.project_path.clone() {
            Some(p) => {
                self.save_project_as(p);
                true
            }
            None => false,
        }
    }

    pub fn save_project_as(&mut self, path: PathBuf) {
        match self.project.save(&path) {
            Ok(()) => {
                self.status = Some(format!("{} : {}", tr("Projet enregistré", "Project saved"), path.display()));
                self.project_path = Some(path);
                self.modified = false;
                // Le chemin du projet est retenu pour « Enregistrer » au prochain lancement.
                self.mark_prefs_dirty();
            }
            Err(e) => self.status = Some(format!("{} : {e}", tr("Enregistrement impossible", "Cannot save"))),
        }
    }

    /// Sauvegarde immédiate (à la fermeture).
    /// Session et préférences de la machine.
    fn save_session(&self, p: &Path) -> anyhow::Result<()> {
        self.project.save_with_backup(p)?;
        MachinePrefs {
            audio: self.project.audio.clone(),
            preferences: self.project.preferences.clone(),
            project_path: self.project_path.clone(),
        }
        .save(&machine_path(p))
    }

    pub fn flush(&mut self) {
        if let Some(p) = self.autosave.clone() {
            if let Err(e) = self.save_session(&p) {
                crate::log::write(&format!("Sauvegarde à la fermeture impossible : {e}"));
            }
        }
        self.dirty = false;
        if self.cache_dirty {
            self.save_loudness_cache();
        }
    }
}

/// Convertit les points d'un son (secondes) en zone de lecture (images).
pub fn sound_region(j: &SoundSettings, buf: &AudioBuffer) -> Region {
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

/// Préférences propres à cet ordinateur, gardées à part du projet : ouvrir un projet venu
/// d'une autre machine ne change ni la carte son ni les télécommandes.
#[derive(serde::Serialize, serde::Deserialize)]
struct MachinePrefs {
    audio: AudioSettings,
    preferences: crate::project::Preferences,
    #[serde(default)]
    project_path: Option<PathBuf>,
}

impl MachinePrefs {
    fn load(path: &Path) -> Option<Self> {
        let mut m: Self = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        // Mêmes garde-fous que le projet (valeurs aberrantes).
        let mut p = Project { preferences: m.preferences, ..Project::default() };
        p.sanitize();
        m.preferences = p.preferences;
        Some(m)
    }

    fn save(&self, path: &Path) -> anyhow::Result<()> {
        use std::io::Write;
        let tmp = crate::project::with_suffix(path, ".tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

fn machine_path(session: &Path) -> PathBuf {
    session.with_file_name("machine.json")
}

/// Exécute `f` en rattrapant une panique, rendue comme une erreur (déjà inscrite au journal).
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(tr("le décodeur a planté sur ce fichier", "the decoder crashed on this file").to_string()))
}

/// Volume reçu de l'extérieur (OSC, télécommande, saisie) : `None` s'il n'est pas un nombre,
/// sinon ramené dans `range`.
pub fn clean_db(db: f32, range: (f32, f32)) -> Option<f32> {
    db.is_finite().then(|| db.clamp(range.0, range.1))
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Pool de threads de décodage, avec deux files : prioritaire et préchargement.
/// La mesure de niveau éventuelle (morceau lancé avant la fin de son analyse) reste faite
/// avant de rendre le son : il part ainsi tout de suite au bon niveau, sans saut de volume.
fn spawn_loaders(repaint: Arc<dyn Fn() + Send + Sync>) -> (Sender<LoadJob>, Sender<LoadJob>, Receiver<LoadDone>) {
    let (urgent_tx, urgent_rx) = unbounded::<LoadJob>();
    let (job_tx, job_rx) = unbounded::<LoadJob>();
    let (done_tx, done_rx) = unbounded::<LoadDone>();
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
    for i in 0..workers {
        let urgent_rx = urgent_rx.clone();
        let job_rx = job_rx.clone();
        let done_tx = done_tx.clone();
        let repaint = repaint.clone();
        std::thread::Builder::new()
            .name(format!("decoder-{i}"))
            .spawn(move || {
                loop {
                    let job = match urgent_rx.try_recv() {
                        Ok(j) => j,
                        Err(_) => crossbeam_channel::select! {
                            recv(urgent_rx) -> j => match j { Ok(j) => j, Err(_) => break },
                            recv(job_rx) -> j => match j { Ok(j) => j, Err(_) => break },
                        },
                    };
                    // Un fichier qui fait planter le décodeur donne une erreur normale :
                    // le thread continue de servir les chargements suivants.
                    let decoded = guarded(|| decode(&job.path, job.rate, job.analyze).map_err(|e| e.to_string()));
                    let (result, loudness, warning) = match decoded {
                        Ok(Decoded { buffer, loudness, warning }) => (Ok(Arc::new(buffer)), loudness, warning),
                        Err(e) => (Err(e), None, None),
                    };
                    let _ = done_tx.send(LoadDone { path: job.path, rate: job.rate, kind: job.kind, result, loudness, warning });
                    repaint();
                }
            })
            .expect("thread de décodage");
    }
    (urgent_tx, job_tx, done_rx)
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
                    let result = guarded(|| analyze_file(&path).map_err(|e| e.to_string()));
                    let _ = done_tx.send(AnalysisDone { path, result });
                    repaint();
                }
            })
            .expect("thread de mesure");
    }
    (job_tx, done_rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::buffer::AudioBuffer;
    use crate::audio::mixer::JINGLE_SLOTS;
    use crate::project::{Cue, CueFollow};

    fn headless(project: Project) -> Controller {
        Controller::with_project(project, None, || {})
    }

    fn wait_until(c: &mut Controller, mut done: impl FnMut(&Controller) -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            c.poll();
            if done(c) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn looping_playlist_of_unreadable_files_stops() {
        let mut p = Project::default();
        p.modules[0].mode = ModuleMode::Playlist;
        p.modules[0].playlist.looping = true;
        p.modules[0].playlist.tracks = vec!["/introuvable/a.wav".into(), "/introuvable/b.wav".into(), "/introuvable/c.wav".into()];
        let mut c = headless(p);
        c.playlist_play(0, 0);
        assert!(c.playlists[0].playing);
        assert!(wait_until(&mut c, |c| !c.playlists[0].playing), "la playlist s'arrête après un tour d'échecs");
        assert!(c.status.as_deref().unwrap_or("").contains(tr("aucun morceau lisible", "no readable track")));
        // Plus aucun chargement ne repart tout seul.
        std::thread::sleep(Duration::from_millis(100));
        c.poll();
        assert!(!c.playlists[0].loading && !c.playlists[0].playing);
    }

    #[test]
    fn removing_a_track_keeps_the_queue_in_step() {
        let mut p = Project::default();
        p.modules[0].mode = ModuleMode::Playlist;
        p.modules[0].playlist.tracks = (0..4).map(|i| PathBuf::from(format!("/m/{i}.wav"))).collect();
        let mut c = headless(p);
        // Morceau 2 en cours, morceau 3 en file.
        let rt = &mut c.playlists[0];
        (rt.playing, rt.current, rt.voice, rt.queued) = (true, Some(2), Some(7), Some((3, 8)));
        let generation = rt.generation;
        c.remove_track(0, 0);
        let rt = &c.playlists[0];
        assert_eq!(rt.current, Some(1), "le morceau en cours garde sa place");
        assert_eq!(rt.queued, None, "l'ancien index en file est abandonné");
        assert!(rt.generation > generation && rt.loading, "le suivant est recalculé et rechargé");
        // Relecture du dossier : le morceau en cours est retrouvé par son chemin.
        c.playlists[0].current = Some(1);
        c.project.modules[0].playlist.tracks.insert(0, "/m/a.wav".into());
        let cur = c.project.modules[0].playlist.tracks.get(2).cloned();
        c.relocate_current(0, cur);
        assert_eq!(c.playlists[0].current, Some(2));
    }

    #[test]
    fn non_numeric_volumes_are_ignored() {
        let mut c = headless(Project::default());
        c.set_module_volume(0, -6.0);
        c.handle_command("OSC", OscCommand::ModuleVolume(0, f32::NAN));
        assert_eq!(c.project.modules[0].volume_db, -6.0);
        c.set_module_volume(0, f32::INFINITY);
        assert_eq!(c.project.modules[0].volume_db, -6.0);
        c.project.modules[0].jingles[0] = Some(JingleSlot::new("/a.wav".into(), "A".into(), [0; 3]));
        c.update_jingle(0, 0, |j| j.sound.volume_db = f32::NAN);
        assert_eq!(c.project.modules[0].jingles[0].as_ref().unwrap().sound.volume_db, 0.0);
    }

    #[test]
    fn decoder_panic_becomes_an_error() {
        let r: Result<(), String> = guarded(|| panic!("fichier piégé"));
        assert!(r.is_err());
    }

    #[test]
    fn mixer_follow_on_updates_the_cue_list() {
        let mut p = Project::default();
        p.modules[0].mode = ModuleMode::Go;
        p.modules[0].cues = (0..3).map(|i| Cue { follow: CueFollow::Continue, ..Cue::new(format!("/show/{i}.wav").into()) }).collect();
        let mut c = headless(p);
        for i in 0..3 {
            let buf = Arc::new(AudioBuffer::from_planar(vec![0.1; 480], vec![0.1; 480], 48_000));
            c.cache.insert(format!("/show/{i}.wav").into(), buf);
        }
        c.cue_go(0);
        assert_eq!(c.gos[0].standby, 1);
        let (s0, id0) = (0..JINGLE_SLOTS).find_map(|s| c.gos[0].slots[s].map(|(_, id)| (s, id))).unwrap();
        let next = c.gos[0].planned.iter().find(|p| p.after_id == id0).copied().expect("enchaînement confié au mixeur");
        assert_eq!(next.index, 1);
        assert_ne!(next.slot, s0);
        // Le mixeur annonce la fin du cue 1 et le départ du cue 2 sur la même image.
        c.on_event(Event::Ended { module: 0, slot: s0, id: id0, follow: Some((next.slot, next.id)) });
        assert_eq!(c.gos[0].slots[next.slot], Some((1, next.id)));
        assert_eq!(c.gos[0].slots[s0], None);
        assert_eq!(c.gos[0].standby, 2, "le cue en attente avance");
        // Le cue 2 prévoit à son tour le cue 3, sur un autre emplacement.
        let third = c.gos[0].planned.iter().find(|p| p.after_id == next.id).copied().unwrap();
        assert_eq!(third.index, 2);
        assert_ne!(third.slot, next.slot);
        // Le dernier cue n'enchaîne sur rien.
        c.on_event(Event::Ended { module: 0, slot: next.slot, id: next.id, follow: Some((third.slot, third.id)) });
        assert!(c.gos[0].planned.iter().all(|p| p.after_id != third.id));
    }
}
