//! Messages entre la régie et le moteur de sortie : une ligne JSON par message,
//! sur l'entrée standard (commandes) et la sortie standard (événements) du moteur.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    /// Charge un média (vidéo, image, son) et le lance, ou le met en attente sur sa première image.
    Load { path: String, paused: bool },
    Play,
    Pause,
    /// Arrête la lecture : écran noir.
    Stop,
    Seek { seconds: f64 },
    /// Volume 0 à 100.
    Volume { value: f64 },
    /// Affiche ou masque la mire (repérage de l'écran de sortie).
    Pattern { on: bool },
    Quit,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Monitor {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
    pub primary: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    /// Fenêtre de sortie ouverte. `mpv` = version chargée, ou None si libmpv est introuvable.
    Ready { mpv: Option<String>, monitor: Monitor, fullscreen: bool, monitors: Vec<Monitor> },
    MpvMissing { detail: String },
    Loaded { path: String },
    Position { time: f64, duration: f64 },
    Paused { paused: bool },
    /// Décodeur réellement utilisé (« d3d11va », « no »…).
    Hwdec { name: String },
    /// Images perdues ou retardées par le moteur de rendu.
    Drops { dropped: i64, delayed: i64 },
    Ended,
    Idle,
    Error { message: String },
}
