//! Réglages des télécommandes de cette machine : entrée MIDI, consoles Yamaha et Midas.

use serde::{Deserialize, Serialize};

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
    /// Adresse IP de la console.
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
