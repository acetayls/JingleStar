//! Régie : barre d'outils, onglets de modules, barre d'état, préférences.

mod cuelist;
mod diaporama;
mod lecteur;
mod prefs_view;

use crate::link::OutputLink;
use crate::prefs::MachinePrefs;
use crate::protocol::Command;
use egui::{Align, Color32, Key, Layout, RichText};
use star_commun::theme::{self, *};
use star_commun::widgets;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Lecteur,
    Diaporama,
    Cuelist,
}

pub struct CueStarApp {
    tab: Tab,
    prefs: MachinePrefs,
    out: OutputLink,
    lecteur: lecteur::Lecteur,
    diaporama: diaporama::Diaporama,
    cuelist: cuelist::Cuelist,
    prefs_view: prefs_view::PrefsView,
    pattern: bool,
    ctx: egui::Context,
}

impl CueStarApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        theme::apply(&cc.egui_ctx);
        let prefs = MachinePrefs::load();
        let mut app = Self {
            tab: Tab::Lecteur,
            prefs,
            out: OutputLink::new(),
            lecteur: Default::default(),
            diaporama: Default::default(),
            cuelist: Default::default(),
            prefs_view: Default::default(),
            pattern: false,
            ctx: cc.egui_ctx.clone(),
        };
        app.restart_output();
        app
    }

    fn restart_output(&mut self) {
        let ctx = self.ctx.clone();
        self.pattern = false;
        self.out.start(&self.prefs, move || ctx.request_repaint());
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            widgets::logo(ui, "Cue", 28.0);
            ui.add_space(18.0);
            for (t, name) in [(Tab::Lecteur, "Lecteur"), (Tab::Diaporama, "Diaporama"), (Tab::Cuelist, "Cuelist")] {
                let sel = self.tab == t;
                let txt = RichText::new(name).size(15.0).color(if sel { PLAYING_INK } else { TEXT });
                let b = egui::Button::new(txt).fill(if sel { ACCENT } else { PANEL_HI }).min_size(egui::vec2(96.0, 30.0));
                if ui.add(b).clicked() {
                    self.tab = t;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let stop = egui::Button::new(RichText::new("■  NOIR").strong().color(Color32::WHITE)).fill(STOP).min_size(egui::vec2(110.0, 32.0));
                if ui.add(stop).on_hover_text("Coupe l'image : écran de sortie au noir (Échap)").clicked() {
                    self.out.send(Command::Stop);
                }
                ui.add_space(8.0);
                if ui.button("⚙ Préférences").clicked() {
                    self.prefs_view.open(&self.prefs);
                }
                if ui
                    .selectable_label(self.pattern, "Mire")
                    .on_hover_text("Affiche une mire sur l'écran de sortie pour le repérer et le régler")
                    .clicked()
                {
                    self.pattern = !self.pattern;
                    self.out.send(Command::Pattern { on: self.pattern });
                }
                ui.add_space(8.0);
                self.output_badge(ui);
            });
        });
    }

    fn output_badge(&mut self, ui: &mut egui::Ui) {
        let s = &self.out.state;
        if !s.running {
            if ui.button("Relancer la sortie").clicked() {
                self.restart_output();
            }
            widgets::status_dot(ui, STOP, "Sortie arrêtée");
        } else if !s.ready {
            widgets::status_dot(ui, METER_MID, "Ouverture de la sortie…");
        } else {
            let m = s.monitor.clone().unwrap_or_default();
            let mode = if s.fullscreen { "plein écran" } else { "fenêtre" };
            let txt = format!("Sortie : {}×{} @ {:.2} Hz, {mode}", m.width, m.height, m.refresh_hz);
            let color = if s.mpv.is_some() { GO } else { METER_MID };
            widgets::status_dot(ui, color, txt).on_hover_text(m.name);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let s = &self.out.state;
        ui.horizontal_centered(|ui| {
            match (&s.mpv, &s.mpv_missing) {
                (Some(v), _) => {
                    ui.label(RichText::new(v).color(TEXT_DIM));
                }
                (None, Some(d)) => {
                    ui.label(RichText::new("libmpv absent : la sortie affiche la mire, la lecture est indisponible").color(METER_MID))
                        .on_hover_text(d);
                }
                _ => {}
            }
            ui.separator();
            let dec = if s.hwdec.is_empty() || s.hwdec == "no" { "CPU".to_string() } else { format!("GPU ({})", s.hwdec) };
            ui.label(RichText::new(format!("Décodage : {dec}")).color(TEXT_DIM));
            ui.separator();
            let col = if s.dropped > 0 { METER_HIGH } else { TEXT_DIM };
            ui.label(RichText::new(format!("Images perdues : {}  retardées : {}", s.dropped, s.delayed)).color(col));
            if let Some(e) = &s.last_error {
                ui.separator();
                ui.label(RichText::new(e).color(STOP));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("CueStar {}", env!("CARGO_PKG_VERSION"))).color(TEXT_DIM));
            });
        });
    }
}

impl eframe::App for CueStarApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let had_mpv_report = self.out.state.mpv_missing.is_some();
        self.out.poll();
        // Sans libmpv, la sortie démarre sur la mire : le bouton doit le refléter.
        if !had_mpv_report && self.out.state.mpv_missing.is_some() {
            self.pattern = true;
        }

        let typing = ctx.egui_wants_keyboard_input();
        if !typing && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.out.send(Command::Stop);
        }

        egui::Panel::top("toolbar")
            .exact_size(50.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 6)))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(10)).show(ui, |ui| match self.tab {
            Tab::Lecteur => self.lecteur.ui(ui, &mut self.out, typing),
            Tab::Diaporama => self.diaporama.ui(ui, &mut self.out, typing),
            Tab::Cuelist => self.cuelist.ui(ui),
        });

        if let Some(new_prefs) = self.prefs_view.show(&ctx, &self.out.state) {
            if new_prefs != self.prefs {
                self.prefs = new_prefs;
                self.prefs.save();
                self.restart_output();
            }
        }
        if self.tab == Tab::Lecteur {
            self.lecteur.handle_drops(&ctx);
        }

        let busy = self.out.state.current.is_some() || self.out.state.slide.is_some();
        ctx.request_repaint_after(Duration::from_millis(if busy { 50 } else { 250 }));
    }

    fn on_exit(&mut self) {
        self.out.stop();
    }
}
