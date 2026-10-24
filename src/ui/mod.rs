//! Interface principale.

mod theme;
mod widgets;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{vec2, Align, Color32, CornerRadius, FontId, Key, Layout, Rect, RichText, Stroke, UiBuilder};

use crate::audio::engine::{host_names, list_devices, DeviceDesc};
use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use crate::controller::Controller;
use crate::project::{AudioSettings, ModuleMode, Transition, DEFAULT_COLORS};
use theme::*;
use widgets::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum DropTarget {
    Jingle(usize, usize),
    Playlist(usize),
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
    settings: Option<SettingsDraft>,
    /// Niveaux affichés (dB), avec retombée progressive.
    meters: [[f32; 2]; MAX_MODULES],
    last_frame: Instant,
    drop_targets: Vec<(Rect, DropTarget)>,
}

impl JingleStarApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        Self {
            ctrl: Controller::new(move || ctx.request_repaint()),
            edit_mode: [false; MAX_MODULES],
            selected: None,
            settings: None,
            meters: [[-120.0; 2]; MAX_MODULES],
            last_frame: Instant::now(),
            drop_targets: Vec::new(),
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
            ui.label(RichText::new("JingleStar").font(FontId::proportional(20.0)).strong().color(Color32::WHITE));
            ui.add_space(16.0);
            ui.label(RichText::new("Modules").color(TEXT_DIM));
            for n in 1..=MAX_MODULES {
                let sel = self.ctrl.project.module_count == n;
                if ui.selectable_label(sel, format!("{n}")).clicked() {
                    self.ctrl.set_module_count(n);
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let panic = egui::Button::new(RichText::new("■  TOUT ARRÊTER").strong().color(Color32::WHITE))
                    .fill(STOP)
                    .min_size(vec2(150.0, 32.0));
                if ui.add(panic).on_hover_text("Échap").clicked() {
                    self.ctrl.stop_all();
                }
                ui.add_space(8.0);
                if ui.button("⚙ Préférences").clicked() {
                    self.open_settings();
                }
                if ui.button("Enregistrer sous…").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Projet JingleStar", &["jinglestar"])
                        .set_file_name("projet.jinglestar")
                        .save_file()
                    {
                        self.ctrl.save_project_as(p);
                    }
                }
                if ui.button("Ouvrir…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Projet JingleStar", &["jinglestar", "json"]).pick_file() {
                        self.selected = None;
                        self.ctrl.open_project(p);
                    }
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            match &self.ctrl.device {
                Some(d) => {
                    ui.label(RichText::new("●").color(GO));
                    ui.label(format!("{} · {} · {} canaux · {} Hz", d.host, d.name, d.channels, d.sample_rate));
                }
                None => {
                    ui.label(RichText::new("●").color(STOP));
                    ui.label("Aucune sortie audio");
                }
            }
            if let Some(e) = &self.ctrl.device_error {
                ui.label(RichText::new(format!("⚠ {e}")).color(STOP));
            }
            if let Some(s) = &self.ctrl.status {
                ui.label(RichText::new(s).color(TEXT_DIM));
            }
        });
    }

    // ---------------------------------------------------------------- modules

    fn modules_grid(&mut self, ui: &mut egui::Ui) {
        let n = self.ctrl.project.module_count;
        let cols = n.min(4);
        let rows = n.div_ceil(4);
        let area = ui.available_rect_before_wrap();
        let gap = 8.0;
        let cw = (area.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let rh = (area.height() - gap * (rows as f32 - 1.0)) / rows as f32;
        for m in 0..n {
            let (r, c) = (m / 4, m % 4);
            let rect = Rect::from_min_size(
                area.min + vec2(c as f32 * (cw + gap), r as f32 * (rh + gap)),
                vec2(cw, rh),
            );
            ui.scope_builder(UiBuilder::new().max_rect(rect).id_salt(("module", m)), |ui| {
                self.module_panel(ui, m, rect);
            });
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
            let strip_w = 46.0;
            let content = Rect::from_min_max(body.min, egui::pos2(body.right() - strip_w - 6.0, body.bottom()));
            let strip = Rect::from_min_max(egui::pos2(body.right() - strip_w, body.top()), body.max);
            ui.scope_builder(UiBuilder::new().max_rect(content), |ui| match self.ctrl.project.modules[m].mode {
                ModuleMode::Jingle => self.jingle_grid(ui, m, content),
                ModuleMode::Playlist => self.playlist_view(ui, m, content),
            });
            ui.scope_builder(UiBuilder::new().max_rect(strip), |ui| self.fader_strip(ui, m, strip));
            ui.allocate_rect(body, egui::Sense::hover());
        });
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
            } else {
                ui.label(RichText::new(&self.ctrl.project.modules[m].name).strong());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let edit = ui.selectable_label(editing, "✏").on_hover_text("Mode édition");
                if edit.clicked() {
                    self.edit_mode[m] = !editing;
                    if editing && self.selected.map(|(sm, _)| sm == m).unwrap_or(false) {
                        self.selected = None;
                    }
                }
                self.output_combo(ui, m);
            });
        });
        ui.horizontal(|ui| {
            let mode = self.ctrl.project.modules[m].mode;
            if ui.selectable_label(mode == ModuleMode::Jingle, "Jingles").clicked() {
                self.ctrl.set_module_mode(m, ModuleMode::Jingle);
            }
            if ui.selectable_label(mode == ModuleMode::Playlist, "Playlist").clicked() {
                self.ctrl.set_module_mode(m, ModuleMode::Playlist);
            }
        });
    }

    fn output_combo(&mut self, ui: &mut egui::Ui, m: usize) {
        let pairs = self.ctrl.output_pairs();
        let cur = self.ctrl.project.modules[m].output_pair;
        let label = |p: usize| format!("Sortie {}-{}", p * 2 + 1, p * 2 + 2);
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
        if resp.double_clicked() {
            db = 0.0;
        }
        if db != self.ctrl.project.modules[m].volume_db {
            self.ctrl.set_module_volume(m, db);
        }
        let txt = if db <= -60.0 { "-∞".to_string() } else { format!("{db:+.1}") };
        ui.painter().text(
            egui::pos2(strip.center().x, fader_area.bottom() + 4.0),
            egui::Align2::CENTER_TOP,
            txt,
            FontId::monospace(11.0),
            TEXT_DIM,
        );
        let stop_rect = Rect::from_min_max(egui::pos2(strip.left(), strip.bottom() - stop_h), strip.max);
        if ui.put(stop_rect, egui::Button::new(RichText::new("■").color(Color32::WHITE)).fill(STOP.gamma_multiply(0.6))).on_hover_text("Arrêter le module").clicked() {
            self.ctrl.stop_module(m);
        }
    }

    // ---------------------------------------------------------------- jingles

    fn jingle_grid(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        let (cols, rows) = (3usize, 4usize);
        let gap = 6.0;
        let w = (area.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = (area.height() - gap * (rows as f32 - 1.0)) / rows as f32;
        let hover_pos = ui.ctx().input(|i| (!i.raw.hovered_files.is_empty()).then(|| i.pointer.hover_pos()).flatten());
        for s in 0..JINGLE_SLOTS {
            let (r, c) = (s / cols, s % cols);
            let rect = Rect::from_min_size(area.min + vec2(c as f32 * (w + gap), r as f32 * (h + gap)), vec2(w, h));
            self.drop_targets.push((rect, DropTarget::Jingle(m, s)));
            let slot = self.ctrl.jingle(m, s).cloned();
            let pos = self.ctrl.engine.telemetry.position(m, s);
            let rate = self.ctrl.device.as_ref().map(|d| d.sample_rate).unwrap_or(48_000) as f64;
            let (progress, time_label) = match (&slot, pos) {
                (Some(j), Some((p, len))) => {
                    let dur = len as f64 / rate;
                    let (_, _, _, end) = j.markers(dur);
                    let t = p as f64 / rate;
                    let label = if j.looping { format!("⟳ {}", fmt_time(t)) } else { format!("-{}", fmt_time(end - t)) };
                    (Some((t / end.max(1e-6)) as f32), Some(label))
                }
                (Some(j), None) => {
                    let label = self.ctrl.duration_of(&j.path).map(|d| {
                        let (st, _, _, end) = j.markers(d);
                        fmt_time(end - st)
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
                progress,
                time_label,
                loading: slot.as_ref().map(|j| self.ctrl.is_loading(&j.path)).unwrap_or(false),
                error: slot.as_ref().map(|j| self.ctrl.load_errors.contains_key(&j.path)).unwrap_or(false),
                looping: slot.as_ref().map(|j| j.looping).unwrap_or(false),
                selected: self.selected == Some((m, s)),
                edit_mode: self.edit_mode[m],
                drop_hover: hover_pos.map(|p| rect.contains(p)).unwrap_or(false),
            };
            let resp = jingle_button(ui, rect, &view);
            if resp.clicked() {
                if self.edit_mode[m] {
                    if slot.is_none() {
                        self.pick_jingle_file(m, s);
                    }
                    self.selected = Some((m, s));
                } else if slot.is_some() {
                    self.ctrl.trigger_jingle(m, s);
                }
            }
            resp.context_menu(|ui| {
                if ui.button("Choisir un fichier…").clicked() {
                    self.pick_jingle_file(m, s);
                    ui.close();
                }
                if slot.is_some() {
                    if ui.button("Éditer…").clicked() {
                        self.edit_mode[m] = true;
                        self.selected = Some((m, s));
                        ui.close();
                    }
                    if ui.button("Vider").clicked() {
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
        let mut open = true;
        egui::Window::new(format!("Jingle {}.{} — {}", m + 1, s + 1, j.name))
            .id(egui::Id::unique("jingle-editor"))
            .open(&mut open)
            .default_width(620.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                let mut j2 = j.clone();
                egui::Grid::new("jingle-fields").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Nom");
                    ui.text_edit_singleline(&mut j2.name);
                    ui.end_row();

                    ui.label("Fichier");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(j.path.display().to_string()).color(TEXT_DIM).small());
                        if ui.button("Changer…").clicked() {
                            self.pick_jingle_file(m, s);
                        }
                    });
                    ui.end_row();

                    ui.label("Volume");
                    ui.add(egui::Slider::new(&mut j2.volume_db, -40.0..=10.0).suffix(" dB").step_by(0.5));
                    ui.end_row();

                    ui.label("Lecture");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut j2.looping, false, "Une fois");
                        ui.radio_value(&mut j2.looping, true, "En boucle");
                    });
                    ui.end_row();

                    ui.label("Fondus");
                    ui.horizontal(|ui| {
                        ui.label("entrée");
                        ui.add(egui::DragValue::new(&mut j2.fade_in).range(0.0..=30.0).speed(0.05).suffix(" s").max_decimals(2));
                        ui.label("sortie");
                        ui.add(egui::DragValue::new(&mut j2.fade_out).range(0.0..=30.0).speed(0.05).suffix(" s").max_decimals(2));
                    });
                    ui.end_row();

                    ui.label("Couleur");
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
                if let Some(buf) = self.ctrl.buffer_of(&j.path) {
                    let dur = buf.duration_secs();
                    let (st, ls, le, end) = j2.markers(dur);
                    let mut mk = Markers { start: st, loop_start: ls, loop_end: le, end };
                    let rate = buf.sample_rate as f64;
                    let playhead = self.ctrl.engine.telemetry.position(m, s).map(|(p, _)| p as f64 / rate);
                    if waveform_editor(ui, &buf.overview, dur, &mut mk, j2.looping, j2.fade_in, j2.fade_out, playhead) {
                        j2.start = mk.start;
                        j2.end = Some(mk.end);
                        j2.loop_start = Some(mk.loop_start);
                        j2.loop_end = Some(mk.loop_end);
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!(
                            "Début {}  ·  Fin {}  ·  Durée {}",
                            fmt_secs(mk.start),
                            fmt_secs(mk.end),
                            fmt_secs(mk.end - mk.start)
                        )).monospace().color(TEXT_DIM));
                        if j2.looping {
                            ui.label(RichText::new(format!("Boucle {} → {}", fmt_secs(mk.loop_start), fmt_secs(mk.loop_end))).monospace().color(ACCENT));
                        }
                        if ui.small_button("Réinitialiser les points").clicked() {
                            j2.start = 0.0;
                            j2.end = None;
                            j2.loop_start = None;
                            j2.loop_end = None;
                        }
                    });
                } else if self.ctrl.is_loading(&j.path) {
                    ui.label("Chargement de la forme d'onde…");
                } else if let Some(e) = self.ctrl.load_errors.get(&j.path) {
                    ui.label(RichText::new(format!("⚠ {e}")).color(STOP));
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let playing = self.ctrl.is_jingle_playing(m, s);
                    let (label, col) = if playing { ("■ Stop", STOP) } else { ("▶ Écouter", GO) };
                    if ui.add(egui::Button::new(RichText::new(label).color(Color32::BLACK).strong()).fill(col)).clicked() {
                        self.ctrl.trigger_jingle(m, s);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(RichText::new("Vider l'emplacement").color(STOP)).clicked() {
                            self.ctrl.clear_jingle(m, s);
                            self.selected = None;
                        }
                    });
                });

                if format!("{j2:?}") != format!("{j:?}") {
                    self.ctrl.update_jingle(m, s, |slot| *slot = j2);
                }
            });
        if !open {
            self.selected = None;
        }
    }

    // --------------------------------------------------------------- playlist

    fn playlist_view(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        self.drop_targets.push((area, DropTarget::Playlist(m)));
        let editing = self.edit_mode[m];
        let pl = self.ctrl.project.modules[m].playlist.clone();
        let rt = self.ctrl.playlists[m].clone();

        ui.horizontal(|ui| {
            if ui.button("📁 Dossier…").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.ctrl.set_playlist_folder(m, dir);
                }
            }
            if pl.folder.is_some() && ui.button("⟳").on_hover_text("Relire le dossier").clicked() {
                self.ctrl.rescan_playlist(m);
            }
            let name = pl
                .folder
                .as_ref()
                .and_then(|f| f.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Aucun dossier".into());
            ui.label(RichText::new(name).color(TEXT_DIM));
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
            ui.set_width(ui.available_width());
            let col = if rt.playing { GO } else { TEXT };
            ui.label(RichText::new(&current_name).strong().size(15.0).color(col));
            let (t, len) = pos.map(|(p, l)| (p as f64 / rate, l as f64 / rate)).unwrap_or((0.0, 0.0));
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), egui::Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(3), PANEL_HI);
            if len > 0.0 {
                let w = r.width() * (t / len).clamp(0.0, 1.0) as f32;
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), CornerRadius::same(3), GO);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(fmt_time(t)).monospace().color(TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("-{}", fmt_time((len - t).max(0.0)))).monospace().color(TEXT_DIM));
                });
            });
        });

        // Transport.
        ui.horizontal(|ui| {
            if ui.button("⏮").clicked() {
                self.ctrl.playlist_step(m, -1);
            }
            let go = egui::Button::new(RichText::new("▶").color(Color32::BLACK).strong()).fill(GO).min_size(vec2(44.0, 0.0));
            if ui.add(go).clicked() {
                self.ctrl.playlist_go(m);
            }
            if ui.add(egui::Button::new(RichText::new("■").color(Color32::WHITE)).fill(STOP.gamma_multiply(0.7))).clicked() {
                self.ctrl.playlist_stop(m);
            }
            if ui.button("⏭").clicked() {
                self.ctrl.playlist_step(m, 1);
            }
            ui.separator();
            let mut p2 = pl.clone();
            ui.toggle_value(&mut p2.shuffle, "🔀").on_hover_text("Lecture aléatoire");
            ui.toggle_value(&mut p2.looping, "🔁").on_hover_text("En boucle");
            ui.toggle_value(&mut p2.auto_advance, "⏩").on_hover_text("Enchaîner les morceaux");
            if (p2.shuffle, p2.looping, p2.auto_advance) != (pl.shuffle, pl.looping, pl.auto_advance) {
                self.ctrl.update_playlist(m, |c| *c = p2);
            }
        });

        // Transition.
        if editing || pl.transition == Transition::Crossfade {
            ui.horizontal(|ui| {
                let mut p2 = pl.clone();
                ui.label(RichText::new("Transition").color(TEXT_DIM));
                ui.selectable_value(&mut p2.transition, Transition::Gapless, "Fin de fichier");
                ui.selectable_value(&mut p2.transition, Transition::Crossfade, "Fondu enchaîné");
                if p2.transition == Transition::Crossfade {
                    ui.add(egui::DragValue::new(&mut p2.crossfade_secs).range(0.5..=20.0).speed(0.1).suffix(" s").max_decimals(1));
                }
                if (p2.transition, p2.crossfade_secs) != (pl.transition, pl.crossfade_secs) {
                    self.ctrl.update_playlist(m, |c| *c = p2);
                }
            });
        }

        // Liste des morceaux.
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if pl.tracks.is_empty() {
                ui.label(RichText::new("Choisissez un dossier ou glissez-le ici.").color(TEXT_DIM));
            }
            for (i, path) in pl.tracks.iter().enumerate() {
                let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let is_cur = rt.current == Some(i);
                let mut text = RichText::new(format!("{:>3}  {}", i + 1, name)).monospace();
                if is_cur {
                    text = text.color(if rt.playing { GO } else { ACCENT }).strong();
                }
                let resp = ui.add(egui::Button::selectable(is_cur, text).frame_when_inactive(false));
                if resp.double_clicked() {
                    self.ctrl.playlist_play(m, i);
                } else if resp.clicked() {
                    self.ctrl.select_track(m, i);
                }
            }
        });
    }

    // ------------------------------------------------------------ préférences

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.settings.as_mut() else { return };
        let mut open = true;
        let mut apply = false;
        let mut close = false;
        let mut refresh = false;
        egui::Window::new("Préférences")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.heading("Audio");
                egui::Grid::new("audio-settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Pilote");
                    let host_label = draft.audio.host.clone().unwrap_or_else(|| "Par défaut".into());
                    egui::ComboBox::from_id_salt("host").selected_text(host_label).width(260.0).show_ui(ui, |ui| {
                        if ui.selectable_label(draft.audio.host.is_none(), "Par défaut").clicked() {
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

                    ui.label("Périphérique");
                    let dev_label = draft.audio.device.clone().unwrap_or_else(|| "Sortie par défaut".into());
                    egui::ComboBox::from_id_salt("device").selected_text(dev_label).width(260.0).show_ui(ui, |ui| {
                        if ui.selectable_label(draft.audio.device.is_none(), "Sortie par défaut").clicked() {
                            draft.audio.device = None;
                        }
                        for d in &draft.devices {
                            let label = format!("{}  ({} canaux){}", d.name, d.channels, if d.is_default { " ★" } else { "" });
                            if ui.selectable_label(draft.audio.device.as_deref() == Some(&d.name), label).clicked() {
                                draft.audio.device = Some(d.name.clone());
                            }
                        }
                    });
                    ui.end_row();

                    ui.label("Taille de buffer");
                    let buf_label = draft.audio.buffer_size.map(|b| format!("{b} échantillons")).unwrap_or_else(|| "Automatique".into());
                    egui::ComboBox::from_id_salt("buffer").selected_text(buf_label).width(260.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.audio.buffer_size, None, "Automatique");
                        for b in [64u32, 128, 256, 512, 1024, 2048] {
                            ui.selectable_value(&mut draft.audio.buffer_size, Some(b), format!("{b} échantillons"));
                        }
                    });
                    ui.end_row();
                });
                if ui.small_button("Rafraîchir la liste").clicked() {
                    refresh = true;
                }
                ui.add_space(4.0);
                ui.label(RichText::new(
                    "Chaque module se route vers une paire de sorties (1-2, 3-4…). \
                     Avec une carte multicanal, choisissez des paires différentes pour le multipiste.",
                ).small().color(TEXT_DIM));

                ui.separator();
                ui.heading("Système");
                let mut ps = self.ctrl.project.preferences.prevent_sleep;
                if ui.checkbox(&mut ps, "Empêcher la mise en veille pendant la lecture").changed() {
                    self.ctrl.set_prevent_sleep(ps);
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Appliquer").clicked() {
                        apply = true;
                    }
                    if ui.button("Fermer").clicked() {
                        close = true;
                    }
                });
            });
        if refresh {
            draft.devices = list_devices(draft.audio.host.as_deref());
            draft.hosts = host_names();
        }
        if apply {
            let a = draft.audio.clone();
            self.ctrl.set_audio_settings(a);
        }
        if !open || close {
            self.settings = None;
        }
    }

    // ---------------------------------------------------------- glisser-déposer

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let (files, pos): (Vec<PathBuf>, _) = ctx.input(|i| {
            (i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect(), i.pointer.hover_pos())
        });
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
            Some(DropTarget::Playlist(m)) => {
                if let Some(dir) = files.iter().find(|p| p.is_dir()).cloned().or_else(|| files[0].parent().map(|p| p.to_path_buf())) {
                    self.ctrl.set_playlist_folder(m, dir);
                }
            }
            None => {}
        }
    }
}

fn fmt_secs(t: f64) -> String {
    let t = t.max(0.0);
    format!("{}:{:05.2}", (t / 60.0) as u64, t % 60.0)
}

impl eframe::App for JingleStarApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ctrl.poll();
        self.update_meters();
        self.drop_targets.clear();

        let typing = ctx.egui_wants_keyboard_input();
        if !typing && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.ctrl.stop_all();
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

        self.jingle_editor(&ctx);
        self.settings_window(&ctx);
        self.handle_drops(&ctx);

        let busy = self.ctrl.anything_playing() || self.meters.iter().flatten().any(|d| *d > -100.0);
        ctx.request_repaint_after(Duration::from_millis(if busy { 33 } else { 250 }));
    }

    fn on_exit(&mut self) {
        self.ctrl.stop_all();
        self.ctrl.flush();
    }
}
