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
        /// Avec `hold` : la dernière image reste, mais la fin est quand même signalée (Ended),
        /// pour enchaîner sur le cue suivant sans passer par le noir.
        #[serde(default)]
        signal_end: bool,
    },
    /// Prépare un média sans l'afficher : ouvert sur le lecteur en attente, arrêté sur sa
    /// première image. Le `Load` suivant du même média (coupe ou fondu enchaîné) part alors
    /// tout de suite. Sans effet avec un seul lecteur (affichage de secours).
    Prepare {
        path: String,
        looping: bool,
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
    /// Délai de la sortie audio en millisecondes, à chaud.
    AudioDelay { ms: f64 },
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
    /// Mire de synchro (timecode, mouvement, images perdues, bip son / image) à 25 i/s.
    Sync25,
    /// Même mire à 60 i/s.
    Sync60,
    Grid,
    White,
    Grey,
    Black,
    Red,
    Green,
    Blue,
}

impl PatternKind {
    pub const ALL: [PatternKind; 11] = [
        PatternKind::Bars,
        PatternKind::Moving,
        PatternKind::Sync25,
        PatternKind::Sync60,
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
            PatternKind::Sync25 => "Synchro 25 i/s",
            PatternKind::Sync60 => "Synchro 60 i/s",
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
            PatternKind::Sync25 => format!("av://lavfi:{}", sync_graph(w, h, 25)),
            PatternKind::Sync60 => format!("av://lavfi:{}", sync_graph(w, h, 60)),
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

/// Mire de synchro, en graphe lavfi (image `out0`, son `out1`) :
/// - barres de couleur, zones de sécurité 93 % et 90 %, repères au milieu des bords (recadrage),
///   carré en bas à gauche (proportions) ;
/// - petites barres qui vont et viennent (fluidité du mouvement) et timecode ;
/// - carrés allumés une image sur 2, 3, 4 et 5 (images perdues ou doublées) ;
/// - carré « bip » allumé une image par seconde, en même temps qu'un bip de 1 kHz à −18 dBFS ;
///   le reste du temps, un son continu de 440 Hz à −20 dBFS (coupures du son).
pub fn sync_graph(w: u32, h: u32, fps: u32) -> String {
    let s = h as f64 / 1080.0;
    let px = |v: f64| (v * s).round().max(1.0) as i64;
    let (w, h) = (w as i64, h as i64);
    let (q, fs) = (px(80.0), px(36.0));
    let rh = px(130.0);
    let rw = (rh as f64 * 16.0 / 9.0).round() as i64;
    let band_y = (h as f64 * 0.20).round() as i64;
    let band_h = (h as f64 * 0.44).round() as i64;
    let line = px(3.0).max(2);
    let pct = |v: i64, p: f64| (v as f64 * p).round() as i64;
    let mut g = vec![format!("smptehdbars=size={w}x{h}:rate={fps}[b]"), format!("smptebars=size={rw}x{rh}:rate={fps}[r]")];
    for k in 0..5 {
        g.push(format!("color=c=white:size={q}x{q}:rate={fps}[c{k}]"));
    }
    let (m, t) = (px(60.0), px(12.0));
    let sq = pct(h, 0.16);
    g.push(format!(
        "[b]drawbox=x=0:y={band_y}:w={w}:h={band_h}:c=black:t=fill,\
         drawbox=x={}:y={}:w={}:h={}:c=yellow:t={line},\
         drawbox=x={}:y={}:w={}:h={}:c=cyan:t={line},\
         drawbox=x={}:y=0:w={t}:h={m}:c=white:t=fill,drawbox=x={}:y={}:w={t}:h={m}:c=white:t=fill,\
         drawbox=x=0:y={}:w={m}:h={t}:c=white:t=fill,drawbox=x={}:y={}:w={m}:h={t}:c=white:t=fill,\
         drawbox=x={}:y={}:w={sq}:h={sq}:c=white:t={}[v0]",
        pct(w, 0.035), pct(h, 0.035), pct(w, 0.93), pct(h, 0.93),
        pct(w, 0.05), pct(h, 0.05), pct(w, 0.9), pct(h, 0.9),
        (w - t) / 2, (w - t) / 2, h - m,
        (h - t) / 2, w - m, (h - t) / 2,
        pct(h, 0.07), h - pct(h, 0.07) - sq, px(6.0).max(2),
    ));
    let rect_y = band_y + px(60.0);
    g.push(format!("[v0][r]overlay=x='(W-w)*(0.5+0.45*sin(2*PI*t/4))':y={rect_y}[v1]"));
    let gap = px(30.0);
    let x0 = (w - (5 * q + 4 * gap)) / 2;
    let y0 = band_y + band_h - q - px(25.0);
    let mut last = "v1".to_string();
    for (k, every) in [2, 3, 4, 5, fps].into_iter().enumerate() {
        g.push(format!("[{last}][c{k}]overlay=x={}:y={y0}:enable='eq(mod(n,{every}),0)'[w{k}]", x0 + k as i64 * (q + gap)));
        last = format!("w{k}");
    }
    let mut txt = format!(
        "[{last}]drawtext=timecode='00\\:00\\:00\\:00':rate={fps}:fontsize={}:fontcolor=white:x=(w-tw)/2:y={}",
        px(64.0),
        rect_y + rh + px(10.0)
    );
    for (k, label) in ["1/2", "1/3", "1/4", "1/5", "bip"].iter().enumerate() {
        txt += &format!(",drawtext=text='{label}':fontsize={fs}:fontcolor=white:x={}+({q}-tw)/2:y={}", x0 + k as i64 * (q + gap), y0 - fs - px(6.0));
    }
    txt += &format!(",drawtext=text='CueStar  {fps} i/s':fontsize={fs}:fontcolor=white:x=(w-tw)/2:y={}[out0]", band_y + px(10.0));
    g.push(txt);
    g.push(format!(
        "aevalsrc='if(lt(mod(t\\,1)\\,1/{fps})\\,0.126*sin(2*PI*1000*t)\\,0.1*sin(2*PI*440*t))':s=48000:c=stereo[out1]"
    ));
    g.join(";")
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
    /// La première image du média chargé vient d'être affichée sur l'écran de sortie.
    OnScreen,
    Ended,
    Idle,
    /// Position dans le diaporama (index à partir de 0). `count` = 0 : diaporama terminé.
    Slide { index: usize, count: usize, path: String },
    /// Sorties audio disponibles : (nom mpv, description).
    AudioDevices { list: Vec<(String, String)> },
    Error { message: String },
}

#[cfg(test)]
mod tests_mire {
    /// Le graphe de la mire de synchro passe dans ffmpeg (test sauté sans ffmpeg).
    #[test]
    fn sync_graph_runs() {
        for (w, h, fps) in [(1920, 1080, 25), (1280, 720, 60)] {
            let g = super::sync_graph(w, h, fps);
            let Ok(out) = std::process::Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", &g, "-map", "0:v", "-map", "0:a", "-t", "0.2", "-f", "null", "-"])
                .output()
            else {
                return;
            };
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
    }
}
