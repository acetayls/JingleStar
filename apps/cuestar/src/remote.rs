//! Télécommandes MIDI et consoles (Yamaha, Midas) : correspondances « déclencheur → action »,
//! reprises de JingleStar et traduites en commandes communes avec l'OSC.

use crate::osc::Msg;
use rosc::OscType;
use serde::{Deserialize, Serialize};

/// Textes de JingleStar en deux langues : CueStar est en français.
fn tr<'a>(fr: &'a str, _en: &'a str) -> &'a str {
    fr
}
/// Valeur « -∞ » des niveaux de la console (centièmes de dB).
pub const CONSOLE_MIN: i32 = -32768;

/// Message reçu d'une télécommande. Les canaux MIDI sont comptés à partir de 0.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// Vélocité 0 : touche relâchée.
    Note { channel: u8, note: u8, velocity: u8 },
    Cc { channel: u8, cc: u8, value: u8 },
    Program { channel: u8, program: u8 },
    /// Paramètre de la console (ex. « MIXER:Current/InCh/Fader/On », canal x, y),
    /// avec la valeur précédente quand elle est connue.
    Console { address: String, x: u32, y: u32, value: i32, previous: Option<i32> },
    /// Rappel de scène sur la console.
    Scene { number: u32 },
}

impl InputEvent {
    /// Déclencheur correspondant, pour le mode « apprendre ».
    pub fn trigger(&self) -> Trigger {
        match self {
            InputEvent::Note { channel, note, .. } => Trigger::Note { channel: *channel, note: *note },
            InputEvent::Cc { channel, cc, .. } => Trigger::Cc { channel: *channel, cc: *cc },
            InputEvent::Program { channel, program } => Trigger::Program { channel: *channel, program: *program },
            InputEvent::Console { address, x, y, .. } => Trigger::Console { address: address.clone(), x: *x, y: *y },
            InputEvent::Scene { number } => Trigger::Scene { number: *number },
        }
    }

    /// Un relâché de touche ne sert pas à apprendre.
    pub fn learnable(&self) -> bool {
        !matches!(self, InputEvent::Note { velocity: 0, .. })
    }

    /// Appui sur un bouton : note enfoncée, CC ≥ 64, program change, rappel de scène,
    /// changement d'état d'un ON de la console, fader qui quitte -∞.
    fn press(&self) -> bool {
        match self {
            InputEvent::Note { velocity, .. } => *velocity > 0,
            InputEvent::Cc { value, .. } => *value >= 64,
            InputEvent::Program { .. } | InputEvent::Scene { .. } => true,
            InputEvent::Console { address, value, previous, .. } => {
                if is_on_param(address) {
                    // Une touche (ON, USER DEFINED affectée à un ON) bascule l'état à chaque appui.
                    previous.map_or(true, |p| p != *value)
                } else {
                    previous.is_some_and(|p| p <= CONSOLE_MIN) && *value > CONSOLE_MIN
                }
            }
        }
    }

    /// Volume en dB dans `range` : CC et vélocité de 0 à 127, niveau de fader de la console.
    fn level_db(&self, range: (f32, f32)) -> Option<f32> {
        let scale = |v: u8| range.0 + (range.1 - range.0) * v as f32 / 127.0;
        match self {
            InputEvent::Cc { value, .. } => Some(scale(*value)),
            InputEvent::Note { velocity, .. } if *velocity > 0 => Some(scale(*velocity)),
            InputEvent::Console { value, .. } if *value <= CONSOLE_MIN => Some(range.0),
            InputEvent::Console { value, .. } => Some((*value as f32 / 100.0).clamp(range.0, range.1)),
            _ => None,
        }
    }

    /// État imposé à une option (CC, ON de la console) ; `None` : l'appui la bascule.
    fn state(&self) -> Option<bool> {
        match self {
            InputEvent::Cc { value, .. } => Some(*value >= 64),
            InputEvent::Console { address, value, .. } if is_on_param(address) => Some(*value > 0),
            _ => None,
        }
    }

    /// Numéro de morceau compté à partir de 1 : valeur du CC, program change + 1, numéro de scène.
    fn track(&self) -> Option<usize> {
        let n = match self {
            InputEvent::Cc { value, .. } => *value as usize,
            InputEvent::Program { program, .. } => *program as usize + 1,
            InputEvent::Scene { number } => *number as usize,
            _ => 0,
        };
        (n >= 1).then_some(n)
    }

    pub fn label(&self) -> String {
        match self {
            InputEvent::Note { velocity, .. } => format!("{} ({} {velocity})", self.trigger().label(), tr("vélocité", "velocity")),
            InputEvent::Cc { value, .. } => format!("{} = {value}", self.trigger().label()),
            InputEvent::Console { value, .. } => format!("{} = {}", self.trigger().label(), console_value(*value)),
            _ => self.trigger().label(),
        }
    }
}

/// Touche qui bascule à chaque appui : ON d'un canal (Yamaha « …/On », Midas « …/mix/on »)
/// ou groupe de mute Midas.
fn is_on_param(address: &str) -> bool {
    address.ends_with("/On") || address.ends_with("/on") || address.starts_with("/config/mute/")
}

fn console_value(v: i32) -> String {
    if v <= CONSOLE_MIN {
        "-∞".into()
    } else {
        v.to_string()
    }
}

/// Ce qui déclenche une correspondance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Trigger {
    /// Pas encore appris.
    #[default]
    Unset,
    Note { channel: u8, note: u8 },
    Cc { channel: u8, cc: u8 },
    Program { channel: u8, program: u8 },
    Console { address: String, x: u32, y: u32 },
    Scene { number: u32 },
}

impl Trigger {
    /// `any_number` : tout program change (du canal) ou toute scène, dont le numéro devient la valeur.
    pub fn matches(&self, ev: &InputEvent, any_number: bool) -> bool {
        match (self, ev) {
            (Trigger::Note { channel, note }, InputEvent::Note { channel: c, note: n, .. }) => channel == c && note == n,
            (Trigger::Cc { channel, cc }, InputEvent::Cc { channel: c, cc: n, .. }) => channel == c && cc == n,
            (Trigger::Program { channel, program }, InputEvent::Program { channel: c, program: p }) => {
                channel == c && (any_number || program == p)
            }
            (Trigger::Console { address, x, y }, InputEvent::Console { address: a, x: ex, y: ey, .. }) => {
                address == a && x == ex && y == ey
            }
            (Trigger::Scene { number }, InputEvent::Scene { number: n }) => any_number || number == n,
            _ => false,
        }
    }

    pub fn label(&self) -> String {
        let ch = |c: &u8| format!("{} {}", tr("can.", "ch."), c + 1);
        match self {
            Trigger::Unset => tr("(non appris)", "(not learned)").into(),
            Trigger::Note { channel, note } => format!("Note {note} · {}", ch(channel)),
            Trigger::Cc { channel, cc } => format!("CC {cc} · {}", ch(channel)),
            Trigger::Program { channel, program } => format!("Program {} · {}", program + 1, ch(channel)),
            // Adresses OSC des Midas M32 / Behringer X32.
            Trigger::Console { address, .. } if address.starts_with('/') => format!("M32 · {}", address.trim_start_matches('/')),
            Trigger::Console { address, x, y } => {
                let short = address.strip_prefix("MIXER:Current/").unwrap_or(address);
                if *y == 0 {
                    format!("QL · {short} {}", x + 1)
                } else {
                    format!("QL · {short} {}/{}", x + 1, y + 1)
                }
            }
            Trigger::Scene { number } => format!("{} {number}", tr("Console · Scène", "Console · Scene")),
        }
    }
}

/// Actions qu'une télécommande peut déclencher dans CueStar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    #[default]
    LecteurGo,
    LecteurPause,
    LecteurStop,
    LecteurNext,
    LecteurPrevious,
    LecteurItem,
    LecteurShuffle,
    LecteurLoop,
    DiaporamaPlay,
    DiaporamaPause,
    DiaporamaStop,
    DiaporamaNext,
    DiaporamaPrevious,
    CueGo,
    CueNext,
    CuePrevious,
    CuePause,
    CueStop,
    Black,
}

impl ActionKind {
    pub const ALL: [ActionKind; 19] = [
        ActionKind::LecteurGo,
        ActionKind::LecteurPause,
        ActionKind::LecteurStop,
        ActionKind::LecteurNext,
        ActionKind::LecteurPrevious,
        ActionKind::LecteurItem,
        ActionKind::LecteurShuffle,
        ActionKind::LecteurLoop,
        ActionKind::DiaporamaPlay,
        ActionKind::DiaporamaPause,
        ActionKind::DiaporamaStop,
        ActionKind::DiaporamaNext,
        ActionKind::DiaporamaPrevious,
        ActionKind::CueGo,
        ActionKind::CueNext,
        ActionKind::CuePrevious,
        ActionKind::CuePause,
        ActionKind::CueStop,
        ActionKind::Black,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ActionKind::LecteurGo => "Lecteur : GO",
            ActionKind::LecteurPause => "Lecteur : pause / reprise",
            ActionKind::LecteurStop => "Lecteur : arrêt",
            ActionKind::LecteurNext => "Lecteur : suivant",
            ActionKind::LecteurPrevious => "Lecteur : précédent",
            ActionKind::LecteurItem => "Lecteur : élément n° (valeur)",
            ActionKind::LecteurShuffle => "Lecteur : aléatoire",
            ActionKind::LecteurLoop => "Lecteur : tout boucler",
            ActionKind::DiaporamaPlay => "Diaporama : lancer / reprendre",
            ActionKind::DiaporamaPause => "Diaporama : pause / reprise",
            ActionKind::DiaporamaStop => "Diaporama : arrêt",
            ActionKind::DiaporamaNext => "Diaporama : suivant",
            ActionKind::DiaporamaPrevious => "Diaporama : précédent",
            ActionKind::CueGo => "Cuelist : GO",
            ActionKind::CueNext => "Cuelist : cue suivant",
            ActionKind::CuePrevious => "Cuelist : cue précédent",
            ActionKind::CuePause => "Cuelist : pause / reprise",
            ActionKind::CueStop => "Cuelist : arrêt",
            ActionKind::Black => "Noir (couper la sortie)",
        }
    }
}

/// Options du Lecteur qu'un appui bascule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Shuffle,
    Loop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Mapping {
    pub trigger: Trigger,
    pub action: ActionKind,
}

fn msg(path: &str, args: Vec<OscType>) -> Msg {
    Msg { path: path.into(), args }
}

impl Mapping {
    pub fn matches(&self, ev: &InputEvent) -> bool {
        self.trigger.matches(ev, self.action == ActionKind::LecteurItem)
    }

    /// Commande à exécuter pour `ev`, sous forme d'adresse OSC interne (sans préfixe) ;
    /// `flag` donne l'état courant d'une option du Lecteur.
    pub fn command(&self, ev: &InputEvent, flag: impl Fn(Flag) -> bool) -> Option<Msg> {
        let press = |p: &str| ev.press().then(|| msg(p, vec![]));
        let set = |f: Flag, p: &str| Some(msg(p, vec![OscType::Bool(ev.state().unwrap_or_else(|| !flag(f)))]));
        match self.action {
            ActionKind::LecteurGo => press("lecteur/go"),
            ActionKind::LecteurPause => press("lecteur/toggle"),
            ActionKind::LecteurStop => press("lecteur/stop"),
            ActionKind::LecteurNext => press("lecteur/next"),
            ActionKind::LecteurPrevious => press("lecteur/prev"),
            ActionKind::LecteurItem => ev.track().map(|n| msg("lecteur/goto", vec![OscType::Int(n as i32)])),
            ActionKind::LecteurShuffle if ev.state().is_some() || ev.press() => set(Flag::Shuffle, "lecteur/shuffle"),
            ActionKind::LecteurLoop if ev.state().is_some() || ev.press() => set(Flag::Loop, "lecteur/loop"),
            ActionKind::DiaporamaPlay => press("diaporama/play"),
            ActionKind::DiaporamaPause => press("diaporama/toggle"),
            ActionKind::DiaporamaStop => press("diaporama/stop"),
            ActionKind::DiaporamaNext => press("diaporama/next"),
            ActionKind::DiaporamaPrevious => press("diaporama/prev"),
            ActionKind::CueGo => press("cuelist/go"),
            ActionKind::CueNext => press("cuelist/next"),
            ActionKind::CuePrevious => press("cuelist/prev"),
            ActionKind::CuePause => press("cuelist/toggle"),
            ActionKind::CueStop => press("cuelist/stop"),
            ActionKind::Black => press("noir"),
            _ => None,
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

/// Télécommandes de cette machine (fichier remote.json), comme les préférences de JingleStar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RemoteSettings {
    pub midi: MidiSettings,
    pub console: ConsoleSettings,
    pub m32: M32Settings,
    pub mappings: Vec<Mapping>,
}

fn settings_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "CueStar").map(|d| d.config_dir().join("remote.json"))
}

impl RemoteSettings {
    pub fn load() -> Self {
        settings_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Some(p) = settings_path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(n: u8, v: u8) -> InputEvent {
        InputEvent::Note { channel: 0, note: n, velocity: v }
    }

    #[test]
    fn note_go_on_press_only() {
        let m = Mapping { trigger: Trigger::Note { channel: 0, note: 36 }, action: ActionKind::CueGo };
        assert!(m.matches(&note(36, 100)));
        assert_eq!(m.command(&note(36, 100), |_| false).map(|c| c.path), Some("cuelist/go".into()));
        assert!(m.command(&note(36, 0), |_| false).is_none());
    }

    #[test]
    fn program_change_picks_item() {
        let m = Mapping { trigger: Trigger::Program { channel: 0, program: 0 }, action: ActionKind::LecteurItem };
        let ev = InputEvent::Program { channel: 0, program: 4 };
        assert!(m.matches(&ev));
        let c = m.command(&ev, |_| false).unwrap();
        assert_eq!(c.path, "lecteur/goto");
        assert_eq!(c.f64(), Some(5.0));
    }

    #[test]
    fn note_toggles_shuffle() {
        let m = Mapping { trigger: Trigger::Note { channel: 0, note: 1 }, action: ActionKind::LecteurShuffle };
        assert!(m.command(&note(1, 90), |_| false).unwrap().flag());
        assert!(!m.command(&note(1, 90), |_| true).unwrap().flag());
    }
}