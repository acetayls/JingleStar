//! Modèle du projet (modules, jingles, playlists, réglages audio) et sauvegarde JSON.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::decode::is_audio_file;
use crate::audio::eq::EqSettings;
use crate::audio::loudness::NormalizeSettings;
use crate::i18n::Lang;
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

/// Un son de la conduite (module GO).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub volume_db: f32,
}

impl Cue {
    pub fn new(path: PathBuf) -> Self {
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { path, name, volume_db: 0.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Correspondances des télécommandes (MIDI, console) vers les actions.
    pub mappings: Vec<Mapping>,
    /// Échap arrête aussi les playlists (sinon seulement les jingles).
    pub escape_stops_playlist: bool,
    /// Délai anti double-GO : un second déclenchement du même jingle dans ce délai est ignoré.
    pub double_go_ms: u32,
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
    /// Écoute stricte : aucun octet n'est envoyé à la console.
    pub silent: bool,
}

impl Default for ConsoleSettings {
    fn default() -> Self {
        Self { enabled: false, host: "192.168.0.128".into(), port: 49280, silent: true }
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

/// Cache des mesures de niveau des morceaux.
pub fn loudness_cache_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "JingleStar").map(|d| d.cache_dir().join("loudness.json"))
}

/// Fichier de session restauré automatiquement au lancement.
pub fn autosave_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "JingleStar")
        .map(|d| d.config_dir().join("session.json"))
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
        p.modules[2].cues = vec![Cue::new(PathBuf::from("/show/01 Ouverture.wav")), Cue { volume_db: -6.0, ..Cue::new(PathBuf::from("/show/02 Pluie.wav")) }];
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"mode\":\"go\""));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.modules[2].mode, ModuleMode::Go);
        assert_eq!(back.modules[2].cues, p.modules[2].cues);
        assert_eq!(back.modules[2].cues[0].name, "01 Ouverture");
    }
}
