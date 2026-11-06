//! Régie : barre d'outils, onglets de modules, barre d'état, préférences.

mod cuelist;
mod diaporama;
mod dialogs;
mod lecteur;
mod monitor_view;
mod output_view;

use crate::link::OutputLink;
use crate::prefs::MachinePrefs;
use crate::project::{self, Project};
use crate::protocol::{Command, PatternKind};
use dialogs::{Dialogs, Kind, Pick};
use std::path::PathBuf;
use std::time::Instant;
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
    output_view: output_view::OutputView,
    monitor: monitor_view::MonitorView,
    pattern: Option<PatternKind>,
    load: std::sync::Arc<std::sync::Mutex<crate::stats::Load>>,
    ctx: egui::Context,
    // ---- projet, comme JingleStar
    /// Nom donné au projet (sinon le nom du fichier).
    project_name: String,
    project_path: Option<PathBuf>,
    /// Contenu à la dernière ouverture / sauvegarde : sert à savoir s'il y a des modifications.
    saved: Project,
    /// Contenu de la dernière session écrite (sauvegarde automatique).
    session_saved: Project,
    autosave: Option<PathBuf>,
    modified: bool,
    last_check: Instant,
    dialogs: Dialogs,
    renaming: Option<String>,
    confirm_new: bool,
    /// Message bien en vue (session réparée, médias introuvables…).
    alert: Option<String>,
    missing: usize,
    status: Option<String>,
    window_title: String,
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
            output_view: Default::default(),
            monitor: Default::default(),
            pattern: None,
            load: crate::stats::start(),
            ctx: cc.egui_ctx.clone(),
            project_name: String::new(),
            project_path: project::last_project_path(),
            saved: Project::default(),
            session_saved: Project::default(),
            autosave: project::autosave_path(),
            modified: false,
            last_check: Instant::now(),
            dialogs: Dialogs::default(),
            renaming: None,
            confirm_new: false,
            alert: None,
            missing: 0,
            status: None,
            window_title: String::new(),
        };
        // Session restaurée au lancement (copie datée gardée), comme JingleStar.
        if let Some(p) = app.autosave.clone() {
            let s = project::load_session(&p);
            if s.alert.is_none() {
                project::snapshot_session(&p);
            }
            app.alert = s.alert;
            app.apply_project(&s.project);
            app.session_saved = s.project.clone();
            // Modifié tant que la session diffère du projet enregistré.
            app.saved = app.project_path.as_deref().and_then(|p| Project::load(p).ok()).unwrap_or_else(|| s.project.clone());
        }
        app.restart_output();
        app
    }

    // ------------------------------------------------------------ projet

    fn snapshot(&self) -> Project {
        Project { name: self.project_name.clone(), lecteur: self.lecteur.to_data(), diaporama: self.diaporama.to_data() }
    }

    fn apply_project(&mut self, p: &Project) {
        self.project_name = p.name.clone();
        self.lecteur.load_data(&p.lecteur);
        self.diaporama.load_data(&p.diaporama);
        self.missing = p.clone().missing_files().len();
    }

    fn display_name(&self) -> Option<String> {
        let n = self.project_name.trim();
        if !n.is_empty() {
            return Some(n.to_string());
        }
        self.project_path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().into_owned())
    }

    fn open_project(&mut self, path: PathBuf) {
        match Project::load_checked(&path) {
            Ok((mut p, repaired)) => {
                // Projet copié depuis un autre ordinateur : médias cherchés près du fichier projet.
                let moved = p.resolve_moved(&path);
                self.out.send(Command::Stop);
                self.apply_project(&p);
                self.saved = p;
                self.project_path = Some(path);
                project::remember_project_path(self.project_path.as_deref());
                let mut msg = "Projet ouvert".to_string();
                if moved > 0 {
                    msg += &format!(" ({moved} média(s) retrouvé(s) près du projet)");
                }
                if repaired {
                    self.alert = Some("Le projet contenait des valeurs invalides : elles ont repris leur réglage par défaut.".into());
                }
                self.status = Some(msg);
            }
            Err(e) => self.status = Some(format!("Ouverture impossible : {e}")),
        }
    }

    fn new_project(&mut self) {
        self.out.send(Command::Stop);
        let p = Project::default();
        self.apply_project(&p);
        self.saved = p;
        self.project_path = None;
        project::remember_project_path(None);
        self.status = Some("Nouveau projet".into());
    }

    /// « Enregistrer » : dans le projet ouvert, sinon « Enregistrer sous… ».
    fn save(&mut self, ctx: &egui::Context) {
        match self.project_path.clone() {
            Some(p) => self.save_as(p),
            None => self.dialogs.show(ctx, Pick::SaveProject, Kind::SaveProject),
        }
    }

    fn save_as(&mut self, path: PathBuf) {
        let p = self.snapshot();
        match p.save(&path) {
            Ok(()) => {
                self.status = Some(format!("Projet enregistré : {}", path.display()));
                self.saved = p;
                self.project_path = Some(path);
                project::remember_project_path(self.project_path.as_deref());
            }
            Err(e) => self.status = Some(format!("Enregistrement impossible : {e}")),
        }
    }

    fn on_picked(&mut self, pick: Pick, files: Vec<PathBuf>) {
        let Some(first) = files.first().cloned() else { return };
        match pick {
            Pick::SaveProject => self.save_as(first),
            Pick::OpenProject => self.open_project(first),
            Pick::Relocate => {
                let mut p = self.snapshot();
                let fixed = p.relocate(&first);
                self.apply_project(&p);
                self.status = Some(format!("{fixed} média(s) retrouvé(s), {} encore introuvable(s)", self.missing));
            }
        }
    }

    /// Modifications et sauvegarde automatique de la session (deux fois par seconde au plus).
    fn track_changes(&mut self) {
        if self.last_check.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_check = Instant::now();
        let now = self.snapshot();
        self.modified = now != self.saved;
        if now != self.session_saved {
            if let Some(p) = &self.autosave {
                let _ = now.save_with_backup(p);
            }
            self.session_saved = now;
        }
    }

    fn flush_session(&mut self) {
        let now = self.snapshot();
        if let Some(p) = &self.autosave {
            let _ = now.save_with_backup(p);
        }
        self.session_saved = now;
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.display_name() {
            Some(n) => format!("CueStar — {n}{}", if self.modified { " ●" } else { "" }),
            None => "CueStar".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn project_title(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(buf) = self.renaming.as_mut() {
                // Entrée valide, Échap ou un clic ailleurs annule.
                let id = ui.make_persistent_id("rename-project");
                let resp = ui.add(egui::TextEdit::singleline(buf).id(id).font(egui::FontId::proportional(17.0)).desired_width(220.0));
                if !resp.has_focus() && !resp.lost_focus() {
                    resp.request_focus();
                }
                if resp.lost_focus() {
                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                    let name = buf.trim().to_string();
                    self.renaming = None;
                    if enter {
                        self.project_name = name;
                    }
                }
            } else {
                let name = self.display_name().unwrap_or_else(|| "Sans titre".to_string());
                let mut tip = "Double-clic pour renommer le projet (le fichier garde son nom)".to_string();
                if let Some(p) = &self.project_path {
                    tip = format!("{tip}\n{}", p.display());
                }
                let resp = ui
                    .add(egui::Label::new(RichText::new(&name).strong().size(20.0).color(Color32::WHITE)).truncate().sense(egui::Sense::click()))
                    .on_hover_text(tip);
                if resp.double_clicked() {
                    self.renaming = Some(name);
                }
                if self.modified {
                    ui.label(RichText::new("●").size(13.0).color(ACCENT)).on_hover_text("Modifications non enregistrées");
                }
            }
            ui.menu_button(RichText::new("▾").color(TEXT_DIM), |ui| {
                if ui.button("Nouveau (Ctrl+N)").on_hover_text("Projet vierge (les préférences de la machine sont gardées). Pensez à enregistrer le projet en cours.").clicked() {
                    self.confirm_new = true;
                    ui.close();
                }
                if ui.button("Ouvrir… (Ctrl+O)").clicked() {
                    self.dialogs.show(ui.ctx(), Pick::OpenProject, Kind::OpenProject);
                    ui.close();
                }
                if ui.button("Enregistrer sous… (Ctrl+Maj+S)").clicked() {
                    self.dialogs.show(ui.ctx(), Pick::SaveProject, Kind::SaveProject);
                    ui.close();
                }
                ui.separator();
                if ui.button("Renommer le projet").clicked() {
                    self.renaming = Some(self.display_name().unwrap_or_default());
                    ui.close();
                }
            })
            .response
            .on_hover_text("Nouveau, Ouvrir, Enregistrer sous");
        });
    }

    /// Bandeau d'alerte en haut : message important, médias introuvables.
    fn alerts(&mut self, ui: &mut egui::Ui) {
        let banner = |ui: &mut egui::Ui, fill: Color32, text: String, add: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(text).strong().color(Color32::WHITE));
                    add(ui);
                });
            });
        };
        if let Some(a) = self.alert.clone() {
            let mut close = false;
            banner(ui, EDIT.gamma_multiply(0.8), a, &mut |ui| close = ui.button("OK").clicked());
            if close {
                self.alert = None;
            }
        }
        if self.missing > 0 {
            let mut relocate = false;
            let mut close = false;
            banner(ui, METER_MID.gamma_multiply(0.7), format!("⚠ {} média(s) introuvable(s)", self.missing), &mut |ui| {
                relocate = ui.button("Relocaliser…").on_hover_text("Choisir un dossier où chercher les médias par leur nom").clicked();
                close = ui.button("Ignorer").clicked();
            });
            if relocate {
                self.dialogs.show(ui.ctx(), Pick::Relocate, Kind::Folder);
            }
            if close {
                self.missing = 0;
            }
        }
    }

    fn restart_output(&mut self) {
        let ctx = self.ctx.clone();
        self.pattern = None;
        self.out.start(&self.prefs, std::sync::Arc::new(move || ctx.request_repaint()));
        self.monitor.sync(&mut self.out);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            // Le logo seul, discret : le nom du projet passe devant (comme JingleStar).
            widgets::logo_mark_play(ui, 26.0).on_hover_text("CueStar");
            ui.add_space(12.0);
            self.project_title(ui);
            ui.add_space(10.0);
            let file = self.project_path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().into_owned());
            let label = if self.modified { "● Enregistrer" } else { "Enregistrer" };
            let tip = match &file {
                Some(n) if self.modified => format!("Enregistre le projet « {n} » (Ctrl+S) : modifications non enregistrées"),
                Some(n) => format!("Enregistre le projet « {n} » (Ctrl+S)"),
                None => "Aucun projet ouvert : choisit où l'enregistrer (Ctrl+S)".to_string(),
            };
            if ui.button(label).on_hover_text(tip).clicked() {
                self.save(ui.ctx());
            }
            if self.confirm_new {
                if ui.add(egui::Button::new(RichText::new("Remplacer le projet ?").color(Color32::WHITE)).fill(STOP)).clicked() {
                    self.confirm_new = false;
                    self.new_project();
                }
                if ui.button("Annuler").clicked() {
                    self.confirm_new = false;
                }
            }
            ui.add_space(14.0);
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
                if ui
                    .selectable_label(self.monitor.open, "Monitoring")
                    .on_hover_text("Fenêtre flottante : retour de l'image envoyée sur la sortie")
                    .clicked()
                {
                    self.monitor.toggle(&mut self.out);
                }
                if ui
                    .selectable_label(self.output_view.open, "Video output")
                    .on_hover_text("Fenêtre flottante : écran et moteur de sortie, mires de test, mapping 4 coins")
                    .clicked()
                {
                    self.output_view.open = !self.output_view.open;
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
            ui.separator();
            let fps = if s.current.is_some() && s.video_fps > 0.0 { format!("{:.2} i/s", s.video_fps) } else { "— i/s".into() };
            let hz = if s.display_fps > 0.0 { format!(" (écran {:.2} Hz)", s.display_fps) } else { String::new() };
            ui.label(RichText::new(format!("{fps}{hz}")).color(TEXT_DIM)).on_hover_text("Images par seconde de la vidéo affichée sur la sortie");
            let load = self.load.lock().map(|l| *l).unwrap_or_default();
            let pct = |v: Option<f32>| v.map(|v| format!("{v:.0} %")).unwrap_or_else(|| "—".into());
            let heat = |v: Option<f32>| match v {
                Some(v) if v >= 90.0 => METER_HIGH,
                Some(v) if v >= 70.0 => METER_MID,
                _ => TEXT_DIM,
            };
            ui.separator();
            ui.label(RichText::new(format!("CPU {}", pct(load.cpu))).color(heat(load.cpu))).on_hover_text("Charge processeur de la machine");
            ui.separator();
            ui.label(RichText::new(format!("GPU {}", pct(load.gpu))).color(heat(load.gpu))).on_hover_text("Charge 3D de la carte graphique la plus sollicitée");
            if let Some(m) = &self.status {
                ui.separator();
                ui.label(RichText::new(m).color(TEXT_DIM));
            }
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
            self.pattern = Some(PatternKind::Bars);
        }

        if let Some((pick, files)) = self.dialogs.take() {
            self.on_picked(pick, files);
        }
        let typing = ctx.egui_wants_keyboard_input();
        if !typing && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.out.send(Command::Stop);
        }
        // Raccourcis de fichier habituels. Ctrl+Maj+S d'abord : Ctrl+S l'attraperait aussi.
        let cmd = egui::Modifiers::COMMAND;
        let (save_as, save, open, new) = ctx.input_mut(|i| {
            let save_as = i.consume_key(cmd | egui::Modifiers::SHIFT, Key::S);
            let save = i.consume_key(cmd, Key::S);
            let open = i.consume_key(cmd, Key::O);
            let new = i.consume_key(cmd, Key::N);
            (save_as, save, open, new)
        });
        if save_as {
            self.dialogs.show(&ctx, Pick::SaveProject, Kind::SaveProject);
        } else if save {
            self.save(&ctx);
        }
        if open {
            self.dialogs.show(&ctx, Pick::OpenProject, Kind::OpenProject);
        }
        if new {
            self.confirm_new = true;
        }

        egui::Panel::top("toolbar")
            .exact_size(50.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 6)))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_bar(ui));
        if self.alert.is_some() || self.missing > 0 {
            egui::Panel::top("alerts").frame(egui::Frame::new()).show_separator_line(false).show(ui, |ui| self.alerts(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(10)).show(ui, |ui| match self.tab {
            Tab::Lecteur => self.lecteur.ui(ui, &mut self.out, typing),
            Tab::Diaporama => self.diaporama.ui(ui, &mut self.out, typing),
            Tab::Cuelist => self.cuelist.ui(ui),
        });

        self.monitor.show(&ctx, &mut self.out);
        if let output_view::Action::Restart = self.output_view.show(&ctx, &mut self.prefs, &mut self.out, &mut self.pattern) {
            self.restart_output();
        }
        if self.tab == Tab::Lecteur {
            self.lecteur.handle_drops(&ctx);
        }

        self.track_changes();
        self.update_title(&ctx);
        let busy = self.out.state.current.is_some() || self.out.state.slide.is_some();
        ctx.request_repaint_after(Duration::from_millis(if busy { 50 } else { 250 }));
    }

    fn on_exit(&mut self) {
        self.flush_session();
        self.out.stop();
    }
}
