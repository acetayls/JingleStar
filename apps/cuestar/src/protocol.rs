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
    /// Lance un diaporama : le moteur de sortie tient le minutage et l'enchaînement.
    Slideshow { items: Vec<String>, photo_seconds: f64, looping: bool, shuffle: bool, fit: Fit, video_sound: bool },
    /// Change l'ajustement des images en cours de diaporama.
    SetFit { fit: Fit },
    Next,
    Prev,
    /// Va au média n° `index` (0 = premier) du diaporama.
    Goto { index: usize },
    Quit,
}

/// Ajustement d'une image à l'écran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fit {
    /// Image entière, bandes noires si besoin.
    #[default]
    Contain,
    /// Remplit l'écran en recadrant.
    Cover,
    /// Remplit si le format est proche de celui de l'écran, sinon image entière.
    Auto,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Contain, Fit::Cover, Fit::Auto];
    pub fn label(self) -> &'static str {
        match self {
            Fit::Contain => "Image entière (bandes noires)",
            Fit::Cover => "Remplir l'écran (recadrer)",
            Fit::Auto => "Automatique",
        }
    }
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
    /// Position dans le diaporama (index à partir de 0). `count` = 0 : diaporama terminé.
    Slide { index: usize, count: usize, path: String },
    Error { message: String },
}
