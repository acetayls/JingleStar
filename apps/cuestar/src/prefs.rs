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
        Self { decode: Decode::Auto, render: RenderApi::Auto, light: false, screen: None, windowed_if_single: true, cadence: Cadence::Auto, interpolation: false, adapt_refresh: false, warp: Warp::default(), audio_device: None, volume: 100.0 }
    }
}

fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "CueStar").map(|d| d.config_dir().join("machine.json"))
}

impl MachinePrefs {
    /// Mêmes réglages de moteur (ceux qui demandent de relancer la sortie).
    pub fn same_engine(&self, o: &Self) -> bool {
        let mut a = self.clone();
        a.warp = o.warp;
        a.audio_device = o.audio_device.clone();
        a.volume = o.volume;
        a == *o
    }

    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Some(p) = path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}
