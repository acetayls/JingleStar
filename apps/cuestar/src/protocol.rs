//! Messages entre la régie et le moteur de sortie : une ligne JSON par message,
//! sur l'entrée standard (commandes) et la sortie standard (événements) du moteur.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    /// Charge un média (vidéo, image, son) et le lance, ou le met en attente sur sa première image.
    /// `looping` : en boucle ; `hold` : garde la dernière image à la fin (sinon fin = Ended).
    Load { path: String, paused: bool, looping: bool, hold: bool },
    Play,
    Pause,
    /// Arrête la lecture : écran noir.
    Stop,
    Seek { seconds: f64 },
    /// Volume 0 à 100.
    Volume { value: f64 },
    /// Affiche une mire de test (None : la retire, écran noir).
    Pattern { kind: Option<PatternKind> },
    /// Mapping 4 coins (None : image droite).
    Warp { corners: Option<[[f32; 2]; 4]> },
    /// Lance un diaporama : le moteur de sortie tient le minutage et l'enchaînement.
    Slideshow { items: Vec<String>, photo_seconds: f64, looping: bool, shuffle: bool, fit: Fit, video_sound: bool },
    /// Change l'ajustement des images en cours de diaporama.
    SetFit { fit: Fit },
    Next,
    Prev,
    /// Va au média n° `index` (0 = premier) du diaporama.
    Goto { index: usize },
    /// Retour image vers la régie, à la cadence de la vidéo.
    Monitor { on: bool },
    Quit,
}

/// Mires de test de la sortie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind {
    Bars,
    Moving,
    Grid,
    White,
    Grey,
    Black,
    Red,
    Green,
    Blue,
}

impl PatternKind {
    pub const ALL: [PatternKind; 9] = [
        PatternKind::Bars,
        PatternKind::Moving,
        PatternKind::Grid,
        PatternKind::White,
        PatternKind::Grey,
        PatternKind::Black,
        PatternKind::Red,
        PatternKind::Green,
        PatternKind::Blue,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PatternKind::Bars => "Barres SMPTE",
            PatternKind::Moving => "Mire animée",
            PatternKind::Grid => "Grille",
            PatternKind::White => "Blanc",
            PatternKind::Grey => "Gris 50 %",
            PatternKind::Black => "Noir",
            PatternKind::Red => "Rouge",
            PatternKind::Green => "Vert",
            PatternKind::Blue => "Bleu",
        }
    }
    /// Source mpv (filtre lavfi) à la taille de la sortie.
    pub fn lavfi(self, w: u32, h: u32) -> String {
        let solid = |c: &str| format!("av://lavfi:color=c={c}:size={w}x{h}:rate=25");
        match self {
            PatternKind::Bars => format!("av://lavfi:smptehdbars=size={w}x{h}:rate=25"),
            PatternKind::Moving => format!("av://lavfi:testsrc2=size={w}x{h}:rate=60"),
            PatternKind::Grid => format!(
                "av://lavfi:color=c=black:size={w}x{h}:rate=25,drawgrid=w={}:h={}:t=2:c=white,drawbox=x=0:y=0:w={w}:h={h}:t=4:c=white",
                (w / 16).max(1),
                (h / 9).max(1)
            ),
            PatternKind::White => solid("white"),
            PatternKind::Grey => solid("0x808080"),
            PatternKind::Black => solid("black"),
            PatternKind::Red => solid("red"),
            PatternKind::Green => solid("0x00ff00"),
            PatternKind::Blue => solid("blue"),
        }
    }
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
    /// Images par seconde : vidéo affichée et rafraîchissement de l'écran.
    Fps { video: f64, display: f64 },
    Ended,
    Idle,
    /// Position dans le diaporama (index à partir de 0). `count` = 0 : diaporama terminé.
    Slide { index: usize, count: usize, path: String },
    Error { message: String },
}
