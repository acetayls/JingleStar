//! Projet CueStar (fichier `.cuestar`, JSON), même mécanisme que JingleStar :
//! enregistrement sûr (fichier temporaire forcé sur le disque puis renommé), session
//! restaurée au lancement avec copie de secours, copies datées, projet déplacé retrouvé.

use crate::protocol::{Clip, Fit};
use serde::{Deserialize, Serialize};
use star_core::document::{self, Document};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "cuestar";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    /// Nom donné par l'utilisateur (sinon le nom du fichier).
    pub name: String,
    pub lecteur: LecteurData,
    pub diaporama: DiaporamaData,
    pub cuelist: CuelistData,
    /// Module Texte : la conduite écrite (balisage léger de JingleStar).
    pub notes: String,
}

/// Suite à donner à la fin d'un cue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CueFollow {
    /// On attend le prochain GO.
    #[default]
    Stop,
    /// Le cue suivant part tout seul à la fin de celui-ci.
    Continue,
}

/// Un cue de la conduite : un média (ou du noir si `path` est vide).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CueData {
    pub name: String,
    pub path: Option<PathBuf>,
    /// Boucle (`looping`), points, fondus, volume, EQ et durée d'image : clic droit « Éditer… ».
    #[serde(flatten)]
    pub clip: Clip,
    pub transition: crate::protocol::Transition,
    pub follow: CueFollow,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CuelistData {
    pub cues: Vec<CueData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LecteurItem {
    pub path: PathBuf,
    /// Boucle (`looping`), points, fondus, volume, EQ et durée d'image : clic droit « Éditer… ».
    #[serde(flatten)]
    pub clip: Clip,
}

impl Default for LecteurItem {
    fn default() -> Self {
        Self { path: PathBuf::new(), clip: Clip::default() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LecteurData {
    pub items: Vec<LecteurItem>,
    pub loop_all: bool,
    pub shuffle: bool,
    pub transition: crate::protocol::Transition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaporamaData {
    pub folder: Option<PathBuf>,
    pub recursive: bool,
    pub shuffle: bool,
    pub looping: bool,
    pub photo_seconds: f64,
    pub fit: Fit,
    pub video_sound: bool,
}

impl Default for DiaporamaData {
    fn default() -> Self {
        Self { folder: None, recursive: true, shuffle: false, looping: true, photo_seconds: 5.0, fit: Fit::Auto, video_sound: false }
    }
}

impl Project {
    /// Lit un projet, réparé au besoin (voir `star_core::document::load_checked`).
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        document::load(path)
    }

    pub fn load_checked(path: &Path) -> anyhow::Result<(Self, bool)> {
        document::load_checked(path)
    }

    /// Remet dans leurs bornes les valeurs aberrantes.
    pub fn sanitize(&mut self) {
        let d = &mut self.diaporama;
        d.photo_seconds = if d.photo_seconds.is_finite() { d.photo_seconds.clamp(0.5, 3600.0) } else { 5.0 };
        for c in self.lecteur.items.iter_mut().map(|i| &mut i.clip).chain(self.cuelist.cues.iter_mut().map(|c| &mut c.clip)) {
            c.sanitize();
        }
    }

    /// Enregistre sur le disque de façon sûre (fichier temporaire puis renommage).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        document::save(self, path, false)
    }

    /// Comme `save`, en gardant la version précédente en `.bak`.
    pub fn save_with_backup(&self, path: &Path) -> anyhow::Result<()> {
        document::save(self, path, true)
    }

    /// Médias introuvables (sans doublon).
    pub fn missing_files(&mut self) -> Vec<PathBuf> {
        document::missing_files(self)
    }

    /// Projet copié ailleurs : médias cherchés à la même place par rapport au fichier projet.
    pub fn resolve_moved(&mut self, project_file: &Path) -> usize {
        document::resolve_moved(self, project_file)
    }

    /// Médias introuvables cherchés par leur nom dans `folder` et ses sous-dossiers.
    pub fn relocate(&mut self, folder: &Path) -> usize {
        document::relocate(self, folder)
    }
}

impl Document for Project {
    fn sanitize(&mut self) {
        Project::sanitize(self);
    }

    /// Chemins de tous les médias du projet.
    fn media_paths(&mut self) -> Vec<&mut PathBuf> {
        self.lecteur
            .items
            .iter_mut()
            .map(|i| &mut i.path)
            .chain(self.diaporama.folder.iter_mut())
            .chain(self.cuelist.cues.iter_mut().filter_map(|c| c.path.as_mut()))
            .collect()
    }
}

/// CueStar et ses dossiers sur cette machine.
pub const APP: star_core::app::App = star_core::app::App::new("CueStar");

/// Session restaurée au lancement (voir `star_core::session`).
pub type SessionLoad = star_core::session::SessionLoad<Project>;

pub use star_core::session::snapshot_session;

pub fn load_session(path: &Path) -> SessionLoad {
    star_core::session::load_session(path)
}

/// Fichier de session restauré automatiquement au lancement.
pub fn autosave_path() -> Option<PathBuf> {
    APP.session_path()
}

/// Dernier projet enregistré (pour « Enregistrer » au lancement suivant).
pub fn last_project_path() -> Option<PathBuf> {
    let f = autosave_path()?.with_file_name("dernier-projet.txt");
    let s = std::fs::read_to_string(f).ok()?;
    let p = PathBuf::from(s.trim());
    (!p.as_os_str().is_empty()).then_some(p)
}

pub fn remember_project_path(p: Option<&Path>) {
    if let Some(f) = autosave_path().map(|a| a.with_file_name("dernier-projet.txt")) {
        let _ = std::fs::write(f, p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
    }
}

#[cfg(test)]
mod golden {
    //! Fichiers de référence écrits par CueStar avant la mise en commun du code : ils doivent
    //! toujours se relire et se réécrire à l'identique (format stable).
    //! `STAR_UPDATE_FIXTURES=1 cargo test` les régénère : seulement pour un changement voulu.
    use super::*;
    use crate::protocol::{Transition, TransitionKind};
    use crate::remote::{ActionKind, ConsoleSettings, M32Settings, Mapping, MidiSettings, RemoteSettings, Trigger};

    fn check(name: &str, value: serde_json::Value) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        if std::env::var_os("STAR_UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let expected: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value, expected, "format de {name} modifié");
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
    }

    fn looped(on: bool) -> Clip {
        Clip { sound: star_core::sound::SoundSettings { looping: on, ..Default::default() }, image_secs: None }
    }

    /// Projet de la 0.0.1, d'avant l'éditeur d'élément.
    fn project_0_0_1() -> Project {
        let fade = Transition { kind: TransitionKind::Crossfade, secs: 2.0 };
        Project {
            name: "Gala".into(),
            lecteur: LecteurData {
                items: vec![LecteurItem { path: "/v/intro.mp4".into(), clip: looped(true) }, LecteurItem { path: "/v/logo.png".into(), clip: looped(false) }],
                loop_all: true,
                shuffle: true,
                transition: fade,
            },
            diaporama: DiaporamaData { folder: Some("/photos".into()), recursive: false, shuffle: true, looping: false, photo_seconds: 7.5, fit: Fit::Cover, video_sound: true },
            cuelist: CuelistData {
                cues: vec![
                    CueData { name: "Ouverture".into(), path: Some("/v/ouverture.mov".into()), clip: looped(false), transition: fade, follow: CueFollow::Continue },
                    CueData { name: "Noir".into(), path: None, clip: looped(true), transition: Transition { kind: TransitionKind::Black, secs: 1.0 }, follow: CueFollow::Stop },
                ],
            },
            notes: "**Acte 1**\nTop vidéo".into(),
        }
    }

    #[test]
    fn old_project_still_loads() {
        let back = Project::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projet-0.0.1.cuestar")).unwrap();
        assert_eq!(back, project_0_0_1());
    }

    #[test]
    fn project_format_is_stable() {
        let mut p = project_0_0_1();
        // Élément édité : points, boucle, fondus au noir, volume, EQ ; image à durée réglée.
        let s = &mut p.lecteur.items[0].clip.sound;
        s.volume_db = -4.5;
        s.start = 2.0;
        s.end = Some(40.0);
        s.loop_start = Some(10.0);
        s.loop_end = Some(30.0);
        s.fade_in = 1.5;
        s.fade_out = 2.0;
        s.eq.low_cut.on = true;
        s.eq.peak1.on = true;
        s.eq.peak1.gain_db = -3.0;
        p.lecteur.items[1].clip.image_secs = Some(8.0);
        p.lecteur.items[1].clip.sound.fade_out = 1.0;
        p.cuelist.cues[0].clip.sound.fade_in = 0.5;
        check("projet-0.0.2.cuestar", serde_json::to_value(&p).unwrap());
        let back = Project::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projet-0.0.2.cuestar")).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn machine_prefs_format_is_stable() {
        use crate::prefs::*;
        let m = MachinePrefs {
            decode: Decode::Cpu,
            render: RenderApi::OpenGL,
            light: true,
            screen: Some("DELL U2720Q".into()),
            windowed_if_single: false,
            cadence: Cadence::Resample,
            fps: Some(50.0),
            interpolation: true,
            adapt_refresh: true,
            warp: Warp { enabled: true, corners: [[0.1, 0.0], [0.9, 0.05], [1.0, 1.0], [0.0, 0.95]] },
            audio_device: Some("wasapi/Haut-parleurs".into()),
            volume: 80.0,
            audio_delay_ms: 40.0,
            audio_delay_unit: DelayUnit::Frames,
            language: star_core::i18n::Lang::En,
        };
        check("machine-0.1.1.json", serde_json::to_value(&m).unwrap());
        assert_eq!(serde_json::from_str::<MachinePrefs>(&fixture("machine-0.1.1.json")).unwrap(), m);
        // Fichier d'avant le choix du FPS : relu à l'identique, FPS automatique.
        let old: MachinePrefs = serde_json::from_str(&fixture("machine-0.0.2.json")).unwrap();
        assert_eq!(old, MachinePrefs { fps: None, audio_delay_ms: 0.0, audio_delay_unit: DelayUnit::Ms, ..m.clone() });
        // Fichier d'avant le choix de la langue : relu à l'identique, en français.
        let old: MachinePrefs = serde_json::from_str(&fixture("machine-0.0.1.json")).unwrap();
        assert_eq!(old, MachinePrefs { language: star_core::i18n::Lang::Fr, fps: None, audio_delay_ms: 0.0, audio_delay_unit: DelayUnit::Ms, ..m });
    }

    #[test]
    fn remote_and_osc_format_is_stable() {
        let r = RemoteSettings {
            midi: MidiSettings { enabled: true, port: Some("nanoKONTROL".into()) },
            console: ConsoleSettings { enabled: true, host: "10.0.0.2".into(), port: 49281 },
            m32: M32Settings { enabled: true, host: "10.0.0.3".into(), port: 10024 },
            mappings: vec![
                Mapping { trigger: Trigger::Note { channel: 1, note: 60 }, action: ActionKind::CueGo },
                Mapping { trigger: Trigger::Cc { channel: 2, cc: 7 }, action: ActionKind::LecteurPause },
                Mapping { trigger: Trigger::Program { channel: 1, program: 5 }, action: ActionKind::LecteurItem },
                Mapping { trigger: Trigger::Console { address: "MIXER:Current/InCh/Fader/On".into(), x: 3, y: 0 }, action: ActionKind::Black },
                Mapping { trigger: Trigger::Scene { number: 12 }, action: ActionKind::DiaporamaPlay },
                Mapping::default(),
            ],
        };
        check("remote-0.0.1.json", serde_json::to_value(&r).unwrap());
        assert_eq!(serde_json::from_str::<RemoteSettings>(&fixture("remote-0.0.1.json")).unwrap(), r);
        // Réglages OSC d'avant OSCQuery : relus avec les valeurs par défaut des nouveaux champs.
        let o = crate::osc::OscSettings { enabled: true, port: 9100, network: true, prefix: "/diaporama".into(), ..Default::default() };
        assert_eq!(serde_json::from_str::<crate::osc::OscSettings>(&fixture("osc-0.0.1.json")).unwrap(), o);
        let o = crate::osc::OscSettings { oscquery: true, oscquery_port: 9101, zeroconf: true, name: "Régie vidéo".into(), ..o };
        check("osc-0.0.2.json", serde_json::to_value(&o).unwrap());
        assert_eq!(serde_json::from_str::<crate::osc::OscSettings>(&fixture("osc-0.0.2.json")).unwrap(), o);
    }

    #[test]
    fn config_paths_are_stable() {
        let d = directories::ProjectDirs::from("com", "acetayls", "CueStar").unwrap();
        assert_eq!(autosave_path().unwrap(), d.config_dir().join("session.json"));
        assert_eq!(crate::prefs::path().unwrap(), d.config_dir().join("machine.json"));
        assert_eq!(crate::remote::settings_path().unwrap(), d.config_dir().join("remote.json"));
        assert_eq!(crate::osc::settings_path().unwrap(), d.config_dir().join("osc.json"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("cuestar-test-{}", std::process::id()));
        let f = dir.join("p.cuestar");
        let mut p = Project::default();
        p.name = "Spectacle".into();
        p.lecteur.items.push(LecteurItem { path: "/v/a.mp4".into(), clip: Clip { sound: star_core::sound::SoundSettings { looping: true, ..Default::default() }, image_secs: None } });
        p.lecteur.shuffle = true;
        p.save_with_backup(&f).unwrap();
        p.save_with_backup(&f).unwrap();
        assert!(star_core::document::with_suffix(&f, ".bak").exists());
        assert_eq!(Project::load(&f).unwrap(), p);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nulls_are_repaired() {
        let dir = std::env::temp_dir().join(format!("cuestar-null-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("p.cuestar");
        std::fs::write(&f, r#"{"name":"x","diaporama":{"photo_seconds":null}}"#).unwrap();
        let (p, repaired) = Project::load_checked(&f).unwrap();
        assert!(repaired);
        assert_eq!(p.diaporama.photo_seconds, 5.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
