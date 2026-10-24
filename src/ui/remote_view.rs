//! Fenêtre des télécommandes : entrée MIDI et correspondances vers les actions.

use egui::{RichText, Ui};

use super::theme::*;
use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use crate::controller::Controller;
use crate::i18n::tr;
use crate::remote::{ActionKind, Mapping};

#[derive(Default)]
pub struct RemoteWindow {
    pub open: bool,
    midi_ports: Vec<String>,
}

impl RemoteWindow {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.midi_ports = crate::midi::port_names();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, ctrl: &mut Controller) {
        if !self.open {
            ctrl.learn = None;
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Télécommandes", "Remote control"))
            .id(egui::Id::unique("remote-window"))
            .open(&mut open)
            .default_width(620.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                self.midi_section(ui, ctrl);
                ui.separator();
                mappings(ui, ctrl);
            });
        if !open {
            self.open = false;
            ctrl.learn = None;
        }
    }

    fn midi_section(&mut self, ui: &mut Ui, ctrl: &mut Controller) {
        ui.heading("MIDI");
        let mut midi = ctrl.project.preferences.midi.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut midi.enabled, tr("Activer l'entrée MIDI", "Enable MIDI input"));
            ui.add_enabled_ui(midi.enabled, |ui| {
                let first = tr("Premier port disponible", "First available port");
                let label = midi.port.clone().unwrap_or_else(|| first.into());
                egui::ComboBox::from_id_salt("midi-port").selected_text(label).width(240.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut midi.port, None, first);
                    for p in &self.midi_ports {
                        ui.selectable_value(&mut midi.port, Some(p.clone()), p);
                    }
                });
                if ui.small_button("⟳").on_hover_text(tr("Rafraîchir la liste et reconnecter", "Refresh list and reconnect")).clicked() {
                    self.midi_ports = crate::midi::port_names();
                    ctrl.restart_midi();
                }
            });
        });
        ctrl.set_midi_settings(midi);
        if ctrl.project.preferences.midi.enabled {
            match (&ctrl.midi, &ctrl.midi_error) {
                (Some(l), _) => {
                    ui.label(RichText::new(format!("{} {}", tr("Connecté à", "Connected to"), l.port)).small().color(GO));
                }
                (None, Some(e)) => {
                    ui.label(RichText::new(e).small().color(STOP));
                }
                (None, None) => {}
            }
        }
    }
}

/// Tableau des correspondances, modifié en direct.
fn mappings(ui: &mut Ui, ctrl: &mut Controller) {
    ui.heading(tr("Correspondances", "Mappings"));
    ui.label(RichText::new(tr(
        "Cliquez sur « Apprendre » puis appuyez sur la touche ou bougez le contrôle à affecter.",
        "Click “Learn”, then press the key or move the control to assign.",
    )).small().color(TEXT_DIM));
    let last = ctrl.last_input.clone().unwrap_or_else(|| "—".into());
    ui.label(RichText::new(format!("{} : {last}", tr("Dernier message reçu", "Last message received"))).small().color(TEXT_DIM));
    ui.add_space(4.0);

    let mut remove = None;
    let count = ctrl.project.preferences.mappings.len();
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("mappings").num_columns(5).spacing([8.0, 6.0]).striped(true).show(ui, |ui| {
            for i in 0..count {
                let mut m = ctrl.project.preferences.mappings[i].clone();
                let learning = ctrl.learn == Some(i);
                ui.horizontal(|ui| {
                    let text = if learning { tr("En attente…", "Waiting…") } else { tr("Apprendre", "Learn") };
                    if ui.selectable_label(learning, text).clicked() {
                        ctrl.learn = if learning { None } else { Some(i) };
                    }
                    ui.label(m.trigger.label());
                });
                egui::ComboBox::from_id_salt(("map-action", i)).selected_text(m.action.label()).width(220.0).show_ui(ui, |ui| {
                    for a in ActionKind::ALL {
                        ui.selectable_value(&mut m.action, a, a.label());
                    }
                });
                if m.action.needs_module() {
                    ui.horizontal(|ui| {
                        ui.label(tr("Module", "Module"));
                        let mut n = m.module + 1;
                        ui.add(egui::DragValue::new(&mut n).range(1..=MAX_MODULES));
                        m.module = n - 1;
                    });
                } else {
                    ui.label("");
                }
                if m.action.needs_jingle() {
                    ui.horizontal(|ui| {
                        ui.label("Jingle");
                        let mut n = m.jingle + 1;
                        ui.add(egui::DragValue::new(&mut n).range(1..=JINGLE_SLOTS));
                        m.jingle = n - 1;
                        let name = ctrl.project.modules.get(m.module).and_then(|md| md.jingles[m.jingle].as_ref()).map(|j| j.name.clone());
                        if let Some(name) = name {
                            ui.label(RichText::new(name).small().color(TEXT_DIM));
                        }
                    });
                } else {
                    ui.label("");
                }
                if ui.small_button("🗑").on_hover_text(tr("Supprimer", "Delete")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
                ctrl.update_mapping(i, m);
            }
        });
    });
    if let Some(i) = remove {
        ctrl.remove_mapping(i);
    }
    if count == 0 {
        ui.label(RichText::new(tr("Aucune correspondance.", "No mapping yet.")).color(TEXT_DIM));
    }
    ui.add_space(4.0);
    if ui.button(tr("＋ Ajouter et apprendre", "＋ Add and learn")).clicked() {
        // Reprend l'action de la dernière ligne, au jingle suivant : pratique pour un pad.
        let next = ctrl.project.preferences.mappings.last().map(|m| Mapping {
            trigger: Default::default(),
            jingle: if m.action.needs_jingle() { (m.jingle + 1) % JINGLE_SLOTS } else { m.jingle },
            ..m.clone()
        });
        ctrl.add_mapping(next.unwrap_or_default());
        ctrl.learn = Some(ctrl.project.preferences.mappings.len() - 1);
    }
}
