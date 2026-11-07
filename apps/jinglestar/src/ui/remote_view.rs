//! Fenêtre des télécommandes : OSC, entrée MIDI, consoles Yamaha et Midas, et assignations vers les actions.

use egui::{RichText, Ui};

use super::theme::*;
use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use crate::controller::Controller;
use star_core::i18n::tr;
use crate::project::{ConsoleSettings, M32Settings, OscSettings};
use crate::remote::{ActionKind, Mapping, Trigger};
use star_remote::ui::{Apply, MappingRows};
use star_remote::Inputs;

/// Onglets de la fenêtre : un par protocole, puis les assignations et les raccourcis clavier.
#[derive(Default, Clone, Copy, PartialEq)]
enum Tab {
    #[default]
    Osc,
    Midi,
    Yamaha,
    M32,
    Mappings,
    Keyboard,
}

#[derive(Default)]
pub struct RemoteWindow {
    pub open: bool,
    tab: Tab,
    midi_ports: Vec<String>,
    /// Adresse de la console en cours de saisie, appliquée avec « Connecter ».
    console: Option<ConsoleSettings>,
    m32: Option<M32Settings>,
    /// Réglages OSC en cours de saisie, appliqués avec « Appliquer ».
    osc: Option<OscSettings>,
}

impl RemoteWindow {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.midi_ports = star_remote::midi::port_names(crate::project::APP);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, ctrl: &mut Controller) {
        if !self.open {
            ctrl.inputs.learn = None;
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
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [
                        (Tab::Osc, "OSC"),
                        (Tab::Midi, "MIDI"),
                        (Tab::Yamaha, "Yamaha"),
                        (Tab::M32, "Midas M32 / X32"),
                        (Tab::Mappings, tr("Assignations", "Mappings")),
                        (Tab::Keyboard, tr("Raccourcis", "Shortcuts")),
                    ] {
                        ui.selectable_value(&mut self.tab, tab, label);
                    }
                });
                ui.separator();
                // « Apprendre » ne reste pas actif dans un onglet où on ne le voit pas.
                if self.tab != Tab::Mappings {
                    ctrl.inputs.learn = None;
                }
                let max_h = (ctx.content_rect().height() - 120.0).max(200.0);
                egui::ScrollArea::vertical().id_salt("remote-scroll").max_height(max_h).auto_shrink([false, true]).show(ui, |ui| match self.tab {
                    Tab::Osc => self.osc_section(ui, ctrl),
                    Tab::Midi => self.midi_section(ui, ctrl),
                    Tab::Yamaha => self.console_section(ui, ctrl),
                    Tab::M32 => self.m32_section(ui, ctrl),
                    Tab::Mappings => mappings(ui, ctrl),
                    Tab::Keyboard => {
                        ui.heading(tr("Raccourcis clavier", "Keyboard shortcuts"));
                        super::shortcuts_page(ui);
                        ui.add_space(8.0);
                        super::hotkeys::jingle_hotkeys_list(ui, ctrl);
                    }
                });
            });
        if !open {
            self.open = false;
            ctrl.inputs.learn = None;
        }
    }

    fn osc_section(&mut self, ui: &mut Ui, ctrl: &mut Controller) {
        ui.heading("OSC");
        let current = ctrl.project.preferences.osc.clone();
        let draft = self.osc.get_or_insert_with(|| current.clone());
        osc_settings(ui, draft, ctrl);
        let mut normalized = draft.clone();
        normalized.prefix = crate::osc::normalize_prefix(&normalized.prefix);
        if ui.add_enabled(normalized != current, egui::Button::new(tr("Appliquer", "Apply"))).clicked() {
            draft.prefix = normalized.prefix.clone();
            ctrl.set_osc_settings(normalized);
        }
    }

    fn midi_section(&mut self, ui: &mut Ui, ctrl: &mut Controller) {
        let mut midi = ctrl.project.preferences.midi.clone();
        if star_remote::ui::midi_section(ui, &mut midi, &mut self.midi_ports, &ctrl.inputs) {
            ctrl.restart_midi();
        }
        ctrl.set_midi_settings(midi);
    }

    fn console_section(&mut self, ui: &mut Ui, ctrl: &mut Controller) {
        let current = ctrl.project.preferences.console.clone();
        match star_remote::ui::console_section(ui, &current, &mut self.console, &ctrl.inputs) {
            Some(Apply::Settings(s)) => ctrl.set_console_settings(s),
            Some(Apply::Restart) => ctrl.restart_console(),
            None => {}
        }
    }

    fn m32_section(&mut self, ui: &mut Ui, ctrl: &mut Controller) {
        let current = ctrl.project.preferences.m32.clone();
        match star_remote::ui::m32_section(ui, &current, &mut self.m32, &ctrl.inputs) {
            Some(Apply::Settings(s)) => ctrl.set_m32_settings(s),
            Some(Apply::Restart) => ctrl.restart_m32(),
            None => {}
        }
    }
}

fn mappings(ui: &mut Ui, ctrl: &mut Controller) {
    star_remote::ui::mappings_table(ui, &mut Rows(ctrl));
}

/// Assignations de JingleStar dans la table commune : action, module et jingle.
struct Rows<'a>(&'a mut Controller);

impl MappingRows for Rows<'_> {
    type Mapping = Mapping;

    fn mappings(&self) -> &[Mapping] {
        &self.0.project.preferences.mappings
    }

    fn trigger_mut(m: &mut Mapping) -> &mut Trigger {
        &mut m.trigger
    }

    fn inputs(&mut self) -> &mut Inputs {
        &mut self.0.inputs
    }

    fn action_columns(&self) -> usize {
        3
    }

    fn trigger_note(&mut self, ui: &mut Ui, i: usize) {
        if let Some(v) = self.0.pickup_waiting(i) {
            ui.label(RichText::new(format!("{} {v:.1} dB", tr("amener le fader à", "bring the fader to"))).small().color(ACCENT)).on_hover_text(tr(
                "Reprise en douceur : le fader agit quand il atteint le volume actuel, pour éviter un saut.",
                "Soft takeover: the fader takes effect once it reaches the current volume, to avoid a jump.",
            ));
        }
    }

    fn action_cells(&mut self, ui: &mut Ui, i: usize, m: &mut Mapping) {
        let ctrl = &*self.0;
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
                // Dit tout de suite si l'assignation ne fera rien : module absent
                // ou dans un autre mode que celui de l'action.
                let p = &ctrl.project;
                let (txt, warn) = if m.module >= p.module_count {
                    (format!("{} ({} {})", tr("absent", "missing"), tr("le projet en a", "the project has"), p.module_count), true)
                } else {
                    let md = &p.modules[m.module];
                    match m.action.required_mode() {
                        Some(need) if md.mode != need => (
                            format!("{} : {} {}", md.name, tr("sans effet, module en mode", "no effect, module in"), super::mode_label(md.mode)),
                            true,
                        ),
                        _ => (md.name.clone(), false),
                    }
                };
                let txt = if warn { format!("⚠ {txt}") } else { txt };
                let resp = ui.label(RichText::new(txt).small().color(if warn { ACCENT } else { TEXT_DIM }));
                if warn {
                    resp.on_hover_text(tr(
                        "Cette assignation n'agit que sur un module présent et dans le mode de l'action (Jingles, Playlist ou Cuelist).",
                        "This mapping only acts on a module that exists and is in the action's mode (Jingles, Playlist or Cuelist).",
                    ));
                }
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
                match name {
                    Some(name) => ui.label(RichText::new(name).small().color(TEXT_DIM)),
                    None => ui.label(RichText::new(tr("⚠ emplacement vide", "⚠ empty slot")).small().color(ACCENT)),
                };
            });
        } else {
            ui.label("");
        }
    }

    fn update(&mut self, i: usize, m: Mapping) {
        self.0.update_mapping(i, m);
    }

    fn remove(&mut self, i: usize) {
        self.0.remove_mapping(i);
    }

    /// Reprend l'action de la dernière ligne, au jingle suivant : pratique pour un pad.
    fn next(&self) -> Mapping {
        self.mappings()
            .last()
            .map(|m| Mapping {
                trigger: Default::default(),
                jingle: if m.action.needs_jingle() { (m.jingle + 1) % JINGLE_SLOTS } else { m.jingle },
                ..m.clone()
            })
            .unwrap_or_default()
    }

    fn add(&mut self, m: Mapping) {
        self.0.add_mapping(m);
    }
}

fn osc_settings(ui: &mut Ui, osc: &mut OscSettings, ctrl: &Controller) {
    ui.checkbox(&mut osc.enabled, tr("Activer le pilotage OSC", "Enable OSC control"));
    ui.add_enabled_ui(osc.enabled, |ui| {
        egui::Grid::new("osc-settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(tr("Port UDP (réception)", "UDP port (receive)"));
            ui.add(egui::DragValue::new(&mut osc.port).range(1024..=65535));
            ui.end_row();

            ui.label(tr("Préfixe des adresses", "Address prefix"));
            ui.add(egui::TextEdit::singleline(&mut osc.prefix).desired_width(200.0).hint_text(tr("(aucun)", "(none)")));
            ui.end_row();

            ui.label("OSCQuery");
            ui.horizontal(|ui| {
                ui.checkbox(&mut osc.oscquery, tr("Serveur HTTP, port", "HTTP server, port"));
                ui.add_enabled(osc.oscquery, egui::DragValue::new(&mut osc.oscquery_port).range(1024..=65535));
            });
            ui.end_row();

            ui.label(tr("Découverte réseau", "Network discovery"));
            ui.add_enabled(osc.oscquery, egui::Checkbox::new(&mut osc.zeroconf, tr("Annoncer en mDNS (Bonjour)", "Advertise over mDNS (Bonjour)")));
            ui.end_row();

            ui.label(tr("Nom annoncé", "Advertised name"));
            ui.add(egui::TextEdit::singleline(&mut osc.name).desired_width(200.0));
            ui.end_row();
        });
    });
    // État du serveur en marche (réglages appliqués).
    match ctrl.osc.as_ref() {
        Some(server) => {
            let ip = crate::osc::local_ip().map(|ip| ip.to_string()).unwrap_or_else(|| "127.0.0.1".into());
            if let Some(port) = server.udp_port {
                let prefix = crate::osc::normalize_prefix(&ctrl.project.preferences.osc.prefix);
                ui.label(RichText::new(format!("{} {ip}:{port}  ·  {prefix}/…", tr("Écoute sur", "Listening on"))).small().color(GO));
            }
            if let Some(port) = server.http_port {
                ui.label(RichText::new(format!("OSCQuery : http://{ip}:{port}/")).small().color(GO));
            }
            for e in &server.startup_errors {
                ui.label(RichText::new(e).small().color(STOP));
            }
        }
        None => {
            ui.label(RichText::new(tr("Pilotage OSC désactivé.", "OSC control disabled.")).small().color(TEXT_DIM));
        }
    }
}
