//! Préférences propres à cette machine (moteur de décodage, rendu, écran de sortie).
//! Elles ne voyagent pas avec le spectacle : un même projet tourne sur deux PC réglés différemment.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decode {
    /// GPU si possible, repli automatique sur le CPU.
    #[default]
    Auto,
    /// Décodage matériel forcé (NVIDIA, Intel, AMD, Apple…).
    Gpu,
    /// Décodage logiciel uniquement.
    Cpu,
}

impl Decode {
    pub const ALL: [Decode; 3] = [Decode::Auto, Decode::Gpu, Decode::Cpu];
    pub fn label(self) -> &'static str {
        match self {
            Decode::Auto => "Auto (GPU, repli CPU)",
            Decode::Gpu => "GPU forcé",
            Decode::Cpu => "CPU seul",
        }
    }
    /// Valeur de l'option `hwdec` de mpv.
    pub fn mpv_hwdec(self) -> &'static str {
        match self {
            Decode::Auto => "auto-safe",
            Decode::Gpu => {
                if cfg!(windows) {
                    "d3d11va,nvdec,dxva2"
                } else if cfg!(target_os = "macos") {
                    "videotoolbox"
                } else {
                    "vaapi,nvdec,vdpau"
                }
            }
            Decode::Cpu => "no",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderApi {
    #[default]
    Auto,
    Direct3D11,
    Vulkan,
    OpenGL,
}

impl RenderApi {
    pub fn available() -> &'static [RenderApi] {
        if cfg!(windows) {
            &[RenderApi::Auto, RenderApi::Direct3D11, RenderApi::Vulkan, RenderApi::OpenGL]
        } else {
            &[RenderApi::Auto, RenderApi::Vulkan, RenderApi::OpenGL]
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RenderApi::Auto => "Auto",
            RenderApi::Direct3D11 => "Direct3D 11",
            RenderApi::Vulkan => "Vulkan",
            RenderApi::OpenGL => "OpenGL",
        }
    }
    /// Valeur de l'option `gpu-api` de mpv (None = choix de mpv).
    pub fn mpv_gpu_api(self) -> Option<&'static str> {
        match self {
            RenderApi::Auto => None,
            RenderApi::Direct3D11 => Some("d3d11"),
            RenderApi::Vulkan => Some("vulkan"),
            RenderApi::OpenGL => Some("opengl"),
        }
    }
}

/// Façon de caler les images de la vidéo sur les rafraîchissements de l'écran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cadence {
    /// Choix par fichier : recalage de vitesse quand l'écran tombe presque juste, sinon synchro audio.
    #[default]
    Auto,
    /// Minutage sur l'horloge du son, sans correction.
    Audio,
    /// Vidéo très légèrement accélérée ou ralentie (moins de 1 %) pour tomber juste sur l'écran.
    Resample,
}

/// Unité d'affichage du délai de sortie audio (il est toujours enregistré en millisecondes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelayUnit {
    #[default]
    Ms,
    /// Images, à la cadence du FPS choisi (25 i/s en automatique).
    Frames,
    /// Mètres de propagation du son dans l'air (343 m/s).
    Meters,
}

/// Vitesse du son dans l'air à 20 °C, en m/s.
pub const SOUND_SPEED: f64 = 343.0;

impl DelayUnit {
    pub const ALL: [DelayUnit; 3] = [DelayUnit::Ms, DelayUnit::Frames, DelayUnit::Meters];
    pub fn label(self) -> &'static str {
        match self {
            DelayUnit::Ms => "ms",
            DelayUnit::Frames => "images",
            DelayUnit::Meters => "mètres",
        }
    }
    /// Millisecondes → valeur dans cette unité.
    pub fn from_ms(self, ms: f64, fps: f64) -> f64 {
        match self {
            DelayUnit::Ms => ms,
            DelayUnit::Frames => ms * fps / 1000.0,
            DelayUnit::Meters => ms * SOUND_SPEED / 1000.0,
        }
    }
    /// Valeur dans cette unité → millisecondes.
    pub fn to_ms(self, v: f64, fps: f64) -> f64 {
        match self {
            DelayUnit::Ms => v,
            DelayUnit::Frames => v * 1000.0 / fps,
            DelayUnit::Meters => v * 1000.0 / SOUND_SPEED,
        }
    }
}

/// Fréquences proposées dans le choix « FPS » des Préférences (Hz, entières comme sous Windows).
pub const FPS_CHOICES: [f64; 9] = [24.0, 25.0, 30.0, 48.0, 50.0, 60.0, 100.0, 120.0, 144.0];

/// Libellé d'un choix « FPS » : None = automatique.
pub fn fps_label(fps: Option<f64>) -> String {
    match fps {
        None => "Automatique (selon la vidéo)".into(),
        Some(f) => format!("{f:.0} i/s (écran forcé à {f:.0} Hz)"),
    }
}

impl Cadence {
    pub const ALL: [Cadence; 3] = [Cadence::Auto, Cadence::Audio, Cadence::Resample];
    pub fn label(self) -> &'static str {
        match self {
            Cadence::Auto => "Automatique (selon la vidéo et l'écran)",
            Cadence::Audio => "Synchro audio",
            Cadence::Resample => "Recalage de vitesse",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachinePrefs {
    pub decode: Decode,
    pub render: RenderApi,
    /// Rendu léger (vo=gpu, profil rapide) pour les petites machines.
    pub light: bool,
    /// Écran de sortie, par nom de moniteur. None = premier écran secondaire.
    pub screen: Option<String>,
    /// Avec un seul écran : sortie dans une fenêtre plutôt qu'en plein écran sur la régie.
    pub windowed_if_single: bool,
    /// Calage des images sur l'écran.
    pub cadence: Cadence,
    /// FPS choisi : l'écran de sortie est forcé sur cette fréquence à l'ouverture de la sortie
    /// (rétablie en quittant). None = automatique.
    pub fps: Option<f64>,
    /// Interpolation : fond deux images voisines pour lisser un rythme irrégulier (24 i/s sur 60 Hz…).
    pub interpolation: bool,
    /// Passe l'écran de sortie sur une fréquence qui tombe juste pour la vidéo, rétablie en quittant.
    pub adapt_refresh: bool,
    /// Mapping 4 coins de l'image sur l'écran de sortie.
    pub warp: Warp,
    /// Sortie audio (nom mpv `audio-device`) ; None = sortie par défaut du système.
    pub audio_device: Option<String>,
    /// Volume général de la sortie (0 à 100).
    pub volume: f64,
    /// Délai de la sortie audio, en millisecondes (son retardé par rapport à l'image).
    pub audio_delay_ms: f64,
    /// Unité d'affichage du délai.
    pub audio_delay_unit: DelayUnit,
    /// Langue de l'interface.
    pub language: star_core::i18n::Lang,
}

/// Mapping 4 coins : position des coins de l'image (haut-gauche, haut-droit, bas-droit,
/// bas-gauche), en fraction de l'écran (0 à 1, origine en haut à gauche).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Warp {
    pub enabled: bool,
    pub corners: [[f32; 2]; 4],
}

impl Warp {
    pub const IDENTITY: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    /// Coins à appliquer, ou None si le mapping est inactif ou neutre.
    pub fn active(&self) -> Option<[[f32; 2]; 4]> {
        (self.enabled && self.corners != Self::IDENTITY).then_some(self.corners)
    }
}

impl Default for Warp {
    fn default() -> Self {
        Self { enabled: false, corners: Self::IDENTITY }
    }
}

impl Default for MachinePrefs {
    fn default() -> Self {
        Self { decode: Decode::Auto, render: RenderApi::Auto, light: false, screen: None, windowed_if_single: true, cadence: Cadence::Auto, fps: None, interpolation: false, adapt_refresh: false, warp: Warp::default(), audio_device: None, volume: 100.0, audio_delay_ms: 0.0, audio_delay_unit: DelayUnit::Ms, language: star_core::i18n::Lang::Fr }
    }
}

pub(crate) fn path() -> Option<PathBuf> {
    crate::project::APP.config_file("machine.json")
}

impl MachinePrefs {
    /// Mêmes réglages de moteur (ceux qui demandent de relancer la sortie).
    pub fn same_engine(&self, o: &Self) -> bool {
        let mut a = self.clone();
        a.warp = o.warp;
        a.audio_device = o.audio_device.clone();
        a.volume = o.volume;
        a.audio_delay_ms = o.audio_delay_ms;
        a.audio_delay_unit = o.audio_delay_unit;
        a == *o
    }

    pub fn load() -> Self {
        path().and_then(|p| star_core::prefs::load(&p)).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Some(p) = path() {
            let _ = star_core::prefs::save(&p, self);
        }
    }
}
