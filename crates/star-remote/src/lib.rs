//! Télécommandes communes aux applis Star (JingleStar, CueStar), extraites de JingleStar :
//! entrée MIDI, écoute des consoles Yamaha (protocole texte) et Midas M32 / X32 (OSC),
//! messages reçus et déclencheurs qu'on leur assigne.
//!
//! Les actions déclenchées sont propres à chaque appli.

pub mod console;
pub mod input;
pub mod inputs;
pub mod m32;
pub mod midi;
pub mod osc;
pub mod settings;
#[cfg(feature = "ui")]
pub mod ui;

pub use input::{InputEvent, Pickup, Trigger, CONSOLE_MIN};
pub use inputs::{Inputs, Learn};
pub use settings::{ConsoleSettings, M32Settings, MidiSettings};
