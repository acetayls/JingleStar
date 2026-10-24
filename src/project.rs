//! Modèle du projet (modules, jingles, playlists, réglages audio) et sauvegarde JSON.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::decode::is_audio_file;
use crate::audio::eq::EqSettings;
use crate::i18n::Lang;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JingleSlot {
    pub path: PathBuf,
    pub name: String,
    pub volume_db: f32,
    pub color: [u8; 3],
    /// Lecture en boucle entre `loop_start` et `loop_end`.
    #[serde(default)]
    pub looping: bool,
    /// Points d'édition en secondes (poignées de début et de fin, traits de boucle).
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub end: Option<f64>,
    #[serde(default)]
    pub loop_start: Option<f64>,
    #[serde(default)]
    pub loop_end: Option<f64>,
    /// Fondus d'entrée et de sortie, en secondes.
    #[serde(default)]
    pub fade_in: f64,
    #[serde(default)]
    pub fade_out: f64,
    #[serde(default)]
    pub eq: EqSettings,
}

impl JingleSlot {
    pub fn new(path: PathBuf, name: String, color: [u8; 3]) -> Self {
        Self {
            path,
            name,
            volume_db: 0.0,
            color,
            looping: false,
            start: 0.0,
            end: None,
            loop_start: None,
            loop_end: None,
            fade_in: 0.0,
            fade_out: 0.0,
            eq: EqSettings::default(),
        }
    }

    /// Points effectifs pour un son de `duration` secondes : (début, début de boucle, fin de boucle, fin).
    pub fn markers(&self, duration: f64) -> (f64, f64, f64, f64) {
        let end = self.end.unwrap_or(duration).clamp(0.0, duration);
        let start = self.start.clamp(0.0, end);
        let ls = self.loop_start.unwrap_or(start).clamp(start, end);
        let le = self.loop_end.unwrap_or(end).clamp(ls, end);
        (start, ls, le, end)
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
    /// Enchaîne automatiquement le morceau suivant.
    pub auto_advance: bool,
    /// Reprend au début à la fin de la liste.
    pub looping: bool,
    /// Ordre de lecture aléatoire.
    pub shuffle: bool,
    pub transition: Transition,
    pub crossfade_secs: f32,
}

impl Default for PlaylistConfig {
    fn default() -> Self {
        Self {
            folder: None,
            tracks: Vec::new(),
            auto_advance: true,
            looping: false,
            shuffle: false,
            transition: Transition::Gapless,
            crossfade_secs: 3.0,
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
}

impl Default for Preferences {
    fn default() -> Self {
        Self { prevent_sleep: true, language: Lang::Fr }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub version: u32,
    pub module_count: usize,
    pub modules: Vec<ModuleConfig>,
    pub audio: AudioSettings,
    pub preferences: Preferences,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: 1,
            module_count: 1,
            modules: (0..MAX_MODULES).map(ModuleConfig::numbered).collect(),
            audio: AudioSettings::default(),
            preferences: Preferences::default(),
        }
    }
}

impl Project {
    /// Garantit 8 modules de 12 jingles, quel que soit le fichier lu.
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

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut p: Project = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        p.normalize();
        Ok(p)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

/// Fichier de session restauré automatiquement au lancement.
pub fn autosave_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "JingleStar")
        .map(|d| d.config_dir().join("session.json"))
}
