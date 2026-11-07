//! Sources des télécommandes en marche dans la régie : entrée MIDI (rebranchée toute seule),
//! consoles Yamaha et Midas, dernier message reçu et mode « apprendre ». Repris du contrôleur
//! de JingleStar ; les assignations et leurs actions restent dans chaque appli.

use std::sync::Arc;
use std::time::{Duration, Instant};

use star_core::app::App;
use star_core::i18n::tr;

use crate::console::ConsoleClient;
use crate::input::{InputEvent, Trigger};
use crate::m32::M32Client;
use crate::midi::MidiListener;
use crate::settings::{ConsoleSettings, M32Settings, MidiSettings};

/// Ce que devient un message reçu pendant l'apprentissage.
#[derive(Debug, Clone, PartialEq)]
pub enum Learn {
    /// Pas d'apprentissage en cours : le message déclenche les assignations.
    Off,
    /// Apprentissage en cours, message ignoré (touche relâchée…).
    Ignored,
    /// L'assignation `.0` apprend ce déclencheur.
    Learned(usize, Trigger),
}

pub struct Inputs {
    app: App,
    repaint: Arc<dyn Fn() + Send + Sync>,
    /// Entrée MIDI ouverte, ou l'erreur d'ouverture.
    pub midi: Option<MidiListener>,
    pub midi_error: Option<String>,
    midi_retry: Instant,
    /// Écoute de la console Yamaha.
    pub console: Option<ConsoleClient>,
    /// Écoute de la console Midas M32 / Behringer X32.
    pub m32: Option<M32Client>,
    /// Assignation en attente d'apprentissage.
    pub learn: Option<usize>,
    /// Dernier message reçu d'une télécommande, pour la fenêtre des télécommandes.
    pub last_input: Option<String>,
}

impl Inputs {
    /// Aucune source ouverte : les `restart_*` les lancent selon les réglages.
    pub fn new(app: App, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { app, repaint, midi: None, midi_error: None, midi_retry: Instant::now(), console: None, m32: None, learn: None, last_input: None }
    }

    pub fn app(&self) -> App {
        self.app
    }

    /// (Ré)ouvre le port MIDI selon `settings`.
    pub fn restart_midi(&mut self, settings: &MidiSettings) {
        self.midi = None;
        self.midi_error = None;
        self.midi_retry = Instant::now();
        if !settings.enabled {
            return;
        }
        match MidiListener::open(self.app, settings.port.as_deref(), self.repaint.clone()) {
            Ok(l) => self.midi = Some(l),
            Err(e) => self.midi_error = Some(e),
        }
    }

    /// (Re)lance l'écoute de la console Yamaha selon `settings`.
    pub fn restart_console(&mut self, settings: &ConsoleSettings) {
        self.console = None;
        if settings.enabled {
            self.console = Some(ConsoleClient::start(settings.clone(), self.repaint.clone()));
        }
    }

    /// (Re)lance l'écoute de la M32 selon `settings`.
    pub fn restart_m32(&mut self, settings: &M32Settings) {
        self.m32 = None;
        if settings.enabled {
            self.m32 = Some(M32Client::start(settings.clone(), self.repaint.clone()));
        }
    }

    /// Messages reçus depuis le dernier appel, avec leur source (« MIDI », « QL », « M32 »).
    /// Un port MIDI débranché puis rebranché est rouvert : `midi` sert à le retrouver.
    pub fn poll(&mut self, midi: &MidiSettings) -> Vec<(&'static str, InputEvent)> {
        let mut events = Vec::new();
        if let Some(l) = self.midi.as_ref() {
            events.extend(l.events.try_iter().map(|e| ("MIDI", e)));
        }
        if let Some(c) = self.console.as_ref() {
            events.extend(c.events.try_iter().map(|e| ("QL", e)));
        }
        if let Some(c) = self.m32.as_ref() {
            events.extend(c.events.try_iter().map(|e| ("M32", e)));
        }
        // Port débranché puis rebranché : on retente régulièrement.
        if midi.enabled && self.midi_retry.elapsed() > Duration::from_secs(3) {
            self.midi_retry = Instant::now();
            let ports = crate::midi::port_names(self.app);
            match self.midi.as_ref().map(|l| l.port.clone()) {
                Some(port) if !ports.contains(&port) => {
                    self.midi = None;
                    self.midi_error = Some(format!("{} : {port}", tr("port MIDI débranché", "MIDI port unplugged")));
                }
                None if !ports.is_empty() => self.restart_midi(midi),
                _ => {}
            }
        }
        events
    }

    /// Note le message comme dernier reçu, puis dit s'il sert à l'apprentissage en cours.
    pub fn receive(&mut self, source: &str, ev: &InputEvent) -> Learn {
        // Les déclencheurs des consoles portent déjà leur source (« QL · … », « M32 · … »).
        let labelled = matches!(ev, InputEvent::Console { .. } | InputEvent::Scene { .. });
        self.last_input = Some(if labelled { ev.label() } else { format!("{source} · {}", ev.label()) });
        match self.learn {
            None => Learn::Off,
            Some(_) if !ev.learnable() => Learn::Ignored,
            Some(i) => {
                self.learn = None;
                Learn::Learned(i, ev.trigger())
            }
        }
    }

    /// L'assignation `i` vient d'être supprimée : l'apprentissage suit la liste.
    pub fn mapping_removed(&mut self, i: usize) {
        self.learn = match self.learn {
            Some(l) if l == i => None,
            Some(l) if l > i => Some(l - 1),
            other => other,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_takes_the_next_press() {
        let mut inputs = Inputs::new(App::new("Test"), Arc::new(|| {}));
        let press = InputEvent::Note { channel: 0, note: 60, velocity: 100 };
        let release = InputEvent::Note { channel: 0, note: 60, velocity: 0 };
        assert_eq!(inputs.receive("MIDI", &press), Learn::Off);
        assert_eq!(inputs.last_input.as_deref(), Some(format!("MIDI · {}", press.label()).as_str()));
        inputs.learn = Some(2);
        assert_eq!(inputs.receive("MIDI", &release), Learn::Ignored);
        assert_eq!(inputs.receive("MIDI", &press), Learn::Learned(2, press.trigger()));
        assert_eq!(inputs.learn, None);
        inputs.learn = Some(3);
        inputs.mapping_removed(1);
        assert_eq!(inputs.learn, Some(2));
        inputs.mapping_removed(2);
        assert_eq!(inputs.learn, None);
    }
}
