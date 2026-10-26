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

/// Réglages de lecture d'un son, communs aux jingles et aux cues de la Cuelist :
/// volume, points de début, de fin et de boucle, fondus et égaliseur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SoundSettings {
    pub volume_db: f32,
    /// Lecture en boucle entre `loop_start` et `loop_end`.
    pub looping: bool,
    /// Points d'édition en secondes (poignées de début et de fin, traits de boucle).
    pub start: f64,
    pub end: Option<f64>,
    pub loop_start: Option<f64>,
    pub loop_end: Option<f64>,
    /// Fondus d'entrée et de sortie, en secondes.
    pub fade_in: f64,
    pub fade_out: f64,
    pub eq: EqSettings,
}

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
}

impl JingleSlot {
    pub fn new(path: PathBuf, name: String, color: [u8; 3]) -> Self {
        Self { path, name, color, sound: SoundSettings::default() }
    }
}

impl SoundSettings {
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

    /// Lit un projet. Un fichier où des valeurs manquent (`null` écrit par une version
    /// précédente pour un nombre invalide) est réparé : ces valeurs reprennent leur réglage
    /// par défaut, et `repaired` le signale.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Self::load_checked(path).map(|(p, _)| p)
    }

    pub fn load_checked(path: &Path) -> anyhow::Result<(Self, bool)> {
        let text = std::fs::read_to_string(path)?;
        let (mut p, repaired) = match serde_json::from_str::<Project>(&text) {
            Ok(p) => (p, false),
            Err(e) => {
                let mut v: serde_json::Value = serde_json::from_str(&text)?;
                if !strip_nulls(&mut v) {
                    return Err(e.into());
                }
                (serde_json::from_value(v).map_err(|_| e)?, true)
            }
        };
        p.normalize();
        p.sanitize();
        Ok((p, repaired))
    }

    /// Remet dans leurs bornes les valeurs aberrantes (NaN, infini, hors limites) : un projet
    /// modifié à la main ou abîmé ne doit ni rendre un son muet ni le faire hurler.
    pub fn sanitize(&mut self) {
        fn num(v: &mut f32, default: f32, min: f32, max: f32) {
            *v = if v.is_finite() { v.clamp(min, max) } else { default };
        }
        fn secs(v: &mut f64) {
            *v = if v.is_finite() { v.clamp(0.0, 86_400.0) } else { 0.0 };
        }
        fn opt_secs(v: &mut Option<f64>) {
            if v.is_some_and(|x| !x.is_finite() || x < 0.0) {
                *v = None;
            }
        }
        fn band(b: &mut crate::audio::eq::Band) {
            use crate::audio::eq::{GAIN_RANGE, MAX_FREQ, MIN_FREQ};
            num(&mut b.freq, 1000.0, MIN_FREQ, MAX_FREQ);
            num(&mut b.gain_db, 0.0, -GAIN_RANGE, GAIN_RANGE);
            num(&mut b.q, 1.0, 0.1, 18.0);
        }
        fn eq(e: &mut EqSettings) {
            for b in [&mut e.low_cut, &mut e.low_shelf, &mut e.peak1, &mut e.peak2, &mut e.high_shelf] {
                band(b);
            }
        }
        fn sound(s: &mut SoundSettings) {
            num(&mut s.volume_db, 0.0, -60.0, crate::osc::JINGLE_DB.1);
            secs(&mut s.start);
            opt_secs(&mut s.end);
            opt_secs(&mut s.loop_start);
            opt_secs(&mut s.loop_end);
            secs(&mut s.fade_in);
            secs(&mut s.fade_out);
            eq(&mut s.eq);
        }
        for m in &mut self.modules {
            num(&mut m.volume_db, 0.0, -60.0, crate::osc::MODULE_DB.1);
            num(&mut m.width, 1.0, 0.05, 20.0);
            eq(&mut m.eq);
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

    /// Enregistre sur le disque de façon sûre : fichier temporaire propre à ce nom, écrit et
    /// forcé sur le disque (une coupure de courant ne laisse pas un fichier vide), puis renommé.
    /// Avec `backup`, la version précédente est gardée en `.bak`.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        self.write_to(path, false)
    }

    pub fn save_with_backup(&self, path: &Path) -> anyhow::Result<()> {
        self.write_to(path, true)
    }

    fn write_to(&self, path: &Path, backup: bool) -> anyhow::Result<()> {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Jamais de `null` dans le fichier : il le rendrait illisible.
        let mut clean = self.clone();
        clean.sanitize();
        let json = serde_json::to_string_pretty(&clean)?;
        let tmp = with_suffix(path, ".tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        if backup && path.exists() {
            std::fs::rename(path, with_suffix(path, ".bak"))?;
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Chemins de tous les sons du projet (jingles, cues, morceaux, dossiers de playlist).
    fn paths_mut(&mut self) -> impl Iterator<Item = &mut PathBuf> {
        self.modules.iter_mut().flat_map(|m| {
            m.jingles
                .iter_mut()
                .flatten()
                .map(|j| &mut j.path)
                .chain(m.cues.iter_mut().map(|c| &mut c.path))
                .chain(m.playlist.tracks.iter_mut())
                .chain(m.playlist.folder.iter_mut())
                .collect::<Vec<_>>()
        })
    }

    /// Sons introuvables (noms de fichiers, sans doublon).
    pub fn missing_files(&mut self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.paths_mut().filter(|p| !p.exists()).map(|p| p.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Projet copié ailleurs (autre ordinateur, autre disque) : un son introuvable à son
    /// ancien chemin est cherché à la même place par rapport au fichier projet
    /// (« …/Spectacle/sons/a.wav » → « <dossier du projet>/sons/a.wav »). Renvoie le nombre corrigé.
    pub fn resolve_moved(&mut self, project_file: &Path) -> usize {
        let Some(base) = project_file.parent() else { return 0 };
        let mut fixed = 0;
        for p in self.paths_mut() {
            if p.exists() {
                continue;
            }
            let parts: Vec<_> = p.components().filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_os_string()),
                _ => None,
            }).collect();
            // Le plus long morceau de chemin d'abord : le plus précis.
            let found = (1..=parts.len().min(4)).rev().map(|k| {
                let mut c = base.to_path_buf();
                c.extend(&parts[parts.len() - k..]);
                c
            }).find(|c| c.exists());
            if let Some(c) = found {
                *p = c;
                fixed += 1;
            }
        }
        fixed
    }

    /// Relocalisation : les sons introuvables sont cherchés par leur nom de fichier dans
    /// `folder` et ses sous-dossiers. Renvoie le nombre de chemins corrigés.
    pub fn relocate(&mut self, folder: &Path) -> usize {
        let mut index: std::collections::HashMap<std::ffi::OsString, PathBuf> = Default::default();
        index_files(folder, &mut index, 0);
        let mut fixed = 0;
        for p in self.paths_mut() {
            if p.exists() {
                continue;
            }
            let key = p.file_name().map(|n| n.to_ascii_lowercase());
            if let Some(found) = key.and_then(|k| index.get(&k)) {
                *p = found.clone();
                fixed += 1;
            } else if p.extension().is_none() {
                // Dossier de playlist : même nom de dossier.
                let key = p.file_name().map(|n| n.to_ascii_lowercase());
                if let Some(d) = key.and_then(|k| find_dir(folder, &k, 0)) {
                    *p = d;
                    fixed += 1;
                }
            }
        }
        fixed
    }
}

/// Fichiers d'un dossier et de ses sous-dossiers (8 niveaux au plus), par nom en minuscules.
fn index_files(dir: &Path, index: &mut std::collections::HashMap<std::ffi::OsString, PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 8 {
                index_files(&p, index, depth + 1);
            }
        } else if let Some(n) = p.file_name() {
            index.entry(n.to_ascii_lowercase()).or_insert(p);
        }
    }
}

fn find_dir(dir: &Path, name: &std::ffi::OsStr, depth: usize) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().map(|n| n.to_ascii_lowercase()).as_deref() == Some(name) {
                return Some(p);
            }
            if depth < 8 {
                if let Some(f) = find_dir(&p, name, depth + 1) {
                    return Some(f);
                }
            }
        }
    }
    None
}

/// « session.json » → « session.json.tmp » : le suffixe s'ajoute au nom complet, deux projets
/// de même nom (a.json, a.jinglestar) n'ont donc pas le même fichier temporaire.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Retire les valeurs `null` des objets JSON (le réglage par défaut les remplace).
/// Les éléments `null` des tableaux (emplacements de jingle vides) restent. Renvoie vrai si
/// quelque chose a été retiré.
fn strip_nulls(v: &mut serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(map) => {
            let before = map.len();
            map.retain(|_, x| !x.is_null());
            let mut changed = map.len() != before;
            for x in map.values_mut() {
                changed |= strip_nulls(x);
            }
            changed
        }
        serde_json::Value::Array(a) => a.iter_mut().fold(false, |c, x| strip_nulls(x) | c),
        _ => false,
    }
}

/// Résultat du chargement de la session au lancement.
pub struct SessionLoad {
    pub project: Project,
    /// Message à afficher bien en vue : session réparée, illisible, copie de secours utilisée.
    pub alert: Option<String>,
}

/// Charge la session sans jamais perdre le fichier : une session illisible est mise de côté
/// (« session-illisible-<date>.json ») au lieu d'être écrasée, et la copie `.bak` prend le relais.
pub fn load_session(path: &Path) -> SessionLoad {
    use crate::i18n::tr;
    let bak = with_suffix(path, ".bak");
    if !path.exists() {
        // Arrêt brutal entre les deux renommages de la sauvegarde : la copie est complète.
        if let Ok(p) = Project::load(&bak) {
            return SessionLoad { project: p, alert: None };
        }
        return SessionLoad { project: Project::default(), alert: None };
    }
    let aside = || {
        let name = format!("session-illisible-{}.json", crate::log::now(true));
        let dest = path.with_file_name(name);
        std::fs::copy(path, &dest).ok().map(|_| dest)
    };
    match Project::load_checked(path) {
        Ok((p, false)) => SessionLoad { project: p, alert: None },
        Ok((p, true)) => {
            let kept = aside();
            let msg = format!(
                "{}{}",
                tr(
                    "La session contenait des valeurs invalides : elles ont repris leur réglage par défaut.",
                    "The session contained invalid values: they were reset to their defaults."
                ),
                kept.map(|k| format!(" {} {}", tr("Original gardé :", "Original kept:"), k.display())).unwrap_or_default()
            );
            crate::log::write(&msg);
            SessionLoad { project: p, alert: Some(msg) }
        }
        Err(e) => {
            let kept = aside();
            let _ = std::fs::remove_file(path);
            let kept = kept.map(|k| format!(" {} {}", tr("Fichier gardé :", "File kept:"), k.display())).unwrap_or_default();
            let (project, msg) = match Project::load(&bak) {
                Ok(p) => (p, format!("{} ({e}). {}{kept}", tr("Session illisible", "Unreadable session"), tr("La copie de secours précédente a été chargée.", "The previous backup copy was loaded."))),
                Err(_) => (Project::default(), format!("{} ({e}). {}{kept}", tr("Session illisible", "Unreadable session"), tr("Aucune copie de secours lisible : projet vierge.", "No readable backup: blank project."))),
            };
            crate::log::write(&msg);
            SessionLoad { project, alert: Some(msg) }
        }
    }
}

/// Au lancement, garde une copie datée de la session dans « sauvegardes » (les 5 dernières).
pub fn snapshot_session(path: &Path) {
    let Some(dir) = path.parent().map(|d| d.join("sauvegardes")) else { return };
    if !path.exists() || std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::copy(path, dir.join(format!("session-{}.json", crate::log::now(true))));
    let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect())
        .unwrap_or_default();
    old.sort();
    let n = old.len().saturating_sub(5);
    for p in &old[..n] {
        let _ = std::fs::remove_file(p);
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
        let back = Project::load(&path).unwrap();
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
        assert!(with_suffix(&path, ".bak").exists());
        assert!(!with_suffix(&path, ".tmp").exists());
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
