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
    /// Synchronisation sur la fréquence de l'écran (lecture fluide à 50, 59,94, 60 Hz…).
    pub display_sync: bool,
}

impl Default for MachinePrefs {
    fn default() -> Self {
        Self { decode: Decode::Auto, render: RenderApi::Auto, light: false, screen: None, windowed_if_single: true, display_sync: true }
    }
}

fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "CueStar").map(|d| d.config_dir().join("machine.json"))
}

impl MachinePrefs {
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
