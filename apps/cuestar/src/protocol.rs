//! Messages entre la régie et le moteur de sortie : une ligne JSON par message,
//! sur l'entrée standard (commandes) et la sortie standard (événements) du moteur.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    /// Charge un média (vidéo, image, son) et le lance, ou le met en attente sur sa première image.
    /// `looping` : en boucle ; `hold` : garde la dernière image à la fin (sinon fin = Ended).
    Load {
        path: String,
        paused: bool,
        looping: bool,
        hold: bool,
        #[serde(default)]
        transition: Transition,
        /// Réglages d'édition du média : points, boucle, fondus au noir, volume, EQ.
        #[serde(default)]
        clip: Clip,
    },
    /// Réglages d'édition du média en cours, modifiés pendant la lecture : volume, EQ et
    /// fondus s'appliquent tout de suite ; les points au prochain chargement.
    SetClip { clip: Clip },
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
    /// Réglages du diaporama en cours, appliqués à chaud (sans recharger le média affiché).
    SlideshowSettings { photo_seconds: f64, looping: bool, shuffle: bool, video_sound: bool },
    /// Sortie audio (nom mpv `audio-device` ; None = automatique), à chaud.
    AudioDevice { name: Option<String> },
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

/// Réglages d'édition d'un élément du Lecteur ou d'un cue, comme ceux d'un jingle de
/// JingleStar : volume, points d'entrée et de sortie, boucle, fondus (au noir pour l'image)
/// et égaliseur de la bande son. Enregistré tel quel dans le projet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clip {
    #[serde(flatten)]
    pub sound: star_core::sound::SoundSettings,
    /// Durée d'affichage d'une image fixe en secondes (None : jusqu'au média suivant).
    pub image_secs: Option<f64>,
}

impl Clip {
    /// Vrai si l'élément est lu tel quel (aucun réglage d'édition).
    pub fn is_plain(&self) -> bool {
        let s = &self.sound;
        s.volume_db == 0.0
            && s.start <= 0.0
            && s.end.is_none()
            && s.fade_in <= 0.0
            && s.fade_out <= 0.0
            && s.eq.is_flat()
            && self.image_secs.is_none()
    }

    /// Remet dans leurs bornes les valeurs aberrantes.
    pub fn sanitize(&mut self) {
        self.sound.sanitize(10.0);
        self.image_secs = self.image_secs.filter(|v| v.is_finite() && *v > 0.0).map(|v| v.clamp(0.1, 86_400.0));
    }
}

/// Passage d'un média au suivant.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transition {
    pub kind: TransitionKind,
    /// Durée totale en secondes (pour « Noir » : moitié en fondu sortant, moitié en entrant).
    pub secs: f64,
}

impl Default for Transition {
    fn default() -> Self {
        Self { kind: TransitionKind::Cut, secs: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// Coupe franche.
    #[default]
    Cut,
    /// Fondu au noir puis fondu d'ouverture.
    Black,
    /// Fondu enchaîné (demande le moteur de composition).
    Crossfade,
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
    /// Sorties audio disponibles : (nom mpv, description).
    AudioDevices { list: Vec<(String, String)> },
    Error { message: String },
    /// Touche appuyée dans la fenêtre de sortie (télécommande de présentation, clavier).
    Key { key: RemoteKey },
}

/// Touches d'une télécommande de présentation (type télécommande PowerPoint), reçues par la
/// fenêtre de sortie quand elle a le focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteKey {
    Next,
    Prev,
    First,
    Last,
    /// Écran noir (B, point, W, virgule), et retour à la diapositive.
    Black,
    /// F5 : depuis la première diapositive.
    StartFirst,
    /// Maj+F5 : depuis la diapositive sélectionnée.
    StartHere,
    Escape,
}
