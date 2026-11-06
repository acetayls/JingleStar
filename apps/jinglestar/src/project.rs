//! Modèle du projet (modules, jingles, playlists, réglages audio) et sauvegarde JSON.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use star_core::document::{self, Document};

use crate::audio::decode::is_audio_file;
use star_core::eq::EqSettings;
pub use star_core::sound::SoundSettings;
use crate::audio::loudness::NormalizeSettings;
use star_core::i18n::Lang;
use crate::remote::Mapping;
use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};

pub const DEFAULT_COLORS: [[u8; 3]; 8] = [
    [74, 144, 226],  // bleu
    [80, 170, 90],   // vert
    [230, 180, 50],  // jaune
    [220, 110, 50],  // orange
    [210, 70, 70],   // rouge
    [160, 100, 210], // violet
    [70, 180, 180],  // cyan
    [140, 140, 140], // gris
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModuleMode {
    #[default]
    Jingle,
    Playlist,
    /// Simple champ de texte, pour écrire sa conduite.
    Text,
    /// Conduite de spectacle : liste ordonnée de cues lancés un à un avec GO.
    Go,
}

/// Réglages de lecture d'un son, communs aux jingles et aux cues de la Cuelist :
/// volume, points de début, de fin et de boucle, fondus et égaliseur.
/// Ce que fait la Cuelist à la fin d'un cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CueFollow {
    /// On attend le prochain GO.
    #[default]
    Stop,
    /// Le cue suivant part tout seul à la fin de celui-ci.
    Continue,
}

/// Un son de la Cuelist (module GO).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub path: PathBuf,
    pub name: String,
    #[serde(flatten)]
    pub sound: SoundSettings,
    #[serde(default)]
    pub follow: CueFollow,
}

impl Cue {
    pub fn new(path: PathBuf) -> Self {
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { path, name, sound: SoundSettings::default(), follow: CueFollow::Stop }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JingleSlot {
    pub path: PathBuf,
    pub name: String,
    pub color: [u8; 3],
    #[serde(flatten)]
    pub sound: SoundSettings,
    /// Raccourci clavier qui lance le jingle (ex. « Ctrl+F5 »), voir `ui::hotkeys`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
}

impl JingleSlot {
    pub fn new(path: PathBuf, name: String, color: [u8; 3]) -> Self {
        Self { path, name, color, sound: SoundSettings::default(), hotkey: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Transition {
    /// Le morceau suivant démarre à la fin du précédent, sans blanc.
    #[default]
    Gapless,
    /// Fondu enchaîné de `crossfade_secs`.
    Crossfade,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaylistConfig {
    pub folder: Option<PathBuf>,
    pub tracks: Vec<PathBuf>,
    /// Reprend au début à la fin de la liste.
    pub looping: bool,
    /// Ordre de lecture aléatoire.
    pub shuffle: bool,
    pub transition: Transition,
    pub crossfade_secs: f32,
    /// Équilibrage du niveau des morceaux (gain de lecture, fichiers intacts).
    pub normalize: NormalizeSettings,
    /// Gains réglés à la main (dB) : ils remplacent le gain d'équilibrage du morceau.
    pub manual_gains: std::collections::HashMap<PathBuf, f32>,
}

impl Default for PlaylistConfig {
    fn default() -> Self {
        Self {
            folder: None,
            tracks: Vec::new(),
            looping: false,
            shuffle: false,
            transition: Transition::Gapless,
            crossfade_secs: 3.0,
            normalize: NormalizeSettings::default(),
            manual_gains: Default::default(),
        }
    }
}

impl PlaylistConfig {
    /// Relit le dossier : fichiers audio triés par nom.
    pub fn rescan(&mut self) {
        self.tracks = self.folder.as_deref().map(scan_folder).unwrap_or_default();
    }
}

pub fn scan_folder(folder: &Path) -> Vec<PathBuf> {
    let mut tracks: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| is_audio_file(p)).collect())
        .unwrap_or_default();
    tracks.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    tracks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleConfig {
    pub name: String,
    pub mode: ModuleMode,
    pub volume_db: f32,
    /// Paire de sorties stéréo : 0 = 1-2, 1 = 3-4, …
    pub output_pair: usize,
    pub jingles: Vec<Option<JingleSlot>>,
    pub playlist: PlaylistConfig,
    /// Égaliseur du bus du module (jingles et playlist).
    pub eq: EqSettings,
    /// Texte du module en mode Texte (la conduite).
    pub notes: String,
    /// Zoom du texte du module Texte (1 = 100 %), enregistré avec le projet.
    pub notes_zoom: f32,
    /// Cues du module GO, dans l'ordre de la conduite.
    pub cues: Vec<Cue>,
    /// Part de la largeur de la fenêtre occupée par le module (1 = part égale).
    pub width: f32,
}

impl ModuleConfig {
    pub fn numbered(i: usize) -> Self {
        Self { name: format!("Module {}", i + 1), ..Default::default() }
    }
}

impl Default for ModuleConfig {
    fn default() -> Self {
        Self {
            name: "Module".into(),
            mode: ModuleMode::Jingle,
            volume_db: 0.0,
            output_pair: 0,
            jingles: vec![None; JINGLE_SLOTS],
            playlist: PlaylistConfig::default(),
            eq: EqSettings::default(),
            notes: String::new(),
            notes_zoom: 1.0,
            cues: Vec::new(),
            width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    pub host: Option<String>,
    pub device: Option<String>,
    pub buffer_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Empêche la mise en veille de l'ordinateur pendant la lecture.
    pub prevent_sleep: bool,
    pub language: Lang,
    pub osc: OscSettings,
    pub midi: MidiSettings,
    pub console: ConsoleSettings,
    pub m32: M32Settings,
    /// Assignations des télécommandes (MIDI, console) vers les actions.
    pub mappings: Vec<Mapping>,
    /// Échap arrête aussi les playlists (sinon seulement les jingles).
    pub escape_stops_playlist: bool,
    /// Délai anti double-GO : un second déclenchement du même jingle dans ce délai est ignoré.
    pub double_go_ms: u32,
    /// Alerte quand les sons décodés occupent plus que cette mémoire (Mo).
    pub memory_warn_mb: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            prevent_sleep: true,
            language: Lang::Fr,
            osc: OscSettings::default(),
            midi: MidiSettings::default(),
            console: ConsoleSettings::default(),
            m32: M32Settings::default(),
            mappings: Vec::new(),
            escape_stops_playlist: false,
            double_go_ms: 300,
            memory_warn_mb: 4000,
        }
    }
}

/// Entrée MIDI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MidiSettings {
    pub enabled: bool,
    /// Nom du port ; `None` = le premier disponible.
    pub port: Option<String>,
}

/// Écoute d'une console Yamaha sur le réseau (protocole texte, TCP 49280).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ConsoleSettings {
    pub enabled: bool,
    /// Adresse IP de la console.
    pub host: String,
    pub port: u16,
}

impl Default for ConsoleSettings {
    fn default() -> Self {
        Self { enabled: false, host: "192.168.0.128".into(), port: 49280 }
    }
}

/// Écoute d'une console Midas M32 / Behringer X32 (OSC, UDP 10023).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct M32Settings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
}

impl Default for M32Settings {
    fn default() -> Self {
        Self { enabled: false, host: "192.168.0.64".into(), port: 10023 }
    }
}

/// Pilotage à distance : commandes OSC (UDP) et description OSCQuery (HTTP).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OscSettings {
    pub enabled: bool,
    /// Port UDP de réception des commandes.
    pub port: u16,
    /// Préfixe des adresses (« /jinglestar »), vide pour aucun.
    pub prefix: String,
    /// Serveur HTTP OSCQuery qui décrit l'arborescence des commandes.
    pub oscquery: bool,
    pub oscquery_port: u16,
    /// Annonce sur le réseau local (mDNS / Bonjour) pour la découverte automatique.
    pub zeroconf: bool,
    /// Nom annoncé sur le réseau.
    pub name: String,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8000,
            prefix: "/jinglestar".into(),
            oscquery: true,
            oscquery_port: 8001,
            zeroconf: true,
            name: "JingleStar".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub version: u32,
    /// Nom du projet affiché en haut de la fenêtre ; vide = nom du fichier.
    pub name: String,
    pub module_count: usize,
    pub modules: Vec<ModuleConfig>,
    pub audio: AudioSettings,
    pub preferences: Preferences,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: 1,
            name: String::new(),
            module_count: 1,
            modules: (0..MAX_MODULES).map(ModuleConfig::numbered).collect(),
            audio: AudioSettings::default(),
            preferences: Preferences::default(),
        }
    }
}

impl Project {
    /// Garantit 4 modules de 12 jingles, quel que soit le fichier lu.
    fn normalize(&mut self) {
        while self.modules.len() < MAX_MODULES {
            self.modules.push(ModuleConfig::numbered(self.modules.len()));
        }
        self.modules.truncate(MAX_MODULES);
        for m in &mut self.modules {
            m.jingles.resize(JINGLE_SLOTS, None);
        }
        self.module_count = self.module_count.clamp(1, MAX_MODULES);
    }

    /// Lit un projet, réparé au besoin (voir `star_core::document::load_checked`).
    pub fn load_checked(path: &Path) -> anyhow::Result<(Self, bool)> {
        document::load_checked(path)
    }

    /// Remet dans leurs bornes les valeurs aberrantes (NaN, infini, hors limites) : un projet
    /// modifié à la main ou abîmé ne doit ni rendre un son muet ni le faire hurler.
    pub fn sanitize(&mut self) {
        use star_core::num::clamp_f32 as num;
        fn sound(s: &mut SoundSettings) {
            s.sanitize(crate::osc::JINGLE_DB.1);
        }
        for m in &mut self.modules {
            num(&mut m.volume_db, 0.0, -60.0, crate::osc::MODULE_DB.1);
            num(&mut m.width, 1.0, 0.05, 20.0);
            m.eq.sanitize();
            for j in m.jingles.iter_mut().flatten() {
                sound(&mut j.sound);
            }
            for c in &mut m.cues {
                sound(&mut c.sound);
            }
            let pl = &mut m.playlist;
            num(&mut pl.crossfade_secs, 3.0, 0.0, 60.0);
            pl.manual_gains.retain(|_, g| g.is_finite());
            for g in pl.manual_gains.values_mut() {
                *g = g.clamp(-60.0, crate::audio::buffer::MAX_DB);
            }
            let n = &mut pl.normalize;
            let d = NormalizeSettings::default();
            num(&mut n.target_lufs, d.target_lufs, -60.0, 0.0);
            num(&mut n.target_peak, d.target_peak, -60.0, 0.0);
            num(&mut n.target_true_peak, d.target_true_peak, -60.0, 0.0);
            num(&mut n.target_rms, d.target_rms, -60.0, 0.0);
            num(&mut n.ceiling, d.ceiling, -60.0, 0.0);
        }
        self.preferences.double_go_ms = self.preferences.double_go_ms.min(5_000);
    }

    /// Enregistre sur le disque de façon sûre (fichier temporaire puis renommage).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        document::save(self, path, false)
    }

    /// Comme `save`, en gardant la version précédente en `.bak`.
    pub fn save_with_backup(&self, path: &Path) -> anyhow::Result<()> {
        document::save(self, path, true)
    }

    /// Sons introuvables (noms de fichiers, sans doublon).
    pub fn missing_files(&mut self) -> Vec<PathBuf> {
        document::missing_files(self)
    }

    /// Projet copié ailleurs : sons cherchés à la même place par rapport au fichier projet.
    pub fn resolve_moved(&mut self, project_file: &Path) -> usize {
        document::resolve_moved(self, project_file)
    }

    /// Sons introuvables cherchés par leur nom dans `folder` et ses sous-dossiers.
    pub fn relocate(&mut self, folder: &Path) -> usize {
        document::relocate(self, folder)
    }
}

impl Document for Project {
    fn normalize(&mut self) {
        Project::normalize(self);
    }

    fn sanitize(&mut self) {
        Project::sanitize(self);
    }

    /// Chemins de tous les sons du projet (jingles, cues, morceaux, dossiers de playlist).
    fn media_paths(&mut self) -> Vec<&mut PathBuf> {
        self.modules
            .iter_mut()
            .flat_map(|m| {
                m.jingles
                    .iter_mut()
                    .flatten()
                    .map(|j| &mut j.path)
                    .chain(m.cues.iter_mut().map(|c| &mut c.path))
                    .chain(m.playlist.tracks.iter_mut())
                    .chain(m.playlist.folder.iter_mut())
            })
            .collect()
    }
}

/// Session restaurée au lancement (voir `star_core::session`).
pub type SessionLoad = star_core::session::SessionLoad<Project>;

pub use star_core::session::snapshot_session;

pub fn load_session(path: &Path) -> SessionLoad {
    star_core::session::load_session(path)
}

/// JingleStar et ses dossiers sur cette machine.
pub const APP: star_core::app::App = star_core::app::App::new("JingleStar");

/// Cache des mesures de niveau des morceaux.
pub fn loudness_cache_path() -> Option<PathBuf> {
    APP.cache_file("loudness.json")
}

/// Fichier de session restauré automatiquement au lancement.
pub fn autosave_path() -> Option<PathBuf> {
    APP.session_path()
}

#[cfg(test)]
pub(crate) mod golden {
    //! Fichiers de référence : un projet et des préférences écrits par JingleStar 0.1.4.
    //! Ils doivent toujours se relire et se réécrire à l'identique (format stable).
    //! `STAR_UPDATE_FIXTURES=1 cargo test` les régénère : à ne faire que pour un changement
    //! de format voulu.
    use super::*;
    use crate::remote::{ActionKind, Mapping, Trigger};

    pub fn check(name: &str, value: serde_json::Value) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        if std::env::var_os("STAR_UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let expected: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value, expected, "format de {name} modifié");
    }

    /// Projet où chaque sorte de réglage a une valeur autre que celle par défaut.
    pub fn sample_project() -> Project {
        let mut p = Project { name: "Gala".into(), module_count: 4, ..Project::default() };
        p.audio = AudioSettings { host: Some("ASIO".into()), device: Some("Carte".into()), buffer_size: Some(256) };
        let m = &mut p.modules[0];
        m.volume_db = -3.0;
        m.output_pair = 1;
        m.width = 1.5;
        m.eq.low_cut.on = true;
        m.eq.peak1.gain_db = 2.5;
        let mut j = JingleSlot::new("/sons/applause.wav".into(), "Applaudissements".into(), DEFAULT_COLORS[2]);
        j.hotkey = Some("F1".into());
        j.sound = SoundSettings { volume_db: -6.0, looping: true, start: 0.5, end: Some(12.0), loop_start: Some(1.0), loop_end: Some(10.0), fade_in: 0.2, fade_out: 1.5, eq: EqSettings::default() };
        m.jingles[3] = Some(j);
        let m = &mut p.modules[1];
        m.mode = ModuleMode::Playlist;
        m.playlist.folder = Some("/musique".into());
        m.playlist.tracks = vec!["/musique/01.mp3".into(), "/musique/02.flac".into()];
        m.playlist.looping = true;
        m.playlist.shuffle = true;
        m.playlist.transition = Transition::Crossfade;
        m.playlist.crossfade_secs = 4.0;
        m.playlist.normalize.enabled = true;
        m.playlist.manual_gains.insert("/musique/01.mp3".into(), -2.0);
        let m = &mut p.modules[2];
        m.mode = ModuleMode::Text;
        m.notes = "**1.** Ouverture\n2. Entracte".into();
        m.notes_zoom = 1.4;
        let m = &mut p.modules[3];
        m.mode = ModuleMode::Go;
        m.cues = vec![Cue::new("/sons/top.wav".into()), Cue { follow: CueFollow::Continue, ..Cue::new("/sons/suite.wav".into()) }];
        let pr = &mut p.preferences;
        pr.prevent_sleep = false;
        pr.language = Lang::En;
        pr.osc = OscSettings { enabled: true, port: 9100, prefix: "/js".into(), oscquery: false, oscquery_port: 9101, zeroconf: false, name: "Régie".into() };
        pr.midi = MidiSettings { enabled: true, port: Some("nanoKONTROL".into()) };
        pr.console = ConsoleSettings { enabled: true, host: "10.0.0.2".into(), port: 49281 };
        pr.m32 = M32Settings { enabled: true, host: "10.0.0.3".into(), port: 10024 };
        pr.escape_stops_playlist = true;
        pr.double_go_ms = 450;
        pr.memory_warn_mb = 2000;
        pr.mappings = vec![
            Mapping { trigger: Trigger::Note { channel: 1, note: 60 }, action: ActionKind::JingleToggle, module: 0, jingle: 3 },
            Mapping { trigger: Trigger::Cc { channel: 2, cc: 7 }, action: ActionKind::ModuleVolume, module: 1, jingle: 0 },
            Mapping { trigger: Trigger::Program { channel: 1, program: 5 }, action: ActionKind::PlaylistTrack, module: 1, jingle: 0 },
            Mapping { trigger: Trigger::Console { address: "MIXER:Current/InCh/Fader/On".into(), x: 3, y: 0 }, action: ActionKind::CueGo, module: 3, jingle: 0 },
            Mapping { trigger: Trigger::Scene { number: 12 }, action: ActionKind::StopAll, module: 0, jingle: 0 },
            Mapping::default(),
        ];
        p
    }

    #[test]
    fn project_format_is_stable() {
        let p = sample_project();
        check("projet-0.1.4.jinglestar", serde_json::to_value(&p).unwrap());
        // Relu par le vrai chargement (normalisation, garde-fous), puis réécrit : identique.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projet-0.1.4.jinglestar");
        let back: Project = document::load(&path).unwrap();
        check("projet-0.1.4.jinglestar", serde_json::to_value(&back).unwrap());
    }

    #[test]
    fn config_paths_are_stable() {
        let d = directories::ProjectDirs::from("com", "acetayls", "JingleStar").unwrap();
        assert_eq!(autosave_path().unwrap(), d.config_dir().join("session.json"));
        assert_eq!(loudness_cache_path().unwrap(), d.cache_dir().join("loudness.json"));
        assert_eq!(APP.log_path().unwrap(), d.config_dir().join("jinglestar.log"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_gains_and_notes_survive_save() {
        let mut p = Project::default();
        p.modules[0].playlist.manual_gains.insert(PathBuf::from("/musique/01 Morceau.wav"), -4.5);
        p.modules[1].mode = ModuleMode::Text;
        p.modules[1].notes = "1. Ouverture\n2. Applaudissements".into();
        let back: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.modules[0].playlist.manual_gains.get(Path::new("/musique/01 Morceau.wav")), Some(&-4.5));
        assert_eq!(back.modules[1].mode, ModuleMode::Text);
        assert_eq!(back.modules[1].notes, p.modules[1].notes);
    }

    #[test]
    fn go_cues_survive_save() {
        let mut p = Project::default();
        p.modules[2].mode = ModuleMode::Go;
        p.modules[2].cues = vec![Cue::new(PathBuf::from("/show/01 Ouverture.wav")), Cue {
            sound: SoundSettings { volume_db: -6.0, fade_in: 1.5, ..Default::default() },
            follow: CueFollow::Continue,
            ..Cue::new(PathBuf::from("/show/02 Pluie.wav"))
        }];
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"mode\":\"go\""));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.modules[2].mode, ModuleMode::Go);
        assert_eq!(back.modules[2].cues, p.modules[2].cues);
        assert_eq!(back.modules[2].cues[0].name, "01 Ouverture");
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jinglestar-projet-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn nan_never_reaches_the_file_and_null_is_repaired() {
        let dir = temp("nan");
        let path = dir.join("session.json");
        let mut p = Project::default();
        p.modules[0].volume_db = f32::NAN;
        p.modules[1].jingles[0] = Some(JingleSlot::new("/a.wav".into(), "A".into(), [1, 2, 3]));
        p.modules[1].jingles[0].as_mut().unwrap().sound.fade_in = f64::INFINITY;
        p.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\"volume_db\": null"));
        let back: Project = document::load(&path).unwrap();
        assert_eq!(back.modules[0].volume_db, 0.0);
        assert_eq!(back.modules[1].jingles[0].as_ref().unwrap().sound.fade_in, 0.0);
        // Fichier écrit par la 0.1.1 avec un volume NaN : réparé au lieu d'être refusé.
        let broken = text.replacen("\"volume_db\": 0.0", "\"volume_db\": null", 1);
        std::fs::write(&path, broken).unwrap();
        let (back, repaired) = Project::load_checked(&path).unwrap();
        assert!(repaired);
        assert_eq!(back.modules[0].volume_db, 0.0);
        assert!(back.modules[0].jingles.iter().all(|j| j.is_none()), "emplacements vides gardés");
    }

    #[test]
    fn unreadable_session_is_kept_and_backup_used() {
        let dir = temp("session");
        let path = dir.join("session.json");
        let mut p = Project::default();
        p.modules[0].name = "Version 1".into();
        p.save_with_backup(&path).unwrap();
        p.modules[0].name = "Version 2".into();
        p.save_with_backup(&path).unwrap();
        assert!(star_core::document::with_suffix(&path, ".bak").exists());
        assert!(!star_core::document::with_suffix(&path, ".tmp").exists());
        // Session abîmée (fichier tronqué) : la copie précédente est chargée, rien n'est perdu.
        std::fs::write(&path, "{\"version\": 1, \"modules\": [").unwrap();
        let s = load_session(&path);
        assert_eq!(s.project.modules[0].name, "Version 1");
        assert!(s.alert.is_some());
        let kept = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("session-illisible-")).count();
        assert_eq!(kept, 1, "le fichier illisible est mis de côté");
        // Arrêt entre les deux renommages : seule la copie existe.
        let _ = std::fs::remove_file(&path);
        assert_eq!(load_session(&path).project.modules[0].name, "Version 1");
    }

    #[test]
    fn moved_show_folder_is_found_again() {
        let dir = temp("deplace");
        std::fs::create_dir_all(dir.join("sons")).unwrap();
        std::fs::write(dir.join("sons/Ouverture.wav"), b"x").unwrap();
        std::fs::create_dir_all(dir.join("ailleurs/rangement")).unwrap();
        std::fs::write(dir.join("ailleurs/rangement/Final.wav"), b"x").unwrap();
        let mut p = Project::default();
        p.modules[0].jingles[0] = Some(JingleSlot::new("/ancien/disque/Spectacle/sons/Ouverture.wav".into(), "O".into(), [0; 3]));
        p.modules[0].cues = vec![Cue::new("/ancien/disque/Spectacle/Final.wav".into())];
        assert_eq!(p.resolve_moved(&dir.join("projet.jinglestar")), 1);
        assert_eq!(p.modules[0].jingles[0].as_ref().unwrap().path, dir.join("sons/Ouverture.wav"));
        assert_eq!(p.missing_files().len(), 1);
        assert_eq!(p.relocate(&dir.join("ailleurs")), 1);
        assert_eq!(p.modules[0].cues[0].path, dir.join("ailleurs/rangement/Final.wav"));
        assert!(p.missing_files().is_empty());
    }

    #[test]
    fn older_projects_still_load() {
        // Avant le bloc commun de réglages : champs à plat, cue réduit à son volume.
        let jingle = r#"{"path":"/a.wav","name":"A","volume_db":-3.0,"color":[1,2,3],"looping":true,"fade_out":2.0}"#;
        let j: JingleSlot = serde_json::from_str(jingle).unwrap();
        assert_eq!(j.sound.volume_db, -3.0);
        assert!(j.sound.looping);
        assert_eq!(j.sound.fade_out, 2.0);
        let cue: Cue = serde_json::from_str(r#"{"path":"/b.wav","name":"B","volume_db":-6.0}"#).unwrap();
        assert_eq!(cue.sound.volume_db, -6.0);
        assert_eq!(cue.follow, CueFollow::Stop);
        // Et l'écriture reste à plat, lisible par une version précédente.
        let json = serde_json::to_string(&j).unwrap();
        assert!(json.contains("\"volume_db\":-3.0") && !json.contains("\"sound\""));
    }
}
