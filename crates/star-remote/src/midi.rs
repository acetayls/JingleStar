//! Entrée MIDI : ouverture d'un port et décodage des notes, CC et program change.

use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver};
use midir::{Ignore, MidiInput, MidiInputConnection};

use star_core::i18n::tr;
use crate::input::InputEvent;
use star_core::app::App;

/// Ports d'entrée MIDI disponibles. `app` : nom du client MIDI (« JingleStar »).
pub fn port_names(app: App) -> Vec<String> {
    let Ok(input) = MidiInput::new(app.name) else { return Vec::new() };
    input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect()
}

/// Port ouvert ; la connexion se ferme quand on le libère.
pub struct MidiListener {
    pub events: Receiver<InputEvent>,
    pub port: String,
    _conn: MidiInputConnection<()>,
}

impl MidiListener {
    /// Ouvre le port nommé, ou le premier disponible, au nom de `app` (client « JingleStar »,
    /// entrée « jinglestar-in »).
    pub fn open(app: App, port: Option<&str>, repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let mut input = MidiInput::new(app.name).map_err(|e| format!("{} ({e})", tr("MIDI indisponible", "MIDI unavailable")))?;
        input.ignore(Ignore::All);
        let ports = input.ports();
        let found = ports
            .iter()
            .find(|p| port.is_none_or(|name| input.port_name(p).ok().as_deref() == Some(name)))
            .ok_or_else(|| match port {
                Some(name) => format!("{} : {name}", tr("port MIDI introuvable", "MIDI port not found")),
                None => tr("aucun port MIDI", "no MIDI port").to_string(),
            })?
            .clone();
        let name = input.port_name(&found).unwrap_or_default();
        let (tx, events) = unbounded();
        let conn = input
            .connect(
                &found,
                &format!("{}-in", app.name.to_lowercase()),
                move |_, bytes, _| {
                    if let Some(ev) = parse(bytes) {
                        let _ = tx.send(ev);
                        repaint();
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(Self { events, port: name, _conn: conn })
    }
}

/// Décode un message MIDI ; les autres messages (horloge, aftertouch…) sont ignorés.
pub fn parse(bytes: &[u8]) -> Option<InputEvent> {
    let (&status, data) = bytes.split_first()?;
    let channel = status & 0x0F;
    let d = |i: usize| data.get(i).map(|b| b & 0x7F);
    match status & 0xF0 {
        0x80 => Some(InputEvent::Note { channel, note: d(0)?, velocity: 0 }),
        0x90 => Some(InputEvent::Note { channel, note: d(0)?, velocity: d(1)? }),
        0xB0 => Some(InputEvent::Cc { channel, cc: d(0)?, value: d(1)? }),
        0xC0 => Some(InputEvent::Program { channel, program: d(0)? }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_messages() {
        assert_eq!(parse(&[0x92, 36, 100]), Some(InputEvent::Note { channel: 2, note: 36, velocity: 100 }));
        assert_eq!(parse(&[0x82, 36, 64]), Some(InputEvent::Note { channel: 2, note: 36, velocity: 0 }));
        assert_eq!(parse(&[0xB0, 7, 127]), Some(InputEvent::Cc { channel: 0, cc: 7, value: 127 }));
        assert_eq!(parse(&[0xCF, 4]), Some(InputEvent::Program { channel: 15, program: 4 }));
        assert_eq!(parse(&[0xF8]), None);
        assert_eq!(parse(&[0x90, 36]), None);
        assert_eq!(parse(&[]), None);
    }
}
