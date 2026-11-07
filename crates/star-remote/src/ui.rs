//! Fenêtre des télécommandes, parties communes (option `ui`) : réglages MIDI, consoles Yamaha
//! et Midas, éditeur de déclencheur et table des assignations avec le mode « apprendre ».
//! Repris de JingleStar ; chaque appli garde ses onglets OSC et Raccourcis, et décrit ses
//! actions dans la table par [`MappingRows`].

use egui::{RichText, Ui};
use star_core::i18n::tr;
use star_ui::theme::*;

use crate::input::Trigger;
use crate::inputs::Inputs;
use crate::settings::{ConsoleSettings, M32Settings, MidiSettings};

/// Ce que demande une section de console.
#[derive(Debug, Clone, PartialEq)]
pub enum Apply<S> {
    /// Nouveaux réglages à enregistrer, puis relancer l'écoute.
    Settings(S),
    /// Mêmes réglages : relancer l'écoute.
    Restart,
}

/// Réglages MIDI : `midi` est modifié sur place (à comparer avec l'ancien). Renvoie vrai si
/// l'on a demandé de rafraîchir la liste des ports et de reconnecter.
pub fn midi_section(ui: &mut Ui, midi: &mut MidiSettings, ports: &mut Vec<String>, inputs: &Inputs) -> bool {
    ui.heading("MIDI");
    let mut restart = false;
    ui.horizontal(|ui| {
        ui.checkbox(&mut midi.enabled, tr("Activer l'entrée MIDI", "Enable MIDI input"));
        ui.add_enabled_ui(midi.enabled, |ui| {
            let first = tr("Premier port disponible", "First available port");
            let label = midi.port.clone().unwrap_or_else(|| first.into());
            egui::ComboBox::from_id_salt("midi-port").selected_text(label).width(240.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut midi.port, None, first);
                for p in ports.iter() {
                    ui.selectable_value(&mut midi.port, Some(p.clone()), p);
                }
            });
            if ui.small_button("⟳").on_hover_text(tr("Rafraîchir la liste et reconnecter", "Refresh list and reconnect")).clicked() {
                *ports = crate::midi::port_names(inputs.app());
                restart = true;
            }
        });
    });
    if midi.enabled {
        match (&inputs.midi, &inputs.midi_error) {
            (Some(l), _) => {
                ui.label(RichText::new(format!("{} {}", tr("Connecté à", "Connected to"), l.port)).small().color(GO));
            }
            (None, Some(e)) => {
                ui.label(RichText::new(e).small().color(STOP));
            }
            (None, None) => {}
        }
    }
    restart
}

/// Réglages de la console Yamaha. `draft` : adresse en cours de saisie, appliquée avec
/// « Connecter ».
pub fn console_section(ui: &mut Ui, current: &ConsoleSettings, draft: &mut Option<ConsoleSettings>, inputs: &Inputs) -> Option<Apply<ConsoleSettings>> {
    ui.heading(tr("Console Yamaha (réseau)", "Yamaha console (network)"));
    let draft = draft.get_or_insert_with(|| current.clone());
    let mut enabled = current.enabled;
    let mut apply = false;
    ui.horizontal(|ui| {
        if ui.checkbox(&mut enabled, tr("Écouter la console", "Listen to the console")).changed() {
            apply = true;
        }
        ui.label(tr("IP", "IP"));
        let r = ui.add(egui::TextEdit::singleline(&mut draft.host).desired_width(130.0));
        apply |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.label(tr("port", "port"));
        ui.add(egui::DragValue::new(&mut draft.port).range(1..=65535));
        let changed = draft.host != current.host || draft.port != current.port;
        if ui.add_enabled(changed || inputs.console.is_some(), egui::Button::new(tr("Connecter", "Connect"))).clicked() {
            apply = true;
            enabled = true;
        }
    });
    let mut out = None;
    if apply {
        draft.enabled = enabled;
        out = Some(if *draft == *current { Apply::Restart } else { Apply::Settings(draft.clone()) });
    }
    let app = inputs.app().name;
    ui.label(
        RichText::new(
            tr(
                "QL, CL, TF, DM7, Rivage : port 49280. {app} écoute les changements \
                 (ON de canal, fader, rappel de scène) et n'envoie aucune donnée à la console.",
                "QL, CL, TF, DM7, Rivage: port 49280. {app} listens to changes \
                 (channel ON, fader, scene recall) and sends no data to the console.",
            )
            .replace("{app}", app),
        )
        .small()
        .color(TEXT_DIM),
    );
    if let Some(c) = inputs.console.as_ref() {
        let st = c.status();
        if st.connected {
            let model = st.model.unwrap_or_else(|| tr("console", "console").into());
            ui.label(RichText::new(format!("{} {model} ({})", tr("Connecté à", "Connected to"), current.host)).small().color(GO));
        } else if let Some(e) = st.error {
            ui.label(RichText::new(format!("{e} · {}", tr("nouvel essai en cours", "retrying"))).small().color(STOP));
        } else {
            ui.label(RichText::new(tr("Connexion…", "Connecting…")).small().color(TEXT_DIM));
        }
        if let Some(line) = st.last_line {
            let ago = st.last_rx.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            ui.label(RichText::new(format!("{} ({} {ago} s) : {line}", tr("Reçu", "Received"), tr("il y a", "ago"))).small().monospace().color(TEXT_DIM));
        }
    }
    out
}

/// Réglages de la console Midas M32 / Behringer X32.
pub fn m32_section(ui: &mut Ui, current: &M32Settings, draft: &mut Option<M32Settings>, inputs: &Inputs) -> Option<Apply<M32Settings>> {
    ui.heading(tr("Console Midas M32 / Behringer X32 (réseau)", "Midas M32 / Behringer X32 console (network)"));
    let draft = draft.get_or_insert_with(|| current.clone());
    let mut enabled = current.enabled;
    let mut apply = false;
    ui.horizontal(|ui| {
        apply |= ui.checkbox(&mut enabled, tr("Écouter la console", "Listen to the console")).changed();
        ui.label("IP");
        let r = ui.add(egui::TextEdit::singleline(&mut draft.host).desired_width(130.0));
        apply |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.label(tr("port", "port"));
        ui.add(egui::DragValue::new(&mut draft.port).range(1..=65535));
        let changed = draft.host != current.host || draft.port != current.port;
        if ui.add_enabled(changed || inputs.m32.is_some(), egui::Button::new(tr("Connecter", "Connect"))).clicked() {
            apply = true;
            enabled = true;
        }
    });
    let mut out = None;
    if apply {
        draft.enabled = enabled;
        out = Some(if *draft == *current { Apply::Restart } else { Apply::Settings(draft.clone()) });
    }
    let app = inputs.app().name;
    ui.label(
        RichText::new(
            tr(
                "Port 10023. La M32 n'envoie ses changements qu'aux clients abonnés : {app} envoie \
                 « /xremote » toutes les 8 s et « /info », deux requêtes sans argument qui ne modifient rien. \
                 Aucune autre donnée n'est envoyée.",
                "Port 10023. The M32 only sends its changes to subscribed clients: {app} sends \
                 “/xremote” every 8 s and “/info”, two requests with no argument that change nothing. \
                 No other data is sent.",
            )
            .replace("{app}", app),
        )
        .small()
        .color(TEXT_DIM),
    );
    if let Some(c) = inputs.m32.as_ref() {
        let st = c.status();
        if st.connected {
            let model = st.model.unwrap_or_else(|| "M32".into());
            ui.label(RichText::new(format!("{} {model} ({})", tr("Connecté à", "Connected to"), current.host)).small().color(GO));
        } else if let Some(e) = st.error {
            ui.label(RichText::new(e).small().color(STOP));
        } else {
            ui.label(RichText::new(tr("En attente de réponse de la console…", "Waiting for the console to answer…")).small().color(TEXT_DIM));
        }
        if let Some(line) = st.last_line {
            let ago = st.last_rx.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            ui.label(RichText::new(format!("{} ({} {ago} s) : {line}", tr("Reçu", "Received"), tr("il y a", "ago"))).small().monospace().color(TEXT_DIM));
        }
    }
    out
}

/// Ce que la table des assignations demande à l'appli.
pub trait MappingRows {
    type Mapping: Clone + PartialEq;
    /// Les assignations, dans l'ordre.
    fn mappings(&self) -> &[Self::Mapping];
    fn trigger_mut(m: &mut Self::Mapping) -> &mut Trigger;
    fn inputs(&mut self) -> &mut Inputs;
    /// Nombre de colonnes dessinées par [`MappingRows::action_cells`].
    fn action_columns(&self) -> usize;
    /// Cellules de l'action de l'assignation `i` (exactement `action_columns`).
    fn action_cells(&mut self, ui: &mut Ui, i: usize, m: &mut Self::Mapping);
    /// Ajout éventuel après le déclencheur (reprise en douceur d'un fader…).
    fn trigger_note(&mut self, _ui: &mut Ui, _i: usize) {}
    fn update(&mut self, i: usize, m: Self::Mapping);
    fn remove(&mut self, i: usize);
    /// Nouvelle assignation, souvent reprise de la dernière.
    fn next(&self) -> Self::Mapping;
    fn add(&mut self, m: Self::Mapping);
}

/// Table des assignations : déclencheur appris ou saisi, action, suppression, ajout.
pub fn mappings_table<R: MappingRows>(ui: &mut Ui, rows: &mut R) {
    ui.heading(tr("Assignations", "Mappings"));
    ui.label(RichText::new(tr(
        "Cliquez sur « Apprendre » puis appuyez sur la touche ou bougez le contrôle à affecter.",
        "Click “Learn”, then press the key or move the control to assign.",
    )).small().color(TEXT_DIM));
    let last = rows.inputs().last_input.clone().unwrap_or_else(|| "—".into());
    ui.label(RichText::new(format!("{} : {last}", tr("Dernier message reçu", "Last message received"))).small().color(TEXT_DIM));
    ui.add_space(4.0);

    let mut remove = None;
    let count = rows.mappings().len();
    let columns = 2 + rows.action_columns();
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("mappings").num_columns(columns).spacing([8.0, 6.0]).striped(true).show(ui, |ui| {
            for i in 0..count {
                let mut m = rows.mappings()[i].clone();
                let learning = rows.inputs().learn == Some(i);
                ui.horizontal(|ui| {
                    let text = if learning { tr("En attente…", "Waiting…") } else { tr("Apprendre", "Learn") };
                    if ui.selectable_label(learning, text).clicked() {
                        rows.inputs().learn = if learning { None } else { Some(i) };
                    }
                    ui.label(R::trigger_mut(&mut m).label());
                    ui.menu_button("✏", |ui| trigger_editor(ui, R::trigger_mut(&mut m)))
                        .response
                        .on_hover_text(tr("Modifier la valeur apprise", "Edit the learned value"));
                    rows.trigger_note(ui, i);
                });
                rows.action_cells(ui, i, &mut m);
                if ui.small_button("🗑").on_hover_text(tr("Supprimer", "Delete")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
                if m != rows.mappings()[i] {
                    rows.update(i, m);
                }
            }
        });
    });
    if let Some(i) = remove {
        rows.remove(i);
        rows.inputs().mapping_removed(i);
    }
    if count == 0 {
        ui.label(RichText::new(tr("Aucune assignation.", "No mapping yet.")).color(TEXT_DIM));
    }
    ui.add_space(4.0);
    if ui.button(tr("＋ Ajouter et apprendre", "＋ Add and learn")).clicked() {
        let next = rows.next();
        rows.add(next);
        let n = rows.mappings().len();
        rows.inputs().learn = Some(n - 1);
    }
}

/// Édition à la main d'un déclencheur appris (ou saisi sans apprentissage).
/// Canaux, programmes et touches de console sont affichés à partir de 1, comme sur le matériel.
pub fn trigger_editor(ui: &mut Ui, t: &mut Trigger) {
    ui.set_min_width(220.0);
    let kinds = [
        (tr("Note MIDI", "MIDI note"), Trigger::Note { channel: 0, note: 60 }),
        ("Control Change (CC)", Trigger::Cc { channel: 0, cc: 1 }),
        ("Program Change", Trigger::Program { channel: 0, program: 0 }),
        (tr("Touche de console", "Console key"), Trigger::Console { address: "MIXER:Current/InCh/Fader/On".into(), x: 0, y: 0 }),
        (tr("Scène de console", "Console scene"), Trigger::Scene { number: 1 }),
        (tr("Paramètre M32", "M32 parameter"), Trigger::Console { address: "/ch/01/mix/on".into(), x: 0, y: 0 }),
    ];
    let current = std::mem::discriminant(t);
    ui.horizontal(|ui| {
        ui.label(tr("Type", "Type"));
        let name = kinds.iter().find(|(_, k)| std::mem::discriminant(k) == current).map_or(tr("(non appris)", "(not learned)"), |(n, _)| n);
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
    let channel = tr("Canal", "Channel");
    match t {
        Trigger::Unset => {
            ui.label(RichText::new(tr("Choisissez un type ou utilisez « Apprendre ».", "Pick a type or use “Learn”.")).small().color(TEXT_DIM));
        }
        Trigger::Note { channel: c, note } => {
            one_based(ui, channel, c, 15);
            raw(ui, "Note", note);
        }
        Trigger::Cc { channel: c, cc } => {
            one_based(ui, channel, c, 15);
            raw(ui, "CC", cc);
        }
        Trigger::Program { channel: c, program } => {
            one_based(ui, channel, c, 15);
            one_based(ui, "Program", program, 127);
        }
        Trigger::Console { address, x, y } => {
            ui.horizontal(|ui| {
                ui.label(tr("Adresse", "Address"));
                ui.add(egui::TextEdit::singleline(address).desired_width(260.0));
            });
            // Les adresses OSC de la M32 contiennent déjà la voie.
            if !address.starts_with('/') {
                ui.horizontal(|ui| {
                    ui.label(tr("Voie", "Channel"));
                    let mut n = *x + 1;
                    ui.add(egui::DragValue::new(&mut n).range(1..=512));
                    *x = n - 1;
                    ui.label(tr("Index", "Index"));
                    let mut n = *y + 1;
                    ui.add(egui::DragValue::new(&mut n).range(1..=64));
                    *y = n - 1;
                });
            }
        }
        Trigger::Scene { number } => {
            ui.horizontal(|ui| {
                ui.label(tr("Scène", "Scene"));
                ui.add(egui::DragValue::new(number).range(0..=999));
            });
        }
    }
}
