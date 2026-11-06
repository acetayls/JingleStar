//! Télécommandes MIDI et console Yamaha : assignations « déclencheur → action »,
//! partagées par les deux sources et traduites en commandes communes avec l'OSC.

use serde::{Deserialize, Serialize};

use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use star_core::i18n::tr;
use crate::osc::{OscCommand, JINGLE_DB, MODULE_DB};
use crate::project::ModuleMode;

pub use star_remote::{InputEvent, Pickup, Trigger};

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
    PlaylistPause,
    PlaylistNext,
    PlaylistPrevious,
    PlaylistTrack,
    PlaylistShuffle,
    PlaylistLoop,
    CueGo,
    CueNext,
    CuePrevious,
    CuePause,
    StopAll,
}

impl ActionKind {
    pub const ALL: [ActionKind; 19] = [
        ActionKind::JingleToggle,
        ActionKind::JinglePlay,
        ActionKind::JingleStop,
        ActionKind::JingleVolume,
        ActionKind::ModuleStop,
        ActionKind::ModuleVolume,
        ActionKind::PlaylistPlay,
        ActionKind::PlaylistStop,
        ActionKind::PlaylistPause,
        ActionKind::PlaylistNext,
        ActionKind::PlaylistPrevious,
        ActionKind::PlaylistTrack,
        ActionKind::PlaylistShuffle,
        ActionKind::PlaylistLoop,
        ActionKind::CueGo,
        ActionKind::CueNext,
        ActionKind::CuePrevious,
        ActionKind::CuePause,
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
            ActionKind::PlaylistPause => tr("Playlist : pause / reprise", "Playlist: pause / resume"),
            ActionKind::PlaylistNext => tr("Playlist : suivant", "Playlist: next"),
            ActionKind::PlaylistPrevious => tr("Playlist : précédent", "Playlist: previous"),
            ActionKind::PlaylistTrack => tr("Playlist : morceau n° (valeur)", "Playlist: track no. (value)"),
            ActionKind::PlaylistShuffle => tr("Playlist : aléatoire", "Playlist: shuffle"),
            ActionKind::PlaylistLoop => tr("Playlist : boucle", "Playlist: loop"),
            ActionKind::CueGo => tr("Cuelist : GO", "Cuelist: GO"),
            ActionKind::CueNext => tr("Cuelist : cue suivant", "Cuelist: next cue"),
            ActionKind::CuePrevious => tr("Cuelist : cue précédent", "Cuelist: previous cue"),
            ActionKind::CuePause => tr("Cuelist : pause / reprise", "Cuelist: pause / resume"),
            ActionKind::StopAll => tr("Tout arrêter", "Stop everything"),
        }
    }

    pub fn needs_module(self) -> bool {
        self != ActionKind::StopAll
    }

    /// Mode que le module doit avoir pour que l'action agisse (aucun : tous les modes).
    pub fn required_mode(self) -> Option<ModuleMode> {
        use ActionKind::*;
        match self {
            JingleToggle | JinglePlay | JingleStop | JingleVolume => Some(ModuleMode::Jingle),
            PlaylistPlay | PlaylistStop | PlaylistPause | PlaylistNext | PlaylistPrevious | PlaylistTrack | PlaylistShuffle
            | PlaylistLoop => Some(ModuleMode::Playlist),
            CueGo | CueNext | CuePrevious | CuePause => Some(ModuleMode::Go),
            ModuleStop | ModuleVolume | StopAll => None,
        }
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
}

/// Une assignation ; module et jingle comptés à partir de 0.
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
            ActionKind::PlaylistPause => press(OscCommand::PlaylistPause(m, None)),
            ActionKind::PlaylistNext => press(OscCommand::PlaylistStep(m, 1)),
            ActionKind::PlaylistPrevious => press(OscCommand::PlaylistStep(m, -1)),
            ActionKind::CueGo => press(OscCommand::CueGo(Some(m))),
            ActionKind::CueNext => press(OscCommand::CueStep(m, 1)),
            ActionKind::CuePrevious => press(OscCommand::CueStep(m, -1)),
            ActionKind::CuePause => press(OscCommand::CuePause(m, None)),
            ActionKind::PlaylistTrack => ev.track().map(|n| OscCommand::PlaylistPlay(m, Some(n - 1))),
            ActionKind::PlaylistShuffle if ev.state().is_some() || ev.press() => set(Flag::Shuffle, OscCommand::PlaylistShuffle),
            ActionKind::PlaylistLoop if ev.state().is_some() || ev.press() => set(Flag::Loop, OscCommand::PlaylistLoop),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use star_remote::CONSOLE_MIN;

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
    fn m32_mute_key_toggles() {
        let ev = |value, previous| InputEvent::Console { address: "/ch/05/mix/on".into(), x: 0, y: 0, value, previous };
        let m = map(ev(1, None).trigger(), ActionKind::JingleToggle, 0, 3);
        assert_eq!(m.command(&ev(0, Some(1)), |_, _| false), Some(OscCommand::JingleToggle(0, 3)));
        assert_eq!(m.command(&ev(1, Some(0)), |_, _| false), Some(OscCommand::JingleToggle(0, 3)));
        assert_eq!(ev(1, None).trigger().label(), "M32 · ch/05/mix/on");
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
