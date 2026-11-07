//! Télécommandes, comme dans JingleStar : entrée MIDI, consoles Yamaha et Midas M32 / X32,
//! correspondances « déclencheur → action » avec le mode apprendre, raccourcis clavier.
//! Les sources et la fenêtre viennent de `star_remote` ; les actions passent par les mêmes
//! commandes que l'OSC.

use crate::osc::Msg;
use crate::remote::{ActionKind, ConsoleSettings, Flag, M32Settings, Mapping, RemoteSettings, Trigger};
use egui::{RichText, Ui};
use star_core::i18n::tr;
use star_remote::ui::{self as remote_ui, Apply, MappingRows};
use star_remote::{Inputs, Learn};
use std::sync::Arc;

/// Sources des télécommandes et correspondances, en marche dans la régie.
pub struct Remotes {
    pub settings: RemoteSettings,
    pub inputs: Inputs,
    // Saisie en cours dans la fenêtre.
    midi_ports: Vec<String>,
    console_draft: Option<ConsoleSettings>,
    m32_draft: Option<M32Settings>,
}

impl Remotes {
    pub fn new(repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let settings = RemoteSettings::load();
        let mut inputs = Inputs::new(crate::project::APP, repaint);
        inputs.restart_midi(&settings.midi);
        inputs.restart_console(&settings.console);
        inputs.restart_m32(&settings.m32);
        Self { settings, inputs, midi_ports: Vec::new(), console_draft: None, m32_draft: None }
    }

    fn save(&self) {
        self.settings.save();
    }

    /// Correspondance en attente d'apprentissage.
    pub fn cancel_learn(&mut self) {
        self.inputs.learn = None;
    }

    /// Messages reçus depuis la dernière image, traduits en commandes. `flag` : état d'une
    /// option du Lecteur (pour les touches qui la basculent).
    pub fn poll(&mut self, flag: impl Fn(Flag) -> bool) -> Vec<Msg> {
        let mut out = Vec::new();
        for (source, ev) in self.inputs.poll(&self.settings.midi) {
            match self.inputs.receive(source, &ev) {
                Learn::Off => {
                    for m in self.settings.mappings.iter().filter(|m| m.matches(&ev)) {
                        if let Some(c) = m.command(&ev, &flag) {
                            out.push(c);
                        }
                    }
                }
                Learn::Ignored => {}
                Learn::Learned(i, trigger) => {
                    if let Some(m) = self.settings.mappings.get_mut(i) {
                        m.trigger = trigger;
                    }
                    self.save();
                }
            }
        }
        out
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum Tab {
    #[default]
    Osc,
    Midi,
    Yamaha,
    M32,
    Mappings,
    Keyboard,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Osc, Tab::Midi, Tab::Yamaha, Tab::M32, Tab::Mappings, Tab::Keyboard];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Osc => "OSC",
            Tab::Midi => "MIDI",
            Tab::Yamaha => "Yamaha",
            Tab::M32 => "Midas M32 / X32",
            Tab::Mappings => tr("Correspondances", "Mappings"),
            Tab::Keyboard => tr("Raccourcis", "Shortcuts"),
        }
    }
}

impl Remotes {
    pub fn refresh_ports(&mut self) {
        self.midi_ports = star_remote::midi::port_names(crate::project::APP);
    }

    pub fn midi_section(&mut self, ui: &mut Ui) {
        let mut midi = self.settings.midi.clone();
        let restart = remote_ui::midi_section(ui, &mut midi, &mut self.midi_ports, &self.inputs);
        if midi != self.settings.midi {
            self.settings.midi = midi;
            self.save();
            self.inputs.restart_midi(&self.settings.midi);
        } else if restart {
            self.inputs.restart_midi(&self.settings.midi);
        }
    }

    pub fn console_section(&mut self, ui: &mut Ui) {
        match remote_ui::console_section(ui, &self.settings.console, &mut self.console_draft, &self.inputs) {
            Some(Apply::Settings(s)) => {
                self.settings.console = s;
                self.save();
                self.inputs.restart_console(&self.settings.console);
            }
            Some(Apply::Restart) => self.inputs.restart_console(&self.settings.console),
            None => {}
        }
    }

    pub fn m32_section(&mut self, ui: &mut Ui) {
        match remote_ui::m32_section(ui, &self.settings.m32, &mut self.m32_draft, &self.inputs) {
            Some(Apply::Settings(s)) => {
                self.settings.m32 = s;
                self.save();
                self.inputs.restart_m32(&self.settings.m32);
            }
            Some(Apply::Restart) => self.inputs.restart_m32(&self.settings.m32),
            None => {}
        }
    }

    /// Tableau des correspondances, modifié en direct.
    pub fn mappings(&mut self, ui: &mut Ui) {
        remote_ui::mappings_table(ui, self);
    }
}

impl MappingRows for Remotes {
    type Mapping = Mapping;

    fn mappings(&self) -> &[Mapping] {
        &self.settings.mappings
    }

    fn trigger_mut(m: &mut Mapping) -> &mut Trigger {
        &mut m.trigger
    }

    fn inputs(&mut self) -> &mut Inputs {
        &mut self.inputs
    }

    fn action_columns(&self) -> usize {
        1
    }

    fn action_cells(&mut self, ui: &mut Ui, i: usize, m: &mut Mapping) {
        egui::ComboBox::from_id_salt(("map-action", i)).selected_text(m.action.label()).width(240.0).show_ui(ui, |ui| {
            for a in ActionKind::ALL {
                ui.selectable_value(&mut m.action, a, a.label());
            }
        });
    }

    fn update(&mut self, i: usize, m: Mapping) {
        self.settings.mappings[i] = m;
        self.save();
    }

    fn remove(&mut self, i: usize) {
        self.settings.mappings.remove(i);
        self.save();
    }

    /// Reprend l'action de la dernière ligne.
    fn next(&self) -> Mapping {
        self.settings.mappings.last().map(|m| Mapping { trigger: Trigger::Unset, action: m.action }).unwrap_or_default()
    }

    fn add(&mut self, m: Mapping) {
        self.settings.mappings.push(m);
        self.save();
    }
}

/// Raccourcis clavier de la régie.
pub fn shortcuts(ui: &mut Ui) {
    ui.heading("Raccourcis clavier");
    let rows: &[(&str, &str)] = &[
        ("Espace", "GO de l'onglet en cours (Lecteur, Cuelist) ; lancer / pause du Diaporama"),
        ("P", "Cuelist : pause / reprise"),
        ("↑ / ↓", "Lecteur, Cuelist : élément ou cue précédent / suivant en attente"),
        ("← / →", "Diaporama : média précédent / suivant"),
        ("Échap", "Noir : coupe la sortie"),
        ("Ctrl+S", "Enregistrer le projet"),
        ("Ctrl+Maj+S", "Enregistrer sous…"),
        ("Ctrl+O", "Ouvrir un projet"),
        ("Ctrl+N", "Nouveau projet"),
        ("Ctrl+B / I / U", "Texte : gras / italique / souligné"),
    ];
    egui::Grid::new("shortcuts").num_columns(2).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
        for (k, d) in rows {
            ui.label(RichText::new(*k).monospace().strong());
            ui.label(*d);
            ui.end_row();
        }
    });
}
