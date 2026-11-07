//! Télécommandes MIDI et consoles (Yamaha, Midas) : correspondances « déclencheur → action »,
//! reprises de JingleStar et traduites en commandes communes avec l'OSC.

use crate::osc::Msg;
use rosc::OscType;
use serde::{Deserialize, Serialize};

pub use star_remote::{ConsoleSettings, InputEvent, M32Settings, MidiSettings, Trigger};

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
    ResetDrops,
}

impl ActionKind {
    pub const ALL: [ActionKind; 20] = [
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
        ActionKind::ResetDrops,
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
            ActionKind::ResetDrops => "Compteur d'images perdues : remise à zéro",
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
            ActionKind::ResetDrops => press("output/drops/reset"),
            _ => None,
        }
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

pub(crate) fn settings_path() -> Option<std::path::PathBuf> {
    crate::project::APP.config_file("remote.json")
}

impl RemoteSettings {
    pub fn load() -> Self {
        settings_path().and_then(|p| star_core::prefs::load(&p)).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Some(p) = settings_path() {
            let _ = star_core::prefs::save(&p, self);
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