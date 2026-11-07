//! Messages des télécommandes (MIDI, consoles Yamaha et Midas) et déclencheurs qu'on leur
//! assigne. Repris de JingleStar.

use serde::{Deserialize, Serialize};
use star_core::i18n::tr;

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
    pub fn press(&self) -> bool {
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
    pub fn level_db(&self, range: (f32, f32)) -> Option<f32> {
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
    pub fn state(&self) -> Option<bool> {
        match self {
            InputEvent::Cc { value, .. } => Some(*value >= 64),
            InputEvent::Console { address, value, .. } if is_on_param(address) => Some(*value > 0),
            _ => None,
        }
    }

    /// Numéro de morceau compté à partir de 1 : valeur du CC, program change + 1, numéro de scène.
    pub fn track(&self) -> Option<usize> {
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
pub fn is_on_param(address: &str) -> bool {
    address.ends_with("/On") || address.ends_with("/on") || address.starts_with("/config/mute/")
}

fn console_value(v: i32) -> String {
    if v <= CONSOLE_MIN {
        "-∞".into()
    } else {
        v.to_string()
    }
}

/// Ce qui déclenche une assignation.
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

/// Reprise en douceur d'un volume : le fader distant n'agit qu'une fois arrivé à la valeur
/// actuelle de l'appli (à 1 dB près, ou en la franchissant). Évite un saut de volume quand
/// le fader de la console ou du contrôleur n'est pas à la même position.
#[derive(Debug, Clone, Default)]
pub struct Pickup {
    /// Dernière valeur appliquée par ce fader ; si le volume a changé depuis (souris, OSC),
    /// il faut le rejoindre à nouveau.
    applied: Option<f32>,
    /// Position précédente du fader.
    last: Option<f32>,
    /// Valeur à rejoindre, tant que le fader ne l'a pas atteinte.
    pub waiting: Option<f32>,
}

impl Pickup {
    /// Le fader passe à `target` alors que le volume vaut `current` : faut-il l'appliquer ?
    pub fn take(&mut self, current: f32, target: f32) -> bool {
        let engaged = self.applied.is_some_and(|a| (a - current).abs() < 0.01)
            || (target - current).abs() <= 1.0
            || self.last.is_some_and(|l| (l - current).signum() != (target - current).signum());
        self.last = Some(target);
        if engaged {
            self.applied = Some(target);
            self.waiting = None;
        } else {
            self.applied = None;
            self.waiting = Some(current);
        }
        engaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_avoids_jumps() {
        let mut p = Pickup::default();
        // Appli à 0 dB, fader de la console à -20 dB : rien ne bouge.
        assert!(!p.take(0.0, -20.0));
        assert_eq!(p.waiting, Some(0.0));
        assert!(!p.take(0.0, -10.0));
        // Le fader franchit 0 dB : il prend la main.
        assert!(p.take(0.0, 2.0));
        assert_eq!(p.waiting, None);
        // Il suit ensuite librement.
        assert!(p.take(2.0, -15.0));
        // Volume changé à la souris : il faut le rejoindre de nouveau.
        assert!(!p.take(-3.0, -14.0));
        // À 1 dB près, il reprend.
        assert!(p.take(-3.0, -3.5));
        // Premier mouvement déjà au bon endroit.
        assert!(Pickup::default().take(-6.0, -6.4));
    }
}
