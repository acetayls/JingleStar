//! Télécommandes, comme dans JingleStar : entrée MIDI, consoles Yamaha et Midas M32 / X32,
//! correspondances « déclencheur → action » avec le mode apprendre, raccourcis clavier.
//! Les actions passent par les mêmes commandes que l'OSC.

use star_remote::console::ConsoleClient;
use star_remote::m32::M32Client;
use star_remote::midi::MidiListener;
use crate::osc::Msg;
use crate::remote::{ActionKind, ConsoleSettings, Flag, InputEvent, M32Settings, Mapping, RemoteSettings, Trigger};
use egui::{RichText, Ui};
use star_ui::theme::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sources des télécommandes et correspondances, en marche dans la régie.
pub struct Remotes {
    pub settings: RemoteSettings,
    midi: Option<MidiListener>,
    midi_error: Option<String>,
    midi_retry: Instant,
    console: Option<ConsoleClient>,
    m32: Option<M32Client>,
    /// Correspondance en attente d'apprentissage.
    pub learn: Option<usize>,
    pub last_input: Option<String>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    // Saisie en cours dans la fenêtre.
    midi_ports: Vec<String>,
    console_draft: Option<ConsoleSettings>,
    m32_draft: Option<M32Settings>,
}

impl Remotes {
    pub fn new(repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let mut r = Self {
            settings: RemoteSettings::load(),
            midi: None,
            midi_error: None,
            midi_retry: Instant::now(),
            console: None,
            m32: None,
            learn: None,
            last_input: None,
            repaint,
            midi_ports: Vec::new(),
            console_draft: None,
            m32_draft: None,
        };
        r.restart_midi();
        r.restart_console();
        r.restart_m32();
        r
    }

    fn restart_midi(&mut self) {
        self.midi = None;
        self.midi_error = None;
        self.midi_retry = Instant::now();
        let s = self.settings.midi.clone();
        if s.enabled {
            match MidiListener::open(crate::project::APP, s.port.as_deref(), self.repaint.clone()) {
                Ok(l) => self.midi = Some(l),
                Err(e) => self.midi_error = Some(e),
            }
        }
    }

    fn restart_console(&mut self) {
        self.console = None;
        let s = self.settings.console.clone();
        if s.enabled {
            self.console = Some(ConsoleClient::start(s, self.repaint.clone()));
        }
    }

    fn restart_m32(&mut self) {
        self.m32 = None;
        let s = self.settings.m32.clone();
        if s.enabled {
            self.m32 = Some(M32Client::start(s, self.repaint.clone()));
        }
    }

    fn save(&self) {
        self.settings.save();
    }

    /// Messages reçus depuis la dernière image, traduits en commandes. `flag` : état d'une
    /// option du Lecteur (pour les touches qui la basculent).
    pub fn poll(&mut self, flag: impl Fn(Flag) -> bool) -> Vec<Msg> {
        let mut events: Vec<(&str, InputEvent)> = Vec::new();
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
        if self.settings.midi.enabled && self.midi_retry.elapsed() > Duration::from_secs(3) {
            self.midi_retry = Instant::now();
            let ports = star_remote::midi::port_names(crate::project::APP);
            match self.midi.as_ref().map(|l| l.port.clone()) {
                Some(port) if !ports.contains(&port) => {
                    self.midi = None;
                    self.midi_error = Some(format!("port MIDI débranché : {port}"));
                }
                None if !ports.is_empty() => self.restart_midi(),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for (source, ev) in events {
            let labelled = matches!(ev, InputEvent::Console { .. } | InputEvent::Scene { .. });
            self.last_input = Some(if labelled { ev.label() } else { format!("{source} · {}", ev.label()) });
            if let Some(i) = self.learn {
                if ev.learnable() {
                    if let Some(m) = self.settings.mappings.get_mut(i) {
                        m.trigger = ev.trigger();
                    }
                    self.learn = None;
                    self.save();
                }
                continue;
            }
            for m in self.settings.mappings.iter().filter(|m| m.matches(&ev)) {
                if let Some(c) = m.command(&ev, &flag) {
                    out.push(c);
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

pub const TABS: [(Tab, &str); 6] = [
    (Tab::Osc, "OSC"),
    (Tab::Midi, "MIDI"),
    (Tab::Yamaha, "Yamaha"),
    (Tab::M32, "Midas M32 / X32"),
    (Tab::Mappings, "Correspondances"),
    (Tab::Keyboard, "Raccourcis"),
];

impl Remotes {
    pub fn refresh_ports(&mut self) {
        self.midi_ports = star_remote::midi::port_names(crate::project::APP);
    }

    pub fn midi_section(&mut self, ui: &mut Ui) {
        ui.heading("MIDI");
        let mut midi = self.settings.midi.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut midi.enabled, "Activer l'entrée MIDI");
            ui.add_enabled_ui(midi.enabled, |ui| {
                let first = "Premier port disponible";
                let label = midi.port.clone().unwrap_or_else(|| first.into());
                egui::ComboBox::from_id_salt("midi-port").selected_text(label).width(240.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut midi.port, None, first);
                    for p in &self.midi_ports {
                        ui.selectable_value(&mut midi.port, Some(p.clone()), p);
                    }
                });
                if ui.small_button("⟳").on_hover_text("Rafraîchir la liste et reconnecter").clicked() {
                    self.midi_ports = star_remote::midi::port_names(crate::project::APP);
                    self.restart_midi();
                }
            });
        });
        if midi != self.settings.midi {
            self.settings.midi = midi;
            self.save();
            self.restart_midi();
        }
        if self.settings.midi.enabled {
            match (&self.midi, &self.midi_error) {
                (Some(l), _) => {
                    ui.label(RichText::new(format!("Connecté à {}", l.port)).small().color(GO));
                }
                (None, Some(e)) => {
                    ui.label(RichText::new(e).small().color(STOP));
                }
                (None, None) => {}
            }
        }
    }

    pub fn console_section(&mut self, ui: &mut Ui) {
        ui.heading("Console Yamaha (réseau)");
        let current = self.settings.console.clone();
        let draft = self.console_draft.get_or_insert_with(|| current.clone());
        let mut enabled = current.enabled;
        let mut apply = false;
        ui.horizontal(|ui| {
            apply |= ui.checkbox(&mut enabled, "Écouter la console").changed();
            ui.label("IP");
            let r = ui.add(egui::TextEdit::singleline(&mut draft.host).desired_width(130.0));
            apply |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.label("port");
            ui.add(egui::DragValue::new(&mut draft.port).range(1..=65535));
            let changed = draft.host != current.host || draft.port != current.port;
            if ui.add_enabled(changed || self.console.is_some(), egui::Button::new("Connecter")).clicked() {
                apply = true;
                enabled = true;
            }
        });
        if apply {
            draft.enabled = enabled;
            self.settings.console = draft.clone();
            self.save();
            self.restart_console();
        }
        ui.label(
            RichText::new(
                "QL, CL, TF, DM7, Rivage : port 49280. CueStar écoute les changements \
                 (ON de canal, fader, rappel de scène) et n'envoie aucune donnée à la console.",
            )
            .small()
            .color(TEXT_DIM),
        );
        if let Some(c) = self.console.as_ref() {
            let st = c.status();
            if st.connected {
                let model = st.model.unwrap_or_else(|| "console".into());
                ui.label(RichText::new(format!("Connecté à {model} ({})", current.host)).small().color(GO));
            } else if let Some(e) = st.error {
                ui.label(RichText::new(format!("{e} · nouvel essai en cours")).small().color(STOP));
            } else {
                ui.label(RichText::new("Connexion…").small().color(TEXT_DIM));
            }
            if let Some(line) = st.last_line {
                let ago = st.last_rx.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                ui.label(RichText::new(format!("Reçu (il y a {ago} s) : {line}")).small().monospace().color(TEXT_DIM));
            }
        }
    }

    pub fn m32_section(&mut self, ui: &mut Ui) {
        ui.heading("Console Midas M32 / Behringer X32 (réseau)");
        let current = self.settings.m32.clone();
        let draft = self.m32_draft.get_or_insert_with(|| current.clone());
        let mut enabled = current.enabled;
        let mut apply = false;
        ui.horizontal(|ui| {
            apply |= ui.checkbox(&mut enabled, "Écouter la console").changed();
            ui.label("IP");
            let r = ui.add(egui::TextEdit::singleline(&mut draft.host).desired_width(130.0));
            apply |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.label("port");
            ui.add(egui::DragValue::new(&mut draft.port).range(1..=65535));
            let changed = draft.host != current.host || draft.port != current.port;
            if ui.add_enabled(changed || self.m32.is_some(), egui::Button::new("Connecter")).clicked() {
                apply = true;
                enabled = true;
            }
        });
        if apply {
            draft.enabled = enabled;
            self.settings.m32 = draft.clone();
            self.save();
            self.restart_m32();
        }
        ui.label(
            RichText::new(
                "Port 10023. La M32 n'envoie ses changements qu'aux clients abonnés : CueStar envoie \
                 « /xremote » toutes les 8 s et « /info », deux requêtes sans argument qui ne modifient rien. \
                 Aucune autre donnée n'est envoyée.",
            )
            .small()
            .color(TEXT_DIM),
        );
        if let Some(c) = self.m32.as_ref() {
            let st = c.status();
            if st.connected {
                let model = st.model.unwrap_or_else(|| "M32".into());
                ui.label(RichText::new(format!("Connecté à {model} ({})", current.host)).small().color(GO));
            } else if let Some(e) = st.error {
                ui.label(RichText::new(e).small().color(STOP));
            } else {
                ui.label(RichText::new("En attente de réponse de la console…").small().color(TEXT_DIM));
            }
            if let Some(line) = st.last_line {
                let ago = st.last_rx.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                ui.label(RichText::new(format!("Reçu (il y a {ago} s) : {line}")).small().monospace().color(TEXT_DIM));
            }
        }
    }

    /// Tableau des correspondances, modifié en direct.
    pub fn mappings(&mut self, ui: &mut Ui) {
        ui.heading("Correspondances");
        ui.label(RichText::new("Cliquez sur « Apprendre » puis appuyez sur la touche ou bougez le contrôle à affecter.").small().color(TEXT_DIM));
        let last = self.last_input.clone().unwrap_or_else(|| "—".into());
        ui.label(RichText::new(format!("Dernier message reçu : {last}")).small().color(TEXT_DIM));
        ui.add_space(4.0);
        let mut remove = None;
        let mut changed = false;
        let count = self.settings.mappings.len();
        egui::Grid::new("mappings").num_columns(3).spacing([8.0, 6.0]).striped(true).show(ui, |ui| {
            for i in 0..count {
                let learning = self.learn == Some(i);
                let m = &mut self.settings.mappings[i];
                let before = m.clone();
                ui.horizontal(|ui| {
                    let text = if learning { "En attente…" } else { "Apprendre" };
                    if ui.selectable_label(learning, text).clicked() {
                        self.learn = if learning { None } else { Some(i) };
                    }
                    ui.label(m.trigger.label());
                    ui.menu_button("✏", |ui| trigger_editor(ui, &mut m.trigger)).response.on_hover_text("Modifier la valeur apprise");
                });
                egui::ComboBox::from_id_salt(("map-action", i)).selected_text(m.action.label()).width(240.0).show_ui(ui, |ui| {
                    for a in ActionKind::ALL {
                        ui.selectable_value(&mut m.action, a, a.label());
                    }
                });
                if ui.small_button("🗑").on_hover_text("Supprimer").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
                changed |= *m != before;
            }
        });
        if let Some(i) = remove {
            self.settings.mappings.remove(i);
            self.learn = match self.learn {
                Some(l) if l == i => None,
                Some(l) if l > i => Some(l - 1),
                other => other,
            };
            changed = true;
        }
        if count == 0 {
            ui.label(RichText::new("Aucune correspondance.").color(TEXT_DIM));
        }
        ui.add_space(4.0);
        if ui.button("+ Ajouter et apprendre").clicked() {
            // Reprend l'action de la dernière ligne.
            let next = self.settings.mappings.last().map(|m| Mapping { trigger: Trigger::Unset, action: m.action });
            self.settings.mappings.push(next.unwrap_or_default());
            self.learn = Some(self.settings.mappings.len() - 1);
            changed = true;
        }
        if changed {
            self.save();
        }
    }
}

/// Édition à la main d'un déclencheur. Canaux, programmes et voies sont affichés à partir de 1.
fn trigger_editor(ui: &mut Ui, t: &mut Trigger) {
    ui.set_min_width(220.0);
    let kinds = [
        ("Note MIDI", Trigger::Note { channel: 0, note: 60 }),
        ("Control Change (CC)", Trigger::Cc { channel: 0, cc: 1 }),
        ("Program Change", Trigger::Program { channel: 0, program: 0 }),
        ("Touche de console", Trigger::Console { address: "MIXER:Current/InCh/Fader/On".into(), x: 0, y: 0 }),
        ("Scène de console", Trigger::Scene { number: 1 }),
        ("Paramètre M32", Trigger::Console { address: "/ch/01/mix/on".into(), x: 0, y: 0 }),
    ];
    let current = std::mem::discriminant(t);
    ui.horizontal(|ui| {
        ui.label("Type");
        let name = kinds.iter().find(|(_, k)| std::mem::discriminant(k) == current).map_or("(non appris)", |(n, _)| n);
        egui::ComboBox::from_id_salt("trigger-kind").selected_text(name).show_ui(ui, |ui| {
            for (n, k) in &kinds {
                if ui.selectable_label(std::mem::discriminant(k) == current, *n).clicked() && std::mem::discriminant(k) != current {
                    *t = k.clone();
                }
            }
        });
    });
    let one_based = |ui: &mut Ui, label: &str, v: &mut u8, max: u8| {
        ui.horizontal(|ui| {
            ui.label(label);
            let mut n = *v as u32 + 1;
            ui.add(egui::DragValue::new(&mut n).range(1..=max as u32 + 1));
            *v = (n - 1) as u8;
        });
    };
    let raw = |ui: &mut Ui, label: &str, v: &mut u8| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::DragValue::new(v).range(0..=127));
        });
    };
    match t {
        Trigger::Unset => {
            ui.label(RichText::new("Choisissez un type ou utilisez « Apprendre ».").small().color(TEXT_DIM));
        }
        Trigger::Note { channel, note } => {
            one_based(ui, "Canal", channel, 15);
            raw(ui, "Note", note);
        }
        Trigger::Cc { channel, cc } => {
            one_based(ui, "Canal", channel, 15);
            raw(ui, "CC", cc);
        }
        Trigger::Program { channel, program } => {
            one_based(ui, "Canal", channel, 15);
            one_based(ui, "Program", program, 127);
        }
        Trigger::Console { address, x, y } => {
            ui.horizontal(|ui| {
                ui.label("Adresse");
                ui.add(egui::TextEdit::singleline(address).desired_width(260.0));
            });
            if !address.starts_with('/') {
                ui.horizontal(|ui| {
                    ui.label("Voie");
                    let mut n = *x + 1;
                    ui.add(egui::DragValue::new(&mut n).range(1..=512));
                    *x = n - 1;
                    ui.label("Index");
                    let mut n = *y + 1;
                    ui.add(egui::DragValue::new(&mut n).range(1..=64));
                    *y = n - 1;
                });
            }
        }
        Trigger::Scene { number } => {
            ui.horizontal(|ui| {
                ui.label("Scène");
                ui.add(egui::DragValue::new(number).range(0..=999));
            });
        }
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
