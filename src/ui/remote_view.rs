//! Fenêtre des télécommandes : OSC, entrée MIDI, consoles Yamaha et Midas, et correspondances vers les actions.

use egui::{RichText, Ui};

use super::theme::*;
use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use crate::controller::Controller;
use crate::i18n::tr;
use crate::project::{ConsoleSettings, M32Settings, OscSettings};
use crate::remote::{ActionKind, Mapping, Trigger};

#[derive(Default)]
pub struct RemoteWindow {
    pub open: bool,
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
                let max_h = (ctx.content_rect().height() - 80.0).max(200.0);
                egui::ScrollArea::vertical().id_salt("remote-scroll").max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                self.osc_section(ui, ctrl);
                ui.separator();
                self.midi_section(ui, ctrl);
                ui.separator();
                self.console_section(ui, ctrl);
                ui.separator();
                self.m32_section(ui, ctrl);
                ui.separator();
                mappings(ui, ctrl);
                });
            });
        if !open {
            self.open = false;
            ctrl.learn = None;
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

impl RemoteWindow {
    fn console_section(&mut self, ui: &mut Ui, ctrl: &mut Controller) {
        ui.heading(tr("Console Yamaha (réseau)", "Yamaha console (network)"));
        let current = ctrl.project.preferences.console.clone();
        let draft = self.console.get_or_insert_with(|| current.clone());
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
            if ui.add_enabled(changed || ctrl.console.is_some(), egui::Button::new(tr("Connecter", "Connect"))).clicked() {
                apply = true;
                enabled = true;
            }
        });
        let mut silent = draft.silent;
        if ui
            .checkbox(&mut silent, tr("Écoute stricte : n'envoyer aucune donnée à la console (recommandé)", "Strict listening: send no data to the console (recommended)"))
            .on_hover_text(tr(
                "Décoché, JingleStar envoie seulement deux requêtes de lecture : le nom du modèle à la connexion \
                 (devinfo productname) et l'état de la console toutes les 5 s (devstatus runmode).",
                "When off, JingleStar only sends two read requests: the model name on connection \
                 (devinfo productname) and the console status every 5 s (devstatus runmode).",
            ))
            .changed()
        {
            draft.silent = silent;
            apply = current.enabled;
        }
        if apply {
            draft.enabled = enabled;
            let settings = draft.clone();
            if settings == current {
                ctrl.restart_console();
            } else {
                ctrl.set_console_settings(settings);
            }
        }
        ui.label(RichText::new(tr(
            "QL, CL, TF, DM7, Rivage : port 49280. JingleStar écoute les changements \
             (ON de canal, fader, rappel de scène) et n'envoie jamais de commande de modification.",
            "QL, CL, TF, DM7, Rivage: port 49280. JingleStar listens to changes \
             (channel ON, fader, scene recall) and never sends any command that changes a setting.",
        )).small().color(TEXT_DIM));
        if let Some(c) = ctrl.console.as_ref() {
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
    }
}

impl RemoteWindow {
    fn m32_section(&mut self, ui: &mut Ui, ctrl: &mut Controller) {
        ui.heading(tr("Console Midas M32 / Behringer X32 (réseau)", "Midas M32 / Behringer X32 console (network)"));
        let current = ctrl.project.preferences.m32.clone();
        let draft = self.m32.get_or_insert_with(|| current.clone());
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
            if ui.add_enabled(changed || ctrl.m32.is_some(), egui::Button::new(tr("Connecter", "Connect"))).clicked() {
                apply = true;
                enabled = true;
            }
        });
        if apply {
            draft.enabled = enabled;
            let settings = draft.clone();
            if settings == current {
                ctrl.restart_m32();
            } else {
                ctrl.set_m32_settings(settings);
            }
        }
        ui.label(RichText::new(tr(
            "Port 10023. La M32 n'envoie ses changements qu'aux clients abonnés : JingleStar envoie \
             « /xremote » toutes les 8 s et « /info », deux requêtes sans argument qui ne modifient rien. \
             Aucune autre donnée n'est envoyée.",
            "Port 10023. The M32 only sends its changes to subscribed clients: JingleStar sends \
             “/xremote” every 8 s and “/info”, two requests with no argument that change nothing. \
             No other data is sent.",
        )).small().color(TEXT_DIM));
        if let Some(c) = ctrl.m32.as_ref() {
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
    }
}

/// Édition à la main d'un déclencheur appris (ou saisi sans apprentissage).
/// Canaux, programmes et touches de console sont affichés à partir de 1, comme sur le matériel.
fn trigger_editor(ui: &mut Ui, t: &mut Trigger) {
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
                    ui.menu_button("✏", |ui| trigger_editor(ui, &mut m.trigger))
                        .response
                        .on_hover_text(tr("Modifier la valeur apprise", "Edit the learned value"));
                    if let Some(v) = ctrl.pickup_waiting(i) {
                        ui.label(RichText::new(format!("{} {v:.1} dB", tr("amener le fader à", "bring the fader to"))).small().color(ACCENT))
                            .on_hover_text(tr(
                                "Reprise en douceur : le fader agit quand il atteint le volume actuel, pour éviter un saut.",
                                "Soft takeover: the fader takes effect once it reaches the current volume, to avoid a jump.",
                            ));
                    }
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

/// Réglages OSC ; les changements partent avec « Appliquer ».
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
