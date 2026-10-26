//! Interface principale.

mod theme;
mod eq_view;
mod go_view;
mod help;
mod remote_view;
mod sound_editor;
mod widgets;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{vec2, Align, Color32, CornerRadius, FontId, Key, Layout, Rect, RichText, Stroke, UiBuilder};

use crate::audio::engine::{host_names, list_devices, DeviceDesc};
use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use crate::audio::loudness::{Loudness, NormalizeMode};
use crate::controller::{Controller, TrackGain};
use crate::i18n::{tr, Lang};
use crate::project::{AudioSettings, ModuleMode, Transition, DEFAULT_COLORS};
use theme::*;
use widgets::*;

/// Morceau de playlist en cours de glisser vers un bouton de jingle.
struct TrackPayload(PathBuf);

#[derive(Debug, Clone, Copy, PartialEq)]
enum DropTarget {
    Jingle(usize, usize),
    Playlist(usize),
    Go(usize),
}

struct SettingsDraft {
    audio: AudioSettings,
    hosts: Vec<String>,
    devices: Vec<DeviceDesc>,
}

pub struct JingleStarApp {
    ctrl: Controller,
    edit_mode: [bool; MAX_MODULES],
    /// Jingle ouvert dans l'éditeur.
    selected: Option<(usize, usize)>,
    /// Module dont la suppression attend confirmation.
    confirm_delete: Option<usize>,
    /// Playlist dont le vidage attend confirmation.
    confirm_clear: Option<usize>,
    /// Module dont l'égaliseur est ouvert.
    module_eq: Option<usize>,
    settings: Option<SettingsDraft>,
    help: help::HelpWindow,
    remote: remote_view::RemoteWindow,
    /// Niveaux affichés (dB), avec retombée progressive.
    meters: [[f32; 2]; MAX_MODULES],
    last_frame: Instant,
    drop_targets: Vec<(Rect, DropTarget)>,
    /// Position du curseur pendant un glisser-déposer de fichiers.
    drag_pos: Option<egui::Pos2>,
    /// Un module Texte avait le focus à l'image précédente : Échap y arrête quand même les jingles.
    notes_focused: bool,
    /// Module renommé par double-clic sur son titre, avec le nom en cours de saisie.
    renaming: Option<(usize, String)>,
    /// Cue de la Cuelist ouvert dans l'éditeur (module, index).
    cue_editor: Option<(usize, usize)>,
    /// « Nouveau » attend sa confirmation.
    confirm_new: bool,
    /// Type de module choisi en mode édition, en attente de validation.
    mode_draft: Option<(usize, ModuleMode)>,
}

impl JingleStarApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        Self {
            ctrl: Controller::new(move || ctx.request_repaint()),
            edit_mode: [false; MAX_MODULES],
            selected: None,
            confirm_delete: None,
            confirm_clear: None,
            cue_editor: None,
            confirm_new: false,
            mode_draft: None,
            module_eq: None,
            settings: None,
            help: help::HelpWindow::default(),
            remote: remote_view::RemoteWindow::default(),
            meters: [[-120.0; 2]; MAX_MODULES],
            last_frame: Instant::now(),
            drop_targets: Vec::new(),
            drag_pos: None,
            notes_focused: false,
            renaming: None,
        }
    }

    fn open_settings(&mut self) {
        let audio = self.ctrl.project.audio.clone();
        let devices = list_devices(audio.host.as_deref());
        self.settings = Some(SettingsDraft { audio, hosts: host_names(), devices });
    }

    fn update_meters(&mut self) {
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();
        for m in 0..MAX_MODULES {
            let peak = self.ctrl.engine.telemetry.take_peak(m);
            for c in 0..2 {
                let db = gain_to_db(peak[c]);
                let shown = &mut self.meters[m][c];
                *shown = if db > *shown { db } else { (*shown - 30.0 * dt).max(db).max(-120.0) };
            }
        }
    }

    // ------------------------------------------------------------ barre haute

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            logo(ui, 26.0);
            ui.add_space(16.0);
            let n = self.ctrl.project.module_count;
            // Un « + » épais et orange, dessiné : le bouton se repère d'un coup d'œil.
            let can_add = n < MAX_MODULES;
            let add = add_button(ui, tr("Ajouter un module", "Add module"), can_add)
                .on_disabled_hover_text(tr("4 modules au maximum", "4 modules maximum"));
            if add.clicked() {
                self.ctrl.add_module();
            }
            ui.label(RichText::new(format!("{n}/{MAX_MODULES}")).color(TEXT_DIM));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let panic = egui::Button::new(RichText::new(tr("■  TOUT ARRÊTER", "■  STOP ALL")).strong().color(Color32::WHITE))
                    .fill(STOP)
                    .min_size(vec2(150.0, 32.0));
                let panic = ui.add(panic).on_hover_text(tr("Arrête jingles et playlists", "Stops jingles and playlists"));
                if pressed(ui, &panic) {
                    self.ctrl.stop_all();
                }
                ui.add_space(8.0);
                if ui.selectable_label(self.help.open, tr("? Aide", "? Help")).on_hover_text("F1").clicked() {
                    self.help.toggle();
                }
                if ui
                    .selectable_label(self.remote.open, tr("Télécommandes", "Remote"))
                    .on_hover_text(tr("Pilotage MIDI et console Yamaha : correspondances et mode apprendre", "MIDI and Yamaha console control: mappings and learn mode"))
                    .clicked()
                {
                    self.remote.toggle();
                }
                if ui.button(tr("⚙ Préférences", "⚙ Preferences")).clicked() {
                    self.open_settings();
                }
                if ui.button(tr("Enregistrer sous…", "Save as…")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter(tr("Projet JingleStar", "JingleStar project"), &["jinglestar"])
                        .set_file_name("projet.jinglestar")
                        .save_file()
                    {
                        self.ctrl.save_project_as(p);
                    }
                }
                if ui.button(tr("Ouvrir…", "Open…")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter(tr("Projet JingleStar", "JingleStar project"), &["jinglestar", "json"]).pick_file() {
                        self.selected = None;
                        self.ctrl.open_project(p);
                    }
                }
                // Nouveau projet, après confirmation : le projet en cours est remplacé.
                if self.confirm_new {
                    if ui.button(tr("Annuler", "Cancel")).clicked() {
                        self.confirm_new = false;
                    }
                    if ui.add(egui::Button::new(RichText::new(tr("Remplacer le projet ?", "Replace the project?")).color(Color32::WHITE)).fill(STOP)).clicked() {
                        self.confirm_new = false;
                        self.selected = None;
                        self.cue_editor = None;
                        self.edit_mode = [false; MAX_MODULES];
                        self.ctrl.new_project();
                    }
                } else if ui.button(tr("Nouveau", "New")).on_hover_text(tr(
                    "Projet vierge : un module Jingles et un module Playlist (les préférences sont gardées). Pensez à enregistrer le projet en cours.",
                    "Blank project: one Jingles module and one Playlist module (preferences are kept). Save the current project first.",
                )).clicked() {
                    self.confirm_new = true;
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            match &self.ctrl.device {
                Some(d) => {
                    ui.label(RichText::new("●").color(GO));
                    ui.label(format!("{} · {} · {} {} · {} Hz", d.host, d.name, d.channels, tr("canaux", "channels"), d.sample_rate));
                }
                None => {
                    ui.label(RichText::new("●").color(STOP));
                    ui.label(tr("Aucune sortie audio", "No audio output"));
                }
            }
            if let Some(e) = &self.ctrl.device_error {
                ui.label(RichText::new(format!("⚠ {e}")).color(STOP));
            }
            if let Some(s) = &self.ctrl.status {
                ui.label(RichText::new(s).color(TEXT_DIM));
            }
            // À droite : tout est-il prêt pour un départ instantané ?
            let mem = self.ctrl.memory_status();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(fmt_bytes(mem.bytes)).monospace().color(TEXT_DIM))
                    .on_hover_text(tr("Mémoire occupée par les sons décodés", "Memory used by decoded sounds"));
                let (dot, col, text) = if !mem.failed.is_empty() {
                    let n = mem.failed.len();
                    ("⚠", STOP, if n == 1 { tr("1 son illisible", "1 unreadable sound").to_string() } else { format!("{n} {}", tr("sons illisibles", "unreadable sounds")) })
                } else if mem.ready < mem.total {
                    ("●", ACCENT, format!("{} {}/{}", tr("Chargement", "Loading"), mem.ready, mem.total))
                } else {
                    ("●", GO, tr("Tout est chargé", "All loaded").to_string())
                };
                let r = ui.label(RichText::new(text).color(col));
                ui.label(RichText::new(dot).color(col));
                let tip = if mem.failed.is_empty() {
                    format!("{} : {}/{}", tr("Jingles et cues prêts en mémoire", "Jingles and cues ready in memory"), mem.ready, mem.total)
                } else {
                    format!("{} :\n{}", tr("Introuvables ou illisibles", "Missing or unreadable"), mem.failed.join("\n"))
                };
                r.on_hover_text(tip);
            });
        });
    }

    // ---------------------------------------------------------------- modules

    fn modules_grid(&mut self, ui: &mut egui::Ui) {
        let n = self.ctrl.project.module_count;
        let area = ui.available_rect_before_wrap();
        let gap = 8.0;
        // Une rangée ; chaque module a sa part de largeur, réglable en glissant la séparation.
        let mut weights: Vec<f32> = (0..n).map(|m| self.ctrl.project.modules[m].width.max(0.05)).collect();
        let total: f32 = weights.iter().sum();
        let avail = (area.width() - gap * (n as f32 - 1.0)).max(1.0);
        let px = |w: f32| avail * w / total;
        let mut x = area.left();
        let mut rects = Vec::with_capacity(n);
        for &w in &weights {
            rects.push(Rect::from_min_size(egui::pos2(x, area.top()), vec2(px(w), area.height())));
            x += px(w) + gap;
        }
        for (m, rect) in rects.iter().enumerate() {
            ui.scope_builder(UiBuilder::new().max_rect(*rect).id_salt(("module", m)), |ui| {
                self.module_panel(ui, m, *rect);
            });
        }
        // Séparations : glisser pour répartir la largeur entre deux modules, double-clic pour égaliser.
        let min_px = 280.0_f32.min(avail / n as f32);
        let mut changed = false;
        for i in 0..n.saturating_sub(1) {
            let r = Rect::from_x_y_ranges(rects[i].right()..=rects[i + 1].left(), area.y_range());
            let resp = ui
                .interact(r, ui.make_persistent_id(("module_split", i)), egui::Sense::click_and_drag())
                .on_hover_text(tr("Glisser pour régler la largeur ; double-clic pour égaliser", "Drag to set the width; double-click to equalize"));
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(2.0, ACCENT.gamma_multiply(0.7)));
            }
            if resp.double_clicked() {
                weights.iter_mut().for_each(|w| *w = 1.0);
                changed = true;
            } else if resp.dragged() {
                let (a, b) = (px(weights[i]), px(weights[i + 1]));
                let dx = resp.drag_delta().x.clamp(min_px - a, b - min_px);
                if dx != 0.0 {
                    weights[i] = (a + dx) / avail * total;
                    weights[i + 1] = (b - dx) / avail * total;
                    changed = true;
                }
            }
        }
        if changed {
            self.ctrl.set_module_widths(&weights);
        }
        ui.allocate_rect(area, egui::Sense::hover());
    }

    fn module_panel(&mut self, ui: &mut egui::Ui, m: usize, rect: Rect) {
        let playing = (0..crate::audio::mixer::SLOTS).any(|s| self.ctrl.engine.telemetry.position(m, s).is_some());
        let border = if playing { GO } else { BORDER };
        ui.painter().rect_filled(rect, CornerRadius::same(6), PANEL);
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, border), egui::StrokeKind::Inside);
        let inner = rect.shrink(8.0);
        ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
            self.module_header(ui, m);
            ui.add_space(2.0);
            let body = ui.available_rect_before_wrap();
            if self.ctrl.project.modules[m].mode == ModuleMode::Text {
                ui.scope_builder(UiBuilder::new().max_rect(body), |ui| self.notes_view(ui, m, body));
                ui.allocate_rect(body, egui::Sense::hover());
                return;
            }
            let strip_w = 46.0;
            let content = Rect::from_min_max(body.min, egui::pos2(body.right() - strip_w - 6.0, body.bottom()));
            let strip = Rect::from_min_max(egui::pos2(body.right() - strip_w, body.top()), body.max);
            // Le contenu ne déborde jamais sous le fader, même dans un module étroit.
            ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                ui.set_clip_rect(content.expand(2.0).intersect(ui.clip_rect()));
                match self.ctrl.project.modules[m].mode {
                ModuleMode::Jingle => self.jingle_grid(ui, m, content),
                ModuleMode::Playlist => self.playlist_view(ui, m, content),
                ModuleMode::Go => self.go_view(ui, m, content),
                ModuleMode::Text => {}
                }
            });
            ui.scope_builder(UiBuilder::new().max_rect(strip), |ui| self.fader_strip(ui, m, strip));
            ui.allocate_rect(body, egui::Sense::hover());
        });
    }

    /// Module Texte : un grand champ libre pour la conduite, sauvegardé avec le projet.
    fn notes_view(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        let mut text = self.ctrl.project.modules[m].notes.clone();
        let mut focused = false;
        egui::ScrollArea::vertical().id_salt(("notes", m)).auto_shrink([false, false]).max_height(area.height()).show(ui, |ui| {
            let resp = ui.add_sized(
                vec2(area.width(), area.height()),
                egui::TextEdit::multiline(&mut text)
                    .font(FontId::proportional(15.0))
                    .hint_text(tr("Écrire la conduite ici…", "Write your running order here…"))
                    .desired_width(f32::INFINITY)
                    .lock_focus(true),
            );
            focused = resp.has_focus();
            if resp.changed() {
                self.ctrl.set_module_notes(m, text);
            }
        });
        self.notes_focused |= focused;
    }

    fn remove_module(&mut self, m: usize) {
        self.ctrl.remove_module(m);
        self.confirm_delete = None;
        self.renaming = None;
        self.selected = None;
        self.module_eq = None;
        // Les modes édition suivent les modules qui remontent.
        for i in m..MAX_MODULES - 1 {
            self.edit_mode[i] = self.edit_mode[i + 1];
            self.meters[i] = self.meters[i + 1];
        }
        self.edit_mode[MAX_MODULES - 1] = false;
    }

    fn module_header(&mut self, ui: &mut egui::Ui, m: usize) {
        ui.horizontal(|ui| {
            let badge = RichText::new(format!(" {} ", m + 1)).strong().color(Color32::BLACK).background_color(ACCENT);
            ui.label(badge);
            let editing = self.edit_mode[m];
            if editing {
                let mut name = self.ctrl.project.modules[m].name.clone();
                if ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0)).changed() {
                    self.ctrl.rename_module(m, name);
                }
            } else if let Some((_, buf)) = self.renaming.as_mut().filter(|(rm, _)| *rm == m) {
                // Entrée valide, Échap ou un clic ailleurs annule.
                let id = ui.make_persistent_id(("rename", m));
                let resp = ui.add(egui::TextEdit::singleline(buf).id(id).desired_width(130.0));
                if !resp.has_focus() && !resp.lost_focus() {
                    resp.request_focus();
                }
                if resp.lost_focus() {
                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                    let name = buf.trim().to_string();
                    self.renaming = None;
                    if enter && !name.is_empty() {
                        self.ctrl.rename_module(m, name);
                    }
                }
            } else {
                let name = self.ctrl.project.modules[m].name.clone();
                let resp = ui
                    .add(egui::Label::new(RichText::new(&name).strong()).sense(egui::Sense::click()))
                    .on_hover_text(tr("Double-clic pour renommer", "Double-click to rename"));
                if resp.double_clicked() {
                    self.renaming = Some((m, name));
                }
            }
            if !editing {
                let mode = self.ctrl.project.modules[m].mode;
                ui.label(RichText::new(mode_label(mode).to_uppercase()).small().strong().color(TEXT_DIM))
                    .on_hover_text(tr("Type du module : se change en mode édition (✏)", "Module type: change it in edit mode (✏)"));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if editing && self.ctrl.project.module_count > 1 {
                    if self.confirm_delete == Some(m) {
                        if ui.add(egui::Button::new(RichText::new(tr("Supprimer ?", "Delete?")).color(Color32::WHITE)).fill(STOP)).clicked() {
                            self.remove_module(m);
                            return;
                        }
                    } else if ui.button("🗑").on_hover_text(tr("Supprimer le module", "Delete module")).clicked() {
                        self.confirm_delete = Some(m);
                    }
                }
                let edit = ui.selectable_label(editing, "✏").on_hover_text(tr("Mode édition", "Edit mode"));
                if edit.clicked() {
                    self.edit_mode[m] = !editing;
                    self.confirm_delete = None;
                    self.mode_draft = None;
                    if editing && self.selected.map(|(sm, _)| sm == m).unwrap_or(false) {
                        self.selected = None;
                    }
                }
                if self.ctrl.project.modules[m].mode == ModuleMode::Text {
                    return;
                }
                let eq_on = !self.ctrl.project.modules[m].eq.is_flat();
                let eq_text = RichText::new("EQ").color(if eq_on { ACCENT } else { TEXT_DIM }).strong();
                if editing {
                    let open = self.module_eq == Some(m);
                    if ui.selectable_label(open, eq_text).on_hover_text(tr("Égaliseur du module", "Module equalizer")).clicked() {
                        self.module_eq = if open { None } else { Some(m) };
                    }
                } else if eq_on {
                    ui.label(eq_text.small()).on_hover_text(tr("Égaliseur du module actif", "Module equalizer on"));
                }
                self.output_combo(ui, m);
            });
        });
        // Le type du module ne se change qu'en mode édition, et après validation :
        // en spectacle, rien ne peut le basculer par erreur.
        if self.edit_mode[m] {
            ui.horizontal(|ui| {
                let mode = self.ctrl.project.modules[m].mode;
                let draft = self.mode_draft.filter(|(dm, _)| *dm == m).map_or(mode, |(_, d)| d);
                let mut pick = draft;
                ui.label(RichText::new(tr("Type", "Type")).color(TEXT_DIM));
                egui::ComboBox::from_id_salt(("module-type", m)).selected_text(mode_label(pick)).show_ui(ui, |ui| {
                    for mm in [ModuleMode::Jingle, ModuleMode::Playlist, ModuleMode::Go, ModuleMode::Text] {
                        ui.selectable_value(&mut pick, mm, mode_label(mm));
                    }
                });
                if pick != draft {
                    self.mode_draft = Some((m, pick));
                }
                if pick != mode {
                    if ui.add(egui::Button::new(RichText::new(tr("Valider", "Apply")).strong().color(Color32::BLACK)).fill(ACCENT)).clicked() {
                        self.ctrl.set_module_mode(m, pick);
                        self.mode_draft = None;
                    }
                    if ui.button(tr("Annuler", "Cancel")).clicked() {
                        self.mode_draft = None;
                    }
                }
            });
            let mode = self.ctrl.project.modules[m].mode;
            if self.mode_draft.is_some_and(|(dm, d)| dm == m && d != mode) {
                let playing = (0..crate::audio::mixer::SLOTS).any(|s| self.ctrl.engine.telemetry.position(m, s).is_some());
                let warn = if playing {
                    RichText::new(tr("⚠ Le son en cours de ce module sera coupé.", "⚠ This module's sound will be cut.")).color(STOP)
                } else {
                    RichText::new(tr("Le contenu actuel est gardé : on le retrouve en revenant à ce type.", "Current content is kept: switch back to find it again.")).color(TEXT_DIM)
                };
                ui.label(warn.small());
            }
        }
    }

    fn output_combo(&mut self, ui: &mut egui::Ui, m: usize) {
        let pairs = self.ctrl.output_pairs();
        let cur = self.ctrl.project.modules[m].output_pair;
        let label = |p: usize| format!("{} {}-{}", tr("Sortie", "Output"), p * 2 + 1, p * 2 + 2);
        let text = if cur < pairs {
            RichText::new(label(cur))
        } else {
            RichText::new(format!("⚠ {} → 1-2", label(cur))).color(ACCENT)
        };
        egui::ComboBox::from_id_salt(("out", m)).selected_text(text).width(96.0).show_ui(ui, |ui| {
            for p in 0..pairs.max(cur + 1) {
                let mut t = RichText::new(label(p));
                if p >= pairs {
                    t = t.color(TEXT_DIM);
                }
                if ui.selectable_label(p == cur, t).clicked() {
                    self.ctrl.set_module_output(m, p);
                }
            }
        });
    }

    fn fader_strip(&mut self, ui: &mut egui::Ui, m: usize, strip: Rect) {
        let stop_h = 26.0;
        let label_h = 18.0;
        let fader_area = Rect::from_min_max(strip.min, egui::pos2(strip.right(), strip.bottom() - stop_h - label_h - 6.0));
        let meter_rect = Rect::from_min_max(egui::pos2(fader_area.right() - 12.0, fader_area.top() + 6.0), egui::pos2(fader_area.right(), fader_area.bottom() - 6.0));
        meter(ui, meter_rect, self.meters[m]);

        let slider_rect = Rect::from_min_max(fader_area.min, egui::pos2(meter_rect.left() - 2.0, fader_area.bottom()));
        let mut db = self.ctrl.project.modules[m].volume_db;
        ui.spacing_mut().slider_width = slider_rect.height() - 8.0;
        let resp = ui.put(
            slider_rect,
            egui::Slider::new(&mut db, -60.0..=10.0).vertical().show_value(false).trailing_fill(true),
        );
        // Repères dessinés d'après la place réelle du curseur (centré dans la zone).
        db_marks(ui, resp.rect, true, &(-60.0..=10.0), true);
        if resp.double_clicked() {
            db = 0.0;
        }
        if db != self.ctrl.project.modules[m].volume_db {
            self.ctrl.set_module_volume(m, db);
        }
        let typed = db_value(
            ui,
            ui.make_persistent_id(("module_db", m)),
            egui::pos2(strip.center().x, fader_area.bottom() + 4.0),
            egui::Align2::CENTER_TOP,
            db,
            -60.0,
            -60.0..=10.0,
            FontId::proportional(11.0),
            if db.abs() < 0.05 { TEXT_DIM } else { TEXT },
        );
        if let Some(v) = typed {
            self.ctrl.set_module_volume(m, v);
        }
        let stop_rect = Rect::from_min_max(egui::pos2(strip.left(), strip.bottom() - stop_h), strip.max);
        let stop = ui.put(stop_rect, egui::Button::new(RichText::new("■").color(Color32::WHITE)).fill(STOP.gamma_multiply(0.6))).on_hover_text(tr("Arrêter le module", "Stop module"));
        if pressed(ui, &stop) {
            self.ctrl.stop_module(m);
        }
    }

    // ---------------------------------------------------------------- jingles

    fn jingle_grid(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        let (cols, rows) = (3usize, 4usize);
        let gap = 6.0;
        let w = (area.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = (area.height() - gap * (rows as f32 - 1.0)) / rows as f32;
        let hover_pos = self.drag_pos;
        for s in 0..JINGLE_SLOTS {
            let (r, c) = (s / cols, s % cols);
            let rect = Rect::from_min_size(area.min + vec2(c as f32 * (w + gap), r as f32 * (h + gap)), vec2(w, h));
            self.drop_targets.push((rect, DropTarget::Jingle(m, s)));
            let slot = self.ctrl.jingle(m, s).cloned();
            let pos = self.ctrl.engine.telemetry.position(m, s);
            let rate = self.ctrl.device.as_ref().map(|d| d.sample_rate).unwrap_or(48_000) as f64;
            let (progress, duration_label) = match (&slot, pos) {
                (Some(j), Some((p, len))) => {
                    let (start, _, _, end) = j.sound.markers(len as f64 / rate);
                    let t = p as f64 / rate;
                    let span = (end - start).max(1e-6);
                    let elapsed = fmt_tenths(t - start);
                    // En boucle, la fin n'arrive qu'à l'arrêt : « BOUCLE » à la place du temps restant.
                    let remaining = if j.sound.looping { tr("BOUCLE", "LOOP").to_string() } else { format!("-{}", fmt_tenths((end - t).max(0.0) + 0.099)) };
                    (Some((((t - start) / span) as f32, elapsed, remaining)), None)
                }
                (Some(j), None) => {
                    let label = self.ctrl.duration_of(&j.path).map(|d| {
                        let (st, _, _, end) = j.sound.markers(d);
                        fmt_tenths(end - st)
                    });
                    (None, label)
                }
                _ => (None, None),
            };
            let view = SlotView {
                index: s,
                name: slot.as_ref().map(|j| j.name.as_str()),
                color: slot.as_ref().map(|j| j.color).unwrap_or(DEFAULT_COLORS[0]),
                playing: pos.is_some(),
                fading: pos.is_some() && self.ctrl.engine.telemetry.is_fading(m, s),
                progress,
                duration_label,
                loading: slot.as_ref().map(|j| self.ctrl.is_loading(&j.path)).unwrap_or(false),
                error: slot.as_ref().map(|j| self.ctrl.load_errors.contains_key(&j.path)).unwrap_or(false),
                looping: slot.as_ref().map(|j| j.sound.looping).unwrap_or(false),
                selected: self.selected == Some((m, s)),
                edit_mode: self.edit_mode[m],
                drop_hover: hover_pos.map(|p| rect.contains(p)).unwrap_or(false)
                    || (egui::DragAndDrop::has_payload_of_type::<TrackPayload>(ui.ctx())
                        && ui.ctx().pointer_hover_pos().is_some_and(|p| rect.contains(p))),
                eq: slot.as_ref().map(|j| !j.sound.eq.is_flat()).unwrap_or(false),
            };
            let resp = jingle_button(ui, rect, &view);
            if let Some(j) = &slot {
                if let Some(v) = jingle_volume(ui, ui.make_persistent_id(("jingle_db", m, s)), rect, j.sound.volume_db, pos.is_some()) {
                    self.ctrl.update_jingle(m, s, |j| j.sound.volume_db = v);
                }
            }
            // Morceau glissé depuis une playlist : il devient le son de ce bouton.
            if let Some(track) = resp.dnd_release_payload::<TrackPayload>() {
                self.ctrl.assign_jingle(m, s, track.0.clone());
                continue;
            }
            if !self.edit_mode[m] && slot.is_some() {
                // En lecture, le jingle part dès l'appui.
                if pressed(ui, &resp) {
                    self.ctrl.trigger_jingle(m, s);
                }
            } else if resp.clicked() {
                if self.edit_mode[m] {
                    if slot.is_none() {
                        self.pick_jingle_file(m, s);
                    }
                    self.selected = Some((m, s));
                } else {
                    // Bouton vide : on propose directement de choisir un son.
                    self.pick_jingle_file(m, s);
                }
            }
            resp.context_menu(|ui| {
                if ui.button(tr("Choisir un fichier…", "Choose a file…")).clicked() {
                    self.pick_jingle_file(m, s);
                    ui.close();
                }
                if slot.is_some() {
                    if ui.button(tr("Éditer…", "Edit…")).clicked() {
                        // Ouvre l'éditeur sans passer le module en mode édition.
                        self.selected = Some((m, s));
                        ui.close();
                    }
                    if ui.button(tr("Vider", "Clear")).clicked() {
                        self.ctrl.clear_jingle(m, s);
                        ui.close();
                    }
                }
            });
        }
    }

    fn pick_jingle_file(&mut self, m: usize, s: usize) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Audio", crate::audio::decode::AUDIO_EXTENSIONS)
            .pick_file()
        {
            self.ctrl.assign_jingle(m, s, p);
        }
    }

    fn jingle_editor(&mut self, ctx: &egui::Context) {
        let Some((m, s)) = self.selected else { return };
        let Some(j) = self.ctrl.jingle(m, s).cloned() else {
            self.selected = None;
            return;
        };
        let mut done = false;
        let mut open = true;
        egui::Window::new(format!("Jingle {}.{} — {}", m + 1, s + 1, j.name))
            .id(egui::Id::unique("jingle-editor"))
            .open(&mut open)
            .default_width(620.0)
            .resizable(true)
            .collapsible(false)
            .max_height((ctx.content_rect().height() - 60.0).max(200.0))
            .show(ctx, |ui| {
                // Ascenseur quand la fenêtre de JingleStar est plus petite que l'éditeur.
                egui::ScrollArea::vertical().id_salt("jingle-editor-scroll").auto_shrink([false, true]).show(ui, |ui| {
                    let mut j2 = j.clone();
                    if sound_editor::listen_button(ui, ("jingle-listen", m, s), self.ctrl.is_jingle_playing(m, s)) {
                        self.ctrl.trigger_jingle(m, s);
                    }
                    ui.add_space(4.0);
                    egui::Grid::new("jingle-fields").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label(tr("Nom", "Name"));
                        ui.text_edit_singleline(&mut j2.name);
                        ui.end_row();

                        ui.label(tr("Fichier", "File"));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(j.path.display().to_string()).color(TEXT_DIM).small());
                            if ui.button(tr("Changer…", "Change…")).clicked() {
                                self.pick_jingle_file(m, s);
                            }
                        });
                        ui.end_row();

                        sound_editor::sound_rows(ui, &mut j2.sound, true);

                        ui.label(tr("Couleur", "Color"));
                        ui.horizontal(|ui| {
                            for c in DEFAULT_COLORS {
                                let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                                ui.painter().rect_filled(r, CornerRadius::same(3), rgb(c));
                                if j2.color == c {
                                    ui.painter().rect_stroke(r, CornerRadius::same(3), Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
                                }
                                if resp.clicked() {
                                    j2.color = c;
                                }
                            }
                        });
                        ui.end_row();
                    });

                    ui.add_space(6.0);
                    let buf = self.ctrl.buffer_of(&j.path);
                    let status = load_status(&self.ctrl, &j.path);
                    let playhead = self.ctrl.engine.telemetry.position(m, s).map(|(p, _)| p);
                    let rate = self.ctrl.sample_rate();
                    if let Some(frame) = sound_editor::sound_waveform_and_eq(ui, ("jingle", m, s), &mut j2.sound, buf.as_deref(), status, playhead, rate) {
                        self.ctrl.jingle_seek(m, s, frame);
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(tr("✓ Terminé", "✓ Done")).clicked() {
                                done = true;
                            }
                            if ui.button(RichText::new(tr("Vider l'emplacement", "Clear slot")).color(STOP)).clicked() {
                                self.ctrl.clear_jingle(m, s);
                                done = true;
                            }
                        });
                    });

                    if j2 != j {
                        self.ctrl.update_jingle(m, s, |slot| *slot = j2);
                    }
                });
            });
        if !open || done {
            // Fin de l'édition : le module repasse en mode lecture, un clic relance le son.
            self.selected = None;
            self.edit_mode[m] = false;
        }
    }

    fn module_eq_window(&mut self, ctx: &egui::Context) {
        let Some(m) = self.module_eq else { return };
        if m >= self.ctrl.project.module_count {
            self.module_eq = None;
            return;
        }
        let mut open = true;
        let mut eq = self.ctrl.project.modules[m].eq;
        let rate = self.ctrl.sample_rate();
        egui::Window::new(format!("{} — {}", tr("Égaliseur", "Equalizer"), self.ctrl.project.modules[m].name))
            .id(egui::Id::unique("module-eq"))
            .open(&mut open)
            .default_width(640.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(tr(
                    "S'applique à tout le module : jingles et playlist.",
                    "Applies to the whole module: jingles and playlist.",
                )).small().color(TEXT_DIM));
                if eq_view::eq_editor(ui, ("module", m), &mut eq, rate) {
                    self.ctrl.set_module_eq(m, eq);
                }
            });
        if !open {
            self.module_eq = None;
        }
    }

    // --------------------------------------------------------------- playlist

    fn playlist_view(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        self.drop_targets.push((area, DropTarget::Playlist(m)));
        let editing = self.edit_mode[m];
        let pl = self.ctrl.project.modules[m].playlist.clone();
        let rt = self.ctrl.playlists[m].clone();

        ui.horizontal(|ui| {
            // Fichiers ou dossiers ajoutés à la suite de la liste en cours, ou dossier qui la remplace.
            let add = add_button(ui, tr("Ajouter", "Add"), true);
            egui::Popup::menu(&add).show(|ui| {
                if ui.button(tr("Des fichiers…", "Files…")).clicked() {
                    ui.close();
                    if let Some(files) = rfd::FileDialog::new()
                        .add_filter("Audio", crate::audio::decode::AUDIO_EXTENSIONS)
                        .pick_files()
                    {
                        self.ctrl.add_playlist_files(m, files);
                    }
                }
                if ui.button(tr("Un dossier…", "A folder…")).clicked() {
                    ui.close();
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.ctrl.add_playlist_files(m, vec![dir]);
                    }
                }
                ui.separator();
                if ui.button(tr("Ouvrir un dossier (remplace la liste)…", "Open a folder (replaces the list)…")).clicked() {
                    ui.close();
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.ctrl.set_playlist_folder(m, dir);
                    }
                }
            });
            if pl.folder.is_some() && ui.button("⟳").on_hover_text(tr("Relire le dossier", "Rescan folder")).clicked() {
                self.ctrl.rescan_playlist(m);
            }
            if !pl.tracks.is_empty() {
                if self.confirm_clear == Some(m) {
                    if ui.add(egui::Button::new(RichText::new(tr("Vider ?", "Clear?")).color(Color32::WHITE)).fill(STOP)).clicked() {
                        self.ctrl.clear_playlist(m);
                        self.confirm_clear = None;
                    }
                    if ui.button(tr("Annuler", "Cancel")).clicked() {
                        self.confirm_clear = None;
                    }
                } else if ui.button(tr("🗑 Vider", "🗑 Clear")).on_hover_text(tr("Vider la playlist (avec confirmation)", "Clear the playlist (asks first)")).clicked() {
                    self.confirm_clear = Some(m);
                }
            }
            let name = pl
                .folder
                .as_ref()
                .and_then(|f| f.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| {
                    if pl.tracks.is_empty() { tr("Aucun dossier", "No folder") } else { tr("Liste libre", "Custom list") }.into()
                });
            ui.add(egui::Label::new(RichText::new(name).color(TEXT_DIM)).truncate());
        });

        // Morceau en cours.
        let current_name = rt
            .current
            .and_then(|i| pl.tracks.get(i))
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "—".into());
        let rate = self.ctrl.device.as_ref().map(|d| d.sample_rate).unwrap_or(48_000) as f64;
        let pos = self.ctrl.playlist_position(m);
        egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x1c, 0x20)).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.set_width(area.width() - 12.0);
            let col = if rt.paused { ACCENT } else if rt.playing { GO } else { TEXT };
            ui.horizontal(|ui| {
                ui.label(RichText::new(&current_name).strong().size(15.0).color(col));
                if rt.paused {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(tr("EN PAUSE", "PAUSED")).strong().small().color(ACCENT));
                    });
                }
            });
            let (mut t, len) = pos.map(|(p, l)| (p as f64 / rate, l as f64 / rate)).unwrap_or((0.0, 0.0));
            // Barre de lecture : un clic ou un glisser (appliqué au relâché) déplace la lecture.
            let (hit, bar_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), egui::Sense::click_and_drag());
            let r = Rect::from_center_size(hit.center(), vec2(hit.width(), 6.0));
            let frac_at = |x: f32| ((x - r.left()) / r.width()).clamp(0.0, 1.0) as f64;
            if len > 0.0 {
                if let Some(p) = bar_resp.interact_pointer_pos() {
                    if bar_resp.dragged() {
                        t = frac_at(p.x) * len;
                    }
                    if bar_resp.clicked() || bar_resp.drag_stopped() {
                        self.ctrl.playlist_seek(m, frac_at(p.x));
                        t = frac_at(p.x) * len;
                    }
                }
                if bar_resp.hovered() || bar_resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
            ui.painter().rect_filled(r, CornerRadius::same(3), PANEL_HI);
            if len > 0.0 {
                let w = r.width() * (t / len).clamp(0.0, 1.0) as f32;
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), CornerRadius::same(3), GO);
                if bar_resp.hovered() || bar_resp.dragged() {
                    ui.painter().circle_filled(egui::pos2(r.left() + w, r.center().y), 5.0, Color32::WHITE);
                }
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(fmt_time(t)).monospace().color(TEXT_DIM));
                if let Some(i) = rt.current {
                    let g = self.ctrl.track_gain(m, i);
                    if !matches!(g, TrackGain::Off) {
                        let (txt, col, tip, _) = gain_label(&g);
                        ui.label(RichText::new(format!("{} {txt}", tr("Gain", "Gain"))).monospace().color(col)).on_hover_text(tip);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("-{}", fmt_time((len - t).max(0.0)))).monospace().color(TEXT_DIM));
                });
            });
        });

        // Transport : précédent, GO, pause, stop, suivant sur toute la largeur.
        let (row, _) = ui.allocate_exact_size(vec2(area.width(), 38.0), egui::Sense::hover());
        // Dans un module étroit, tous les boutons rétrécissent ensemble (GO garde la plus grande part).
        let gap = 4.0;
        let k = ((row.width() - 4.0 * gap) / (2.0 * 40.0 + 2.0 * 48.0 + 64.0)).min(1.0);
        let (side, stop_w) = (40.0 * k, 48.0 * k);
        let go_w = row.width() - 2.0 * side - 2.0 * stop_w - 4.0 * gap;
        let mut x = row.left();
        let mut cell = |w: f32| {
            let r = Rect::from_min_size(egui::pos2(x, row.top()), vec2(w, row.height()));
            x += w + gap;
            r
        };
        let (prev_r, go_r, pause_r, stop_r, next_r) = (cell(side), cell(go_w), cell(stop_w), cell(stop_w), cell(side));
        if transport_button(ui, prev_r, ("pl-prev", m), TransportIcon::Prev, None, PANEL_HI, TEXT)
            .on_hover_text(tr("Morceau précédent", "Previous track"))
            .clicked()
        {
            self.ctrl.playlist_step(m, -1);
        }
        let go = transport_button(ui, go_r, ("pl-go", m), TransportIcon::Play, Some("GO"), GO, Color32::from_rgb(0x0c, 0x1a, 0x0e))
            .on_hover_text(if rt.paused {
                tr("Reprendre là où le morceau s'est arrêté", "Resume where the track was paused")
            } else {
                tr("Lancer le morceau sélectionné", "Play the selected track")
            });
        if pressed(ui, &go) {
            self.ctrl.playlist_go(m);
        }
        let (pause_fill, pause_fg) = if rt.paused { (ACCENT, Color32::BLACK) } else { (PANEL_HI, TEXT) };
        let pause = transport_button(ui, pause_r, ("pl-pause", m), TransportIcon::Pause, None, pause_fill, pause_fg)
            .on_hover_text(if rt.paused {
                tr("Reprendre", "Resume")
            } else {
                tr("Pause : le morceau reprendra au même endroit", "Pause: the track will resume at the same spot")
            });
        if pressed(ui, &pause) {
            self.ctrl.playlist_toggle_pause(m);
        }
        let stop = transport_button(ui, stop_r, ("pl-stop", m), TransportIcon::Stop, None, STOP.gamma_multiply(0.75), Color32::WHITE)
            .on_hover_text(tr("Arrêter", "Stop"));
        if pressed(ui, &stop) {
            self.ctrl.playlist_stop(m);
        }
        if transport_button(ui, next_r, ("pl-next", m), TransportIcon::Next, None, PANEL_HI, TEXT)
            .on_hover_text(tr("Morceau suivant", "Next track"))
            .clicked()
        {
            self.ctrl.playlist_step(m, 1);
        }

        // Modes : les morceaux s'enchaînent toujours, dans l'ordre ou au hasard.
        let (row, _) = ui.allocate_exact_size(vec2(area.width(), 26.0), egui::Sense::hover());
        let half = (row.width() - 4.0) / 2.0;
        let shuffle_r = Rect::from_min_size(row.min, vec2(half, row.height()));
        let loop_r = Rect::from_min_size(egui::pos2(row.left() + half + 4.0, row.top()), vec2(half, row.height()));
        if mode_chip(ui, shuffle_r, ("pl-shuffle", m), pl.shuffle, tr("Aléatoire", "Shuffle"))
            .on_hover_text(tr("Enchaîner les morceaux au hasard", "Play tracks in random order"))
            .clicked()
        {
            self.ctrl.update_playlist(m, |c| c.shuffle = !c.shuffle);
        }
        if mode_chip(ui, loop_r, ("pl-loop", m), pl.looping, tr("Tout boucler", "Loop all"))
            .on_hover_text(tr("Reprendre au début à la fin de la liste", "Start over at the end of the list"))
            .clicked()
        {
            self.ctrl.update_playlist(m, |c| c.looping = !c.looping);
        }

        // Transition, toujours visible.
        {
            // Retour à la ligne si le module est étroit (sinon le champ déborde sous le fader).
            ui.horizontal_wrapped(|ui| {
                let mut p2 = pl.clone();
                ui.label(RichText::new("Transition").color(TEXT_DIM));
                let fade = p2.transition == Transition::Crossfade;
                let secs_w = if fade { 52.0 } else { 0.0 };
                let seg_w = (area.width() - secs_w - if fade { 6.0 } else { 0.0 }).max(80.0);
                let (seg, _) = ui.allocate_exact_size(vec2(seg_w, 22.0), egui::Sense::hover());
                let labels = [tr("Fin de fichier", "End of file"), tr("Fondu", "Crossfade")];
                if let Some(i) = segmented(ui, seg, ("pl-transition", m), &labels, fade as usize) {
                    p2.transition = if i == 1 { Transition::Crossfade } else { Transition::Gapless };
                }
                if fade {
                    ui.add_sized(
                        vec2(secs_w, 22.0),
                        egui::DragValue::new(&mut p2.crossfade_secs).range(0.5..=20.0).speed(0.1).suffix(" s").max_decimals(1),
                    );
                }
                if (p2.transition, p2.crossfade_secs) != (pl.transition, pl.crossfade_secs) {
                    self.ctrl.update_playlist(m, |c| *c = p2);
                }
            });
        }

        // Équilibrage du niveau des morceaux.
        // Toujours visible : la case suffit pour l'activer, le réglage fin se fait en mode édition.
        self.normalize_controls(ui, m, editing);

        // Liste des morceaux, avec le gain d'équilibrage de chacun.
        // Largeur fixée sur celle du module : le gain de chaque morceau reste visible à droite.
        let row_w = (area.right() - ui.cursor().left()).max(40.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_width(row_w).show(ui, |ui| {
            ui.set_max_width(row_w);
            if pl.tracks.is_empty() {
                ui.label(RichText::new(tr(
                    "Ajoutez des fichiers ou un dossier avec « ＋ Ajouter », ou glissez-les ici.",
                    "Add files or a folder with “＋ Add”, or drop them here.",
                )).color(TEXT_DIM));
            }
            ui.spacing_mut().item_spacing.y = 1.0;
            let mut remove = None;
            for (i, path) in pl.tracks.iter().enumerate() {
                let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let is_cur = rt.current == Some(i);
                let color = if !is_cur { TEXT } else if rt.playing && !rt.paused { GO } else { ACCENT };
                let tg = self.ctrl.track_gain(m, i);
                let manual = matches!(tg, TrackGain::Manual(_));
                let (gtxt, gcol, gtip, gdb) = gain_label(&tg);
                let (resp, anchor) = track_row(ui, &format!("{:>3}  {}", i + 1, name), color, is_cur, Some(&gtxt));
                // Gain du morceau : double-clic pour le régler à la main.
                let gid = ui.make_persistent_id(("track_gain", m, i));
                if let Some(db) = db_value_shown(ui, gid, anchor, egui::Align2::RIGHT_CENTER, gdb, -60.0, -60.0..=20.0, FontId::monospace(11.0), gcol, gtxt, Some(gtip)) {
                    self.ctrl.set_track_gain(m, i, Some(db));
                }
                if resp.double_clicked() {
                    self.ctrl.playlist_play(m, i);
                } else if resp.clicked() {
                    self.ctrl.select_track(m, i);
                }
                resp.dnd_set_drag_payload(TrackPayload(path.clone()));
                resp.context_menu(|ui| {
                    if ui.button(tr("Lire", "Play")).clicked() {
                        self.ctrl.playlist_play(m, i);
                        ui.close();
                    }
                    if manual && ui.button(tr("Revenir au gain automatique", "Back to automatic gain")).clicked() {
                        self.ctrl.set_track_gain(m, i, None);
                        ui.close();
                    }
                    if ui.button(tr("Retirer de la liste", "Remove from list")).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            }
            if let Some(i) = remove {
                self.ctrl.remove_track(m, i);
            }
        });
    }

    /// Réglages d'équilibrage : complets en mode édition, résumé sinon.
    fn normalize_controls(&mut self, ui: &mut egui::Ui, m: usize, editing: bool) {
        let n = self.ctrl.project.modules[m].playlist.normalize.clone();
        let pending = self.ctrl.pending_analyses(m);
        let total = self.ctrl.project.modules[m].playlist.tracks.len();
        let progress = (pending > 0).then(|| format!("{} {}/{}", tr("mesure", "measuring"), total - pending, total));
        if !editing {
            ui.horizontal_wrapped(|ui| {
                let mut on = n.enabled;
                if ui.checkbox(&mut on, tr("Équilibrer le niveau", "Normalize level")).on_hover_text(tr(
                    "Met tous les morceaux à un niveau semblable (cible et mode réglables en mode édition ✏). Les fichiers ne sont pas modifiés.",
                    "Brings all tracks to a similar level (target and mode in edit mode ✏). Files are not modified.",
                )).changed() {
                    self.ctrl.update_playlist(m, |c| c.normalize.enabled = on);
                }
                if !n.enabled {
                    return;
                }
                let mut txt = format!("{:.1} {}", n.target(), n.mode.unit());
                if n.limit && n.mode != NormalizeMode::TruePeak {
                    txt += &format!(" · max {:.1} dBTP", n.ceiling);
                }
                ui.label(RichText::new(txt).monospace().color(ACCENT));
                if let Some(p) = &progress {
                    ui.spinner();
                    ui.label(RichText::new(p).small().color(TEXT_DIM));
                }
            });
            return;
        }
        let mut n2 = n.clone();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut n2.enabled, tr("Équilibrer le niveau", "Normalize level")).on_hover_text(tr(
                "Mesure chaque morceau et ajuste son gain de lecture pour atteindre la cible. Les fichiers ne sont pas modifiés.",
                "Measures each track and adjusts its playback gain to reach the target. Files are not modified.",
            ));
            if !n2.enabled {
                return;
            }
            let label = |md: NormalizeMode| match md {
                NormalizeMode::Lufs => tr("Sonie (LUFS)", "Loudness (LUFS)"),
                NormalizeMode::Peak => tr("Crête", "Peak"),
                NormalizeMode::TruePeak => tr("Crête vraie", "True peak"),
                NormalizeMode::Rms => "RMS",
            };
            egui::ComboBox::from_id_salt(("norm-mode", m)).selected_text(label(n2.mode)).width(110.0).show_ui(ui, |ui| {
                for md in NormalizeMode::ALL {
                    ui.selectable_value(&mut n2.mode, md, label(md));
                }
            });
            let unit = format!(" {}", n2.mode.unit());
            let range = if n2.mode == NormalizeMode::Lufs { -40.0..=-5.0 } else { -40.0..=0.0 };
            ui.add(egui::DragValue::new(n2.target_mut()).range(range).speed(0.1).max_decimals(1).suffix(unit))
                .on_hover_text(tr("Cible (EBU R128 : -23 LUFS, streaming : -14 à -16 LUFS)", "Target (EBU R128: -23 LUFS, streaming: -14 to -16 LUFS)"));
            if n2.mode != NormalizeMode::TruePeak {
                ui.checkbox(&mut n2.limit, tr("Max", "Max")).on_hover_text(tr(
                    "Réduit le gain des morceaux dont la crête vraie dépasserait ce plafond, pour ne jamais saturer.",
                    "Lowers the gain of tracks whose true peak would exceed this ceiling, so nothing clips.",
                ));
                if n2.limit {
                    ui.add(egui::DragValue::new(&mut n2.ceiling).range(-20.0..=0.0).speed(0.1).max_decimals(1).suffix(" dBTP"));
                }
            }
            if let Some(p) = &progress {
                ui.spinner();
                ui.label(RichText::new(p).small().color(TEXT_DIM));
            }
        });
        if n2 != n {
            self.ctrl.update_playlist(m, |c| c.normalize = n2);
        }
    }

    // ------------------------------------------------------------ préférences

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.settings.as_mut() else { return };
        let mut open = true;
        let mut apply = false;
        let mut close = false;
        let mut refresh = false;
        egui::Window::new(tr("Préférences", "Preferences"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.heading("Audio");
                egui::Grid::new("audio-settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(tr("Pilote", "Driver"));
                    let host_label = draft.audio.host.clone().unwrap_or_else(|| tr("Par défaut", "Default").into());
                    egui::ComboBox::from_id_salt("host").selected_text(host_label).width(260.0).show_ui(ui, |ui| {
                        if ui.selectable_label(draft.audio.host.is_none(), tr("Par défaut", "Default")).clicked() {
                            draft.audio.host = None;
                            refresh = true;
                        }
                        for h in &draft.hosts {
                            if ui.selectable_label(draft.audio.host.as_deref() == Some(h), h).clicked() {
                                draft.audio.host = Some(h.clone());
                                draft.audio.device = None;
                                refresh = true;
                            }
                        }
                    });
                    ui.end_row();

                    ui.label(tr("Périphérique", "Device"));
                    let dev_label = draft.audio.device.clone().unwrap_or_else(|| tr("Sortie par défaut", "Default output").into());
                    egui::ComboBox::from_id_salt("device").selected_text(dev_label).width(260.0).show_ui(ui, |ui| {
                        if ui.selectable_label(draft.audio.device.is_none(), tr("Sortie par défaut", "Default output")).clicked() {
                            draft.audio.device = None;
                        }
                        for d in &draft.devices {
                            let label = format!("{}  ({} {}){}", d.name, d.channels, tr("canaux", "channels"), if d.is_default { " ★" } else { "" });
                            if ui.selectable_label(draft.audio.device.as_deref() == Some(&d.name), label).clicked() {
                                draft.audio.device = Some(d.name.clone());
                            }
                        }
                    });
                    ui.end_row();

                    ui.label(tr("Taille de buffer", "Buffer size"));
                    let buf_label = draft.audio.buffer_size.map(|b| format!("{b} {}", tr("échantillons", "samples"))).unwrap_or_else(|| tr("Automatique", "Automatic").into());
                    egui::ComboBox::from_id_salt("buffer").selected_text(buf_label).width(260.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.audio.buffer_size, None, tr("Automatique", "Automatic"));
                        for b in [64u32, 128, 256, 512, 1024, 2048] {
                            ui.selectable_value(&mut draft.audio.buffer_size, Some(b), format!("{b} {}", tr("échantillons", "samples")));
                        }
                    });
                    ui.end_row();
                });
                if ui.small_button(tr("Rafraîchir la liste", "Refresh list")).clicked() {
                    refresh = true;
                }
                ui.add_space(4.0);
                ui.label(RichText::new(tr(
                    "Chaque module se route vers une paire de sorties (1-2, 3-4…). \
                     Avec une carte multicanal, choisissez des paires différentes pour le multipiste.",
                    "Each module is routed to an output pair (1-2, 3-4…). \
                     With a multichannel interface, pick different pairs for multitrack playback.",
                )).small().color(TEXT_DIM));

                ui.separator();
                ui.heading(tr("Système", "System"));
                let mut lang = self.ctrl.project.preferences.language;
                ui.horizontal(|ui| {
                    ui.label(tr("Langue", "Language"));
                    ui.selectable_value(&mut lang, Lang::Fr, "Français");
                    ui.selectable_value(&mut lang, Lang::En, "English");
                });
                if lang != self.ctrl.project.preferences.language {
                    self.ctrl.set_language(lang);
                }
                let mut esc = self.ctrl.project.preferences.escape_stops_playlist;
                if ui
                    .checkbox(&mut esc, tr("Échap arrête aussi les playlists", "Esc also stops playlists"))
                    .on_hover_text(tr("Sinon Échap n'arrête que les jingles", "Otherwise Esc only stops jingles"))
                    .changed()
                {
                    self.ctrl.set_escape_stops_playlist(esc);
                }
                ui.horizontal(|ui| {
                    let mut ms = self.ctrl.project.preferences.double_go_ms;
                    ui.label(tr("Anti double-GO", "Double-GO protection"));
                    if ui
                        .add(egui::DragValue::new(&mut ms).range(0..=2000).speed(10).suffix(" ms"))
                        .on_hover_text(tr(
                            "Un second clic sur le même jingle dans ce délai est ignoré (0 = désactivé)",
                            "A second click on the same jingle within this delay is ignored (0 = off)",
                        ))
                        .changed()
                    {
                        self.ctrl.set_double_go_ms(ms);
                    }
                });
                let mut ps = self.ctrl.project.preferences.prevent_sleep;
                if ui.checkbox(&mut ps, tr("Empêcher la mise en veille pendant la lecture", "Prevent sleep during playback")).changed() {
                    self.ctrl.set_prevent_sleep(ps);
                }


                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("Appliquer", "Apply")).on_hover_text(tr("Applique les réglages et ferme la fenêtre", "Applies the settings and closes the window")).clicked() {
                        apply = true;
                        close = true;
                    }
                    if ui.button(tr("Fermer", "Close")).clicked() {
                        close = true;
                    }
                
                });
                // Version de build, pour savoir quelle mise à jour est installée.
                let v = build_version();
                if ui
                    .add(egui::Label::new(RichText::new(&v).monospace().size(10.0).color(TEXT_DIM)).sense(egui::Sense::click()))
                    .on_hover_text(tr("Version de build (clic pour copier)", "Build version (click to copy)"))
                    .clicked()
                {
                    ui.ctx().copy_text(v);
                }
            });
        if refresh {
            draft.devices = list_devices(draft.audio.host.as_deref());
            draft.hosts = host_names();
        }
        if apply {
            // Ne rouvre le périphérique (et n'interrompt la lecture) que si l'audio a changé.
            let a = draft.audio.clone();
            if a != self.ctrl.project.audio || self.ctrl.device.is_none() {
                self.ctrl.set_audio_settings(a);
            }
        }
        if !open || close {
            self.settings = None;
        }
    }

    // ---------------------------------------------------------- glisser-déposer

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        let pos = self.drag_pos;
        if files.is_empty() {
            return;
        }
        let target = pos
            .and_then(|p| self.drop_targets.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t))
            .or_else(|| self.selected.map(|(m, s)| DropTarget::Jingle(m, s)));
        match target {
            Some(DropTarget::Jingle(m, s)) => {
                // Plusieurs fichiers : on remplit les emplacements suivants.
                let audio: Vec<_> = files.into_iter().filter(|p| crate::audio::decode::is_audio_file(p)).collect();
                for (k, p) in audio.into_iter().enumerate() {
                    if s + k < JINGLE_SLOTS {
                        self.ctrl.assign_jingle(m, s + k, p);
                    }
                }
            }
            Some(DropTarget::Go(m)) => self.ctrl.add_cues(m, files, None),
            Some(DropTarget::Playlist(m)) => {
                // Fichiers de plusieurs dossiers ou dossiers entiers : ajoutés à la suite de la liste.
                self.ctrl.add_playlist_files(m, files);
            }
            None => {}
        }
    }
}

/// Texte, couleur et infobulle du gain d'équilibrage d'un morceau.
/// Gain d'un morceau pour l'affichage : texte, couleur, bulle d'aide, et valeur de départ d'une saisie.
fn gain_label(g: &TrackGain) -> (String, Color32, String, f32) {
    let details = |l: &Loudness| {
        let lufs = l.integrated.map(|v| format!("{v:.1} LUFS")).unwrap_or_else(|| "—".into());
        format!(
            "{} : {lufs}\n{} : {:.1} dBFS\n{} : {:.1} dBTP\nRMS : {:.1} dBFS",
            tr("Sonie intégrée", "Integrated loudness"),
            tr("Crête", "Peak"),
            l.peak,
            tr("Crête vraie", "True peak"),
            l.true_peak,
            l.rms,
        )
    };
    match g {
        TrackGain::Off => (fmt_db(0.0, -60.0), TEXT_DIM.gamma_multiply(0.6), tr("Aucun gain appliqué", "No gain applied").into(), 0.0),
        TrackGain::Manual(db) => (
            fmt_db(*db, -60.0),
            EDIT,
            tr("Gain réglé à la main (clic droit : revenir au gain automatique)", "Manual gain (right-click: back to automatic gain)").into(),
            *db,
        ),
        TrackGain::Pending => ("…".into(), TEXT_DIM, tr("Mesure en cours", "Measuring").into(), 0.0),
        TrackGain::Failed(e) => ("⚠".into(), STOP, format!("{} : {e}", tr("Mesure impossible", "Cannot measure")), 0.0),
        TrackGain::Unmeasurable(l) => ("—".into(), TEXT_DIM, format!("{}\n{}", tr("Son muet ou trop court : gain inchangé", "Silent or too short: gain unchanged"), details(l)), 0.0),
        TrackGain::Ready { db, limited, loudness } => {
            let mut tip = format!("{} : {}\n{}", tr("Gain appliqué", "Applied gain"), fmt_db(*db, -60.0), details(loudness));
            if *limited {
                tip += &format!("\n{}", tr("Gain réduit pour ne pas dépasser le plafond de crête vraie", "Gain reduced to stay under the true peak ceiling"));
            }
            (fmt_db(*db, -60.0), if *limited { ACCENT } else { TEXT_DIM }, tip, *db)
        }
    }
}

/// Position du curseur quand des fichiers sont survolés ou déposés.
/// Sous Windows, winit ne transmet pas le mouvement de la souris pendant un glisser-déposer :
/// on lit directement la position du curseur système.
fn file_drag_pos(ctx: &egui::Context, frame: &eframe::Frame) -> Option<egui::Pos2> {
    let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty() || !i.raw.dropped_files.is_empty());
    if !dragging {
        return None;
    }
    os_cursor_pos(ctx, frame).or_else(|| ctx.input(|i| i.pointer.latest_pos()))
}

#[cfg(windows)]
fn os_cursor_pos(ctx: &egui::Context, frame: &eframe::Frame) -> Option<egui::Pos2> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let RawWindowHandle::Win32(h) = frame.window_handle().ok()?.as_raw() else { return None };
    let mut pt = POINT { x: 0, y: 0 };
    // SAFETY : appels Win32 simples sur la fenêtre de l'application et une structure locale.
    unsafe {
        if GetCursorPos(&mut pt) == 0 || ScreenToClient(h.hwnd.get() as _, &mut pt) == 0 {
            return None;
        }
    }
    let ppp = ctx.pixels_per_point();
    Some(egui::pos2(pt.x as f32 / ppp, pt.y as f32 / ppp))
}

#[cfg(not(windows))]
fn os_cursor_pos(_ctx: &egui::Context, _frame: &eframe::Frame) -> Option<egui::Pos2> {
    None
}

/// Message à la place de la forme d'onde d'un son pas encore chargé (ou illisible).
/// « 1,2 Go », « 350 Mo ».
fn fmt_bytes(b: usize) -> String {
    let mb = b as f64 / 1_000_000.0;
    let s = if mb >= 1000.0 { format!("{:.1} {}", mb / 1000.0, tr("Go", "GB")) } else { format!("{mb:.0} {}", tr("Mo", "MB")) };
    if crate::i18n::is_en() { s } else { s.replace('.', ",") }
}

pub(super) fn load_status(ctrl: &Controller, path: &std::path::Path) -> Option<RichText> {
    if ctrl.is_loading(path) {
        Some(RichText::new(tr("Chargement de la forme d'onde…", "Loading waveform…")))
    } else {
        ctrl.load_errors.get(path).map(|e| RichText::new(format!("⚠ {e}")).color(STOP))
    }
}

/// « JingleStar 0.1.0 · a1b2c3d · 2026-10-24 » : version du Cargo.toml, commit et date de compilation.
pub fn build_version() -> String {
    format!(
        "JingleStar {} · {} · {}",
        env!("CARGO_PKG_VERSION"),
        env!("JINGLESTAR_GIT"),
        env!("JINGLESTAR_BUILD_DATE")
    )
}

impl eframe::App for JingleStarApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drag_pos = file_drag_pos(&ctx, frame);
        self.ctrl.poll();
        self.update_meters();
        self.drop_targets.clear();

        // Dans un module Texte, Échap sort du champ et arrête quand même les jingles.
        let typing = ctx.egui_wants_keyboard_input() && !std::mem::take(&mut self.notes_focused);
        if !typing && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.ctrl.escape();
        }
        // Barre d'espace : GO du premier module GO. La touche est retirée de la file
        // pour qu'un bouton qui a le focus ne la reçoive pas en plus.
        if !typing {
            if let Some(m) = self.ctrl.first_go_module() {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Space)) {
                    self.ctrl.cue_go(m);
                }
            }
        }
        if ctx.input(|i| i.key_pressed(Key::F1)) {
            self.help.toggle();
        }

        egui::Panel::top("toolbar")
            .exact_size(48.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 6)))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(10))
            .show(ui, |ui| self.modules_grid(ui));

        // Étiquette qui suit le pointeur pendant le glisser d'un morceau.
        let dragged = egui::DragAndDrop::payload::<TrackPayload>(&ctx)
            .map(|t| format!("♪ {}", t.0.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
            .or_else(|| egui::DragAndDrop::payload::<go_view::CuePayload>(&ctx).map(|c| format!("{}  {}", c.index + 1, c.name)));
        if let (Some(name), Some(p)) = (dragged, ctx.pointer_interact_pos()) {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("track-drag")));
            let galley = painter.layout_no_wrap(name, FontId::proportional(13.0), Color32::WHITE);
            let r = Rect::from_min_size(p + vec2(12.0, 8.0), galley.size() + vec2(12.0, 6.0));
            painter.rect_filled(r, CornerRadius::same(4), EDIT.gamma_multiply(0.9));
            painter.galley(r.min + vec2(6.0, 3.0), galley, Color32::WHITE);
        }
        self.jingle_editor(&ctx);
        self.cue_editor_window(&ctx);
        self.module_eq_window(&ctx);
        self.settings_window(&ctx);
        self.help.show(&ctx);
        self.remote.show(&ctx, &mut self.ctrl);
        self.handle_drops(&ctx);

        let busy = self.ctrl.anything_playing() || self.meters.iter().flatten().any(|d| *d > -100.0);
        ctx.request_repaint_after(Duration::from_millis(if busy { 33 } else { 250 }));
    }

    fn on_exit(&mut self) {
        self.ctrl.stop_all();
        self.ctrl.flush();
    }
}

fn mode_label(mode: ModuleMode) -> &'static str {
    match mode {
        ModuleMode::Jingle => "Jingles",
        ModuleMode::Playlist => "Playlist",
        ModuleMode::Go => "Cuelist",
        ModuleMode::Text => tr("Texte", "Text"),
    }
}
