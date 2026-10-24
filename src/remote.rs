//! Télécommandes MIDI et console Yamaha : correspondances « déclencheur → action »,
//! partagées par les deux sources et traduites en commandes communes avec l'OSC.

use serde::{Deserialize, Serialize};

use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use crate::i18n::tr;
use crate::osc::{OscCommand, JINGLE_DB, MODULE_DB};

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

fn is_on_param(address: &str) -> bool {
    address.ends_with("/On")
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
            Trigger::Console { address, x, y } => {
                let short = address.strip_prefix("MIXER:Current/").unwrap_or(address);
                if *y == 0 {
                    format!("QL · {short} {}", x + 1)
                } else {
                    format!("QL · {short} {}/{}", x + 1, y + 1)
                }
            }
            Trigger::Scene { number } => format!("QL · {} {number}", tr("Scène", "Scene")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    #[default]
    JingleToggle,
    JinglePlay,
    JingleStop,
    JingleVolume,
    ModuleStop,
    ModuleVolume,
    PlaylistPlay,
    PlaylistStop,
    PlaylistNext,
    PlaylistPrevious,
    PlaylistTrack,
    PlaylistShuffle,
    PlaylistLoop,
    PlaylistAutoAdvance,
    StopAll,
}

impl ActionKind {
    pub const ALL: [ActionKind; 15] = [
        ActionKind::JingleToggle,
        ActionKind::JinglePlay,
        ActionKind::JingleStop,
        ActionKind::JingleVolume,
        ActionKind::ModuleStop,
        ActionKind::ModuleVolume,
        ActionKind::PlaylistPlay,
        ActionKind::PlaylistStop,
        ActionKind::PlaylistNext,
        ActionKind::PlaylistPrevious,
        ActionKind::PlaylistTrack,
        ActionKind::PlaylistShuffle,
        ActionKind::PlaylistLoop,
        ActionKind::PlaylistAutoAdvance,
        ActionKind::StopAll,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ActionKind::JingleToggle => tr("Jingle : lancer / arrêter", "Jingle: play / stop"),
            ActionKind::JinglePlay => tr("Jingle : lancer", "Jingle: play"),
            ActionKind::JingleStop => tr("Jingle : arrêter", "Jingle: stop"),
            ActionKind::JingleVolume => tr("Jingle : volume", "Jingle: volume"),
            ActionKind::ModuleStop => tr("Module : tout arrêter", "Module: stop all"),
            ActionKind::ModuleVolume => tr("Module : volume", "Module: volume"),
            ActionKind::PlaylistPlay => tr("Playlist : lecture", "Playlist: play"),
            ActionKind::PlaylistStop => tr("Playlist : arrêt", "Playlist: stop"),
            ActionKind::PlaylistNext => tr("Playlist : suivant", "Playlist: next"),
            ActionKind::PlaylistPrevious => tr("Playlist : précédent", "Playlist: previous"),
            ActionKind::PlaylistTrack => tr("Playlist : morceau n° (valeur)", "Playlist: track no. (value)"),
            ActionKind::PlaylistShuffle => tr("Playlist : aléatoire", "Playlist: shuffle"),
            ActionKind::PlaylistLoop => tr("Playlist : boucle", "Playlist: loop"),
            ActionKind::PlaylistAutoAdvance => tr("Playlist : enchaînement", "Playlist: auto-advance"),
            ActionKind::StopAll => tr("Tout arrêter", "Stop everything"),
        }
    }

    pub fn needs_module(self) -> bool {
        self != ActionKind::StopAll
    }

    pub fn needs_jingle(self) -> bool {
        matches!(self, ActionKind::JingleToggle | ActionKind::JinglePlay | ActionKind::JingleStop | ActionKind::JingleVolume)
    }
}

/// Options de playlist qu'un appui bascule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Shuffle,
    Loop,
    AutoAdvance,
}

/// Une correspondance ; module et jingle comptés à partir de 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Mapping {
    pub trigger: Trigger,
    pub action: ActionKind,
    pub module: usize,
    pub jingle: usize,
}

impl Mapping {
    pub fn matches(&self, ev: &InputEvent) -> bool {
        self.trigger.matches(ev, self.action == ActionKind::PlaylistTrack)
    }

    /// Commande à exécuter pour `ev` ; `flag` donne l'état courant d'une option de playlist.
    pub fn command(&self, ev: &InputEvent, flag: impl Fn(usize, Flag) -> bool) -> Option<OscCommand> {
        let m = self.module.min(MAX_MODULES - 1);
        let s = self.jingle.min(JINGLE_SLOTS - 1);
        let press = |c: OscCommand| ev.press().then_some(c);
        let set = |f: Flag, make: fn(usize, bool) -> OscCommand| Some(make(m, ev.state().unwrap_or_else(|| !flag(m, f))));
        match self.action {
            ActionKind::StopAll => press(OscCommand::StopAll),
            ActionKind::ModuleStop => press(OscCommand::ModuleStop(m)),
            ActionKind::ModuleVolume => ev.level_db(MODULE_DB).map(|db| OscCommand::ModuleVolume(m, db)),
            ActionKind::JingleToggle => press(OscCommand::JingleToggle(m, s)),
            ActionKind::JinglePlay => press(OscCommand::JinglePlay(m, s)),
            ActionKind::JingleStop => press(OscCommand::JingleStop(m, s)),
            ActionKind::JingleVolume => ev.level_db(JINGLE_DB).map(|db| OscCommand::JingleVolume(m, s, db)),
            ActionKind::PlaylistPlay => press(OscCommand::PlaylistPlay(m, None)),
            ActionKind::PlaylistStop => press(OscCommand::PlaylistStop(m)),
            ActionKind::PlaylistNext => press(OscCommand::PlaylistStep(m, 1)),
            ActionKind::PlaylistPrevious => press(OscCommand::PlaylistStep(m, -1)),
            ActionKind::PlaylistTrack => ev.track().map(|n| OscCommand::PlaylistPlay(m, Some(n - 1))),
            ActionKind::PlaylistShuffle if ev.state().is_some() || ev.press() => set(Flag::Shuffle, OscCommand::PlaylistShuffle),
            ActionKind::PlaylistLoop if ev.state().is_some() || ev.press() => set(Flag::Loop, OscCommand::PlaylistLoop),
            ActionKind::PlaylistAutoAdvance if ev.state().is_some() || ev.press() => {
                set(Flag::AutoAdvance, OscCommand::PlaylistAutoAdvance)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(trigger: Trigger, action: ActionKind, module: usize, jingle: usize) -> Mapping {
        Mapping { trigger, action, module, jingle }
    }

    fn note(n: u8, v: u8) -> InputEvent {
        InputEvent::Note { channel: 0, note: n, velocity: v }
    }

    fn cc(n: u8, v: u8) -> InputEvent {
        InputEvent::Cc { channel: 0, cc: n, value: v }
    }

    fn on(x: u32, value: i32, previous: Option<i32>) -> InputEvent {
        InputEvent::Console { address: "MIXER:Current/InCh/Fader/On".into(), x, y: 0, value, previous }
    }

    #[test]
    fn note_triggers_on_press_only() {
        let m = map(note(36, 100).trigger(), ActionKind::JingleToggle, 1, 4);
        assert!(m.matches(&note(36, 100)));
        assert!(!m.matches(&note(37, 100)));
        assert!(!m.matches(&InputEvent::Note { channel: 1, note: 36, velocity: 100 }));
        assert_eq!(m.command(&note(36, 100), |_, _| false), Some(OscCommand::JingleToggle(1, 4)));
        assert_eq!(m.command(&note(36, 0), |_, _| false), None);
    }

    #[test]
    fn cc_sets_volume_and_buttons() {
        let vol = map(cc(7, 0).trigger(), ActionKind::ModuleVolume, 0, 0);
        assert_eq!(vol.command(&cc(7, 127), |_, _| false), Some(OscCommand::ModuleVolume(0, MODULE_DB.1)));
        assert_eq!(vol.command(&cc(7, 0), |_, _| false), Some(OscCommand::ModuleVolume(0, MODULE_DB.0)));
        let stop = map(cc(20, 0).trigger(), ActionKind::StopAll, 0, 0);
        assert_eq!(stop.command(&cc(20, 127), |_, _| false), Some(OscCommand::StopAll));
        assert_eq!(stop.command(&cc(20, 0), |_, _| false), None);
        let shuffle = map(cc(21, 0).trigger(), ActionKind::PlaylistShuffle, 2, 0);
        assert_eq!(shuffle.command(&cc(21, 0), |_, _| true), Some(OscCommand::PlaylistShuffle(2, false)));
    }

    #[test]
    fn program_change_picks_track() {
        let pc = |p| InputEvent::Program { channel: 3, program: p };
        let m = map(pc(0).trigger(), ActionKind::PlaylistTrack, 1, 0);
        assert!(m.matches(&pc(9)), "toute valeur de program change choisit le morceau");
        assert_eq!(m.command(&pc(4), |_, _| false), Some(OscCommand::PlaylistPlay(1, Some(4))));
        let next = map(pc(5).trigger(), ActionKind::PlaylistNext, 1, 0);
        assert!(!next.matches(&pc(4)));
        assert_eq!(next.command(&pc(5), |_, _| false), Some(OscCommand::PlaylistStep(1, 1)));
    }

    #[test]
    fn note_toggles_flag() {
        let m = map(note(40, 1).trigger(), ActionKind::PlaylistLoop, 0, 0);
        assert_eq!(m.command(&note(40, 90), |_, _| true), Some(OscCommand::PlaylistLoop(0, false)));
        assert_eq!(m.command(&note(40, 0), |_, _| true), None);
    }

    #[test]
    fn console_on_key_and_fader() {
        let m = map(on(31, 1, None).trigger(), ActionKind::JingleToggle, 0, 0);
        assert!(m.matches(&on(31, 0, Some(1))));
        assert!(!m.matches(&on(30, 0, Some(1))));
        // Chaque appui bascule le ON : les deux sens déclenchent.
        assert_eq!(m.command(&on(31, 0, Some(1)), |_, _| false), Some(OscCommand::JingleToggle(0, 0)));
        assert_eq!(m.command(&on(31, 1, Some(0)), |_, _| false), Some(OscCommand::JingleToggle(0, 0)));
        let shuffle = map(on(31, 1, None).trigger(), ActionKind::PlaylistShuffle, 0, 0);
        assert_eq!(shuffle.command(&on(31, 1, Some(0)), |_, _| false), Some(OscCommand::PlaylistShuffle(0, true)));

        let fader = |value, previous| InputEvent::Console {
            address: "MIXER:Current/InCh/Fader/Level".into(),
            x: 2,
            y: 0,
            value,
            previous,
        };
        let vol = map(fader(0, None).trigger(), ActionKind::ModuleVolume, 0, 0);
        assert_eq!(vol.command(&fader(-1050, None), |_, _| false), Some(OscCommand::ModuleVolume(0, -10.5)));
        assert_eq!(vol.command(&fader(CONSOLE_MIN, None), |_, _| false), Some(OscCommand::ModuleVolume(0, MODULE_DB.0)));
        let go = map(fader(0, None).trigger(), ActionKind::PlaylistPlay, 0, 0);
        assert_eq!(go.command(&fader(-2000, Some(CONSOLE_MIN)), |_, _| false), Some(OscCommand::PlaylistPlay(0, None)));
        assert_eq!(go.command(&fader(-1000, Some(-2000)), |_, _| false), None);
    }

    #[test]
    fn scene_recall() {
        let m = map(Trigger::Scene { number: 12 }, ActionKind::JinglePlay, 0, 2);
        assert!(m.matches(&InputEvent::Scene { number: 12 }));
        assert!(!m.matches(&InputEvent::Scene { number: 13 }));
        let track = map(Trigger::Scene { number: 1 }, ActionKind::PlaylistTrack, 0, 0);
        assert_eq!(track.command(&InputEvent::Scene { number: 7 }, |_, _| false), Some(OscCommand::PlaylistPlay(0, Some(6))));
    }

    #[test]
    fn mapping_roundtrips_json() {
        let m = map(Trigger::Console { address: "MIXER:Current/InCh/Fader/On".into(), x: 3, y: 0 }, ActionKind::StopAll, 0, 0);
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Mapping>(&json).unwrap(), m);
    }
}
