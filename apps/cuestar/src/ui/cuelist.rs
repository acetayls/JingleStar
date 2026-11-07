//! Module Cuelist : la conduite, le cue en cours, le cue en attente et le gros bouton GO,
//! repris du module Cuelist de JingleStar. Un cue = un média (ou du noir) envoyé sur la
//! sortie, avec sa transition, sa boucle et la suite à donner (attendre le GO ou enchaîner).

use crate::link::OutputLink;
use crate::project::{CueData, CueFollow, CuelistData};
use crate::protocol::{Command, TransitionKind};
use star_core::i18n::tr;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Key, Rect, RichText, Sense, Stroke, StrokeKind};
use star_ui::theme::*;
use star_ui::widgets::{add_button, fmt_time, loop_icon, transport_button, TransportIcon};
use std::path::{Path, PathBuf};

const ROW_H: f32 = 26.0;

const EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "mxf", "mpg", "mpeg", "ts", "wmv", "jpg", "jpeg", "png", "bmp", "gif", "tif",
    "tiff", "webp", "mp3", "wav", "flac", "aac", "m4a", "ogg",
];

/// Cue en cours de glisser pour changer l'ordre de la conduite.
struct CuePayload {
    index: usize,
    name: String,
}

#[derive(Default)]
pub struct Cuelist {
    cues: Vec<CueData>,
    standby: usize,
    /// Cue envoyé à la sortie en dernier.
    running: Option<usize>,
    seen_end: u64,
    editor: Option<usize>,
    pending_play: Option<usize>,
    pending_go: bool,
    /// Aperçu muet de l'éditeur de cue (processus lancé à la première ouverture).
    preview: crate::preview::Preview,
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Gros bouton GO, éteint quand la conduite est terminée.
fn go_button(ui: &mut egui::Ui, rect: Rect, id: Id, enabled: bool) -> egui::Response {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter_at(rect.expand(1.0));
    let r = CornerRadius::same(6);
    let fill = if !enabled {
        PANEL_HI
    } else if resp.is_pointer_button_down_on() {
        GO.gamma_multiply(0.8)
    } else if resp.hovered() {
        GO.gamma_multiply(1.15)
    } else {
        GO
    };
    p.rect_filled(rect, r, fill);
    p.rect_stroke(rect, r, Stroke::new(1.0, Color32::from_black_alpha(110)), StrokeKind::Inside);
    let fg = if enabled { Color32::from_rgb(0x0c, 0x1a, 0x0e) } else { TEXT_DIM };
    let size = (rect.height() * 0.5).clamp(16.0, 34.0);
    p.text(rect.center(), Align2::CENTER_CENTER, "GO", FontId::proportional(size), fg);
    p.text(rect.right_bottom() + vec2(-8.0, -5.0), Align2::RIGHT_BOTTOM, "Espace", FontId::proportional(10.5), fg.gamma_multiply(0.7));
    resp
}

/// Flèche noire sur pastille orange (enchaîner), ou barre grise (arrêter et attendre le GO).
fn follow_icon(p: &egui::Painter, r: Rect, chained: bool, hovered: bool) {
    let c = r.center();
    if chained {
        p.rect_filled(r, CornerRadius::same(3), ACCENT);
        let ink = Color32::BLACK;
        p.line_segment([pos2(c.x, r.top() + 3.5), pos2(c.x, r.bottom() - 7.0)], Stroke::new(2.5, ink));
        p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 5.0, r.bottom() - 8.0), pos2(c.x + 5.0, r.bottom() - 8.0), pos2(c.x, r.bottom() - 3.0)], ink, Stroke::NONE));
    } else {
        let col = if hovered { TEXT } else { TEXT_DIM };
        p.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, col.gamma_multiply(0.6)), StrokeKind::Inside);
        p.line_segment([pos2(r.left() + 4.5, c.y), pos2(r.right() - 4.5, c.y)], Stroke::new(2.5, col));
    }
}

enum CueAction {
    Standby,
    Play,
    Remove,
    Follow(CueFollow),
    Loop(bool),
    Edit,
    Go,
}

impl Cuelist {
    pub fn to_data(&self) -> CuelistData {
        CuelistData { cues: self.cues.clone() }
    }

    pub fn load_data(&mut self, d: &CuelistData) {
        self.cues = d.cues.clone();
        self.standby = 0;
        self.running = None;
        self.editor = None;
    }

    pub fn add(&mut self, files: impl IntoIterator<Item = PathBuf>, at: Option<usize>) {
        let mut new: Vec<CueData> = Vec::new();
        for f in files {
            if f.is_dir() {
                let Ok(rd) = std::fs::read_dir(&f) else { continue };
                let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
                v.sort_by_key(|p| p.to_string_lossy().to_lowercase());
                new.extend(v.into_iter().filter(|p| is_media(p)).map(CueData::new));
            } else if is_media(&f) {
                new.push(CueData::new(f));
            }
        }
        let at = at.unwrap_or(self.cues.len()).min(self.cues.len());
        self.cues.splice(at..at, new);
    }

    pub fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() {
            self.add(dropped, None);
        }
    }

    /// Lance le cue `i` sur la sortie.
    fn play(&mut self, i: usize, out: &mut OutputLink) {
        let Some(c) = self.cues.get(i) else { return };
        match &c.path {
            Some(p) => out.send(Command::Load {
                path: p.to_string_lossy().into_owned(),
                paused: false,
                looping: c.clip.sound.looping,
                clip: c.clip.clone(),
                // Un cue qui enchaîne doit signaler sa fin ; sinon sa dernière image reste.
                hold: c.follow == CueFollow::Stop,
                transition: c.transition,
            }),
            None => out.send(Command::Stop),
        }
        self.running = Some(i);
        self.seen_end = out.state.ended;
    }

    /// GO : reprend si en pause, sinon lance le cue en attente et passe au suivant.
    fn go(&mut self, out: &mut OutputLink) {
        if out.state.paused && out.state.current.is_some() {
            out.send(Command::Play);
            return;
        }
        if self.standby < self.cues.len() {
            let i = self.standby;
            self.play(i, out);
            self.standby = i + 1;
        }
    }

    fn stop(&mut self, out: &mut OutputLink) {
        out.send(Command::Stop);
        self.running = None;
    }

    fn step(&mut self, d: i32) {
        let n = self.cues.len() as i32;
        self.standby = (self.standby as i32 + d).clamp(0, n) as usize;
    }

    fn toggle_pause(&mut self, out: &mut OutputLink) {
        if out.state.current.is_some() {
            out.send(if out.state.paused { Command::Play } else { Command::Pause });
        }
    }

    /// Commande OSC « cuelist/… ».
    /// État publié par OSC.
    pub fn osc_state(&self) -> crate::osc::CuelistState {
        let name = |i: usize| self.cues.get(i).map(|c| c.name.clone()).unwrap_or_default();
        crate::osc::CuelistState {
            cues: self.cues.len(),
            standby: if self.standby < self.cues.len() { self.standby + 1 } else { 0 },
            standby_name: name(self.standby),
            running_name: self.running.map(name).unwrap_or_default(),
        }
    }

    pub fn handle_osc(&mut self, cmd: &str, m: &crate::osc::Msg, out: &mut OutputLink) {
        match cmd {
            "go" => self.go(out),
            "stop" => self.stop(out),
            "pause" => out.send(Command::Pause),
            "play" | "resume" => out.send(Command::Play),
            "toggle" => self.toggle_pause(out),
            "next" => self.step(1),
            "prev" => self.step(-1),
            "standby" => {
                if let Some(i) = m.f64().filter(|v| *v >= 1.0) {
                    self.standby = (i as usize - 1).min(self.cues.len());
                }
            }
            _ => {}
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, out: &mut OutputLink, typing: bool) {
        // Fin du cue en cours : enchaînement s'il est réglé ainsi.
        if out.state.ended != self.seen_end {
            self.seen_end = out.state.ended;
            if let Some(i) = self.running {
                if out.state.slide.is_none() && self.cues.get(i).is_some_and(|c| c.follow == CueFollow::Continue) && self.standby == i + 1 {
                    self.go(out);
                } else {
                    self.running = None;
                }
            }
        }
        if out.state.current.is_none() && out.state.ended == self.seen_end && self.running.is_some() && !out.state.paused {
            // Sortie coupée ailleurs (NOIR, autre module) : plus de cue en cours.
            if out.state.slide.is_some() {
                self.running = None;
            }
        }
        if !typing {
            let none = egui::Modifiers::NONE;
            let (go, pause, up, down) = ui.input_mut(|i| {
                (i.consume_key(none, Key::Space), i.consume_key(none, Key::P), i.consume_key(none, Key::ArrowUp), i.consume_key(none, Key::ArrowDown))
            });
            if go {
                self.go(out);
            }
            if pause {
                self.toggle_pause(out);
            }
            if up {
                self.step(-1);
            }
            if down {
                self.step(1);
            }
        }

        let area = ui.available_rect_before_wrap();
        let len = self.cues.len();
        self.standby = self.standby.min(len);
        let standby = self.standby;
        let s = out.state.clone();
        let running = self.running.filter(|_| s.current.is_some());
        let paused = s.paused && running.is_some();

        egui::Frame::new().fill(PANEL).corner_radius(6).inner_margin(8).show(ui, |ui| {
            let width = area.width() - 16.0;
            ui.set_width(width);
            ui.set_min_height(area.height() - 16.0);
            ui.horizontal(|ui| {
                let add = add_button(ui, "Ajouter des médias", true);
                egui::Popup::menu(&add).show(|ui| {
                    if ui.button("Des fichiers…").clicked() {
                        ui.close();
                        if let Some(files) = rfd::FileDialog::new().add_filter("Médias", EXTENSIONS).pick_files() {
                            self.add(files, None);
                        }
                    }
                    if ui.button("Un dossier…").clicked() {
                        ui.close();
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.add([d], None);
                        }
                    }
                    ui.separator();
                    if ui.button("Un cue Noir").on_hover_text("Coupe l'image (avec la transition du cue)").clicked() {
                        ui.close();
                        self.cues.push(CueData::black());
                    }
                });
                let count = if len == 1 { "1 cue".to_string() } else { format!("{len} cues") };
                ui.label(RichText::new(count).color(TEXT_DIM));
            });

            // Cue en cours, façon QLab : temps écoulé, barre et temps restant (ambre en pause).
            let (run_col, run_fill) = if paused { (ACCENT, Color32::from_rgb(0x24, 0x1e, 0x12)) } else { (GO, Color32::from_rgb(0x14, 0x22, 0x16)) };
            let run_title = if paused { "EN PAUSE" } else { "EN COURS" };
            if let Some(c) = running.and_then(|i| self.cues.get(i).map(|c| (i, c))) {
                let (i, c) = c;
                egui::Frame::new().fill(run_fill).stroke(Stroke::new(1.0, run_col.gamma_multiply(0.7))).corner_radius(4).inner_margin(6).show(ui, |ui| {
                    ui.set_width(width - 12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(run_title).small().strong().color(run_col));
                        if paused {
                            ui.label(RichText::new("P ou GO pour reprendre").small().color(TEXT_DIM));
                        }
                        if c.clip.sound.looping {
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
                            loop_icon(ui.painter(), r, run_col);
                        }
                    });
                    ui.add(egui::Label::new(RichText::new(format!("{}  {}", i + 1, c.name)).strong().size(15.0).color(run_col)).truncate());
                    let frac = if s.duration > 0.0 { (s.time / s.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
                    let (bar, bresp) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::click());
                    let bresp = bresp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Cliquer pour aller à cet endroit");
                    ui.painter().rect_filled(bar, CornerRadius::same(3), Color32::from_black_alpha(120));
                    ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height())), CornerRadius::same(3), run_col);
                    if let Some(pt) = bresp.interact_pointer_pos().filter(|_| bresp.clicked() && s.duration > 0.0) {
                        let f = ((pt.x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0) as f64;
                        out.send(Command::Seek { seconds: f * s.duration });
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(fmt_time(s.time)).monospace().size(15.0).color(TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let rest = if s.duration > 0.0 { format!("-{}", fmt_time((s.duration - s.time).max(0.0))) } else { "∞".into() };
                            ui.label(RichText::new(rest).monospace().size(15.0).strong().color(Color32::WHITE));
                        });
                    });
                });
            }

            // Cue en attente : ce que le prochain GO va lancer.
            egui::Frame::new()
                .fill(Color32::from_rgb(0x1c, 0x1c, 0x20))
                .stroke(Stroke::new(1.0, if standby < len { ACCENT.gamma_multiply(0.7) } else { BORDER }))
                .corner_radius(4)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.set_width(width - 12.0);
                    let chained = standby > 0 && standby < len && self.cues[standby - 1].follow == CueFollow::Continue;
                    let title = if chained { "ENCHAÎNÉ" } else { "SUIVANT" };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).small().strong().color(if standby < len { ACCENT } else { TEXT_DIM }));
                        if chained {
                            ui.label(RichText::new(format!("part seul à la fin du cue {standby}")).small().color(TEXT_DIM));
                        }
                    });
                    match self.cues.get(standby) {
                        Some(c) => {
                            ui.add(egui::Label::new(RichText::new(format!("{}  {}", standby + 1, c.name)).strong().size(18.0).color(ACCENT)).truncate());
                            let next = match self.cues.get(standby + 1) {
                                Some(n) => format!("Ensuite : {}  {}", standby + 2, n.name),
                                None => "Dernier cue de la conduite".into(),
                            };
                            ui.add(egui::Label::new(RichText::new(next).small().color(TEXT_DIM)).truncate());
                        }
                        None => {
                            let txt = if len == 0 { "Ajoutez des médias à la conduite" } else { "Fin de la conduite" };
                            ui.label(RichText::new(txt).size(18.0).color(TEXT_DIM));
                            ui.label(RichText::new("Cliquez sur un cue pour le mettre en attente").small().color(TEXT_DIM));
                        }
                    }
                });

            let go_h = (ui.available_height() * 0.22).clamp(44.0, 84.0);
            let (go_r, _) = ui.allocate_exact_size(vec2(ui.available_width(), go_h), Sense::hover());
            let ready = paused || standby < len;
            let go_tip = if paused { "Reprend le cue en pause" } else { "Lance le cue en attente et passe au suivant (barre d'espace)" };
            if go_button(ui, go_r, ui.make_persistent_id("cl-go"), ready).on_hover_text(go_tip).clicked() {
                self.go(out);
            }

            // Précédent, pause, arrêter, suivant.
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
            let gap = 4.0;
            let w = (row.width() - 3.0 * gap) / 4.0;
            let labels = w >= 100.0;
            let label = |t: &'static str| labels.then_some(t);
            let cell = |k: f32| Rect::from_min_size(pos2(row.left() + k * (w + gap), row.top()), vec2(w, row.height()));
            if transport_button(ui, cell(0.0), "cl-prev", TransportIcon::Prev, label("Précédent"), PANEL_HI, TEXT)
                .on_hover_text("Précédent : met le cue précédent en attente, sans le lancer (flèche Haut)")
                .clicked()
            {
                self.step(-1);
            }
            let (picon, plabel, pfill, pfg) = if paused {
                (TransportIcon::Play, "Reprendre", ACCENT.gamma_multiply(0.85), Color32::BLACK)
            } else {
                (TransportIcon::Pause, "Pause", PANEL_HI, if running.is_some() { TEXT } else { TEXT_DIM })
            };
            if transport_button(ui, cell(1.0), "cl-pause", picon, label(plabel), pfill, pfg).on_hover_text("Pause / reprise (P)").clicked() {
                self.toggle_pause(out);
            }
            if transport_button(ui, cell(2.0), "cl-stop", TransportIcon::Stop, label("Arrêter"), STOP.gamma_multiply(0.75), Color32::WHITE)
                .on_hover_text("Arrêter : écran noir")
                .clicked()
            {
                self.stop(out);
            }
            if transport_button(ui, cell(3.0), "cl-next", TransportIcon::Next, label("Suivant"), PANEL_HI, TEXT)
                .on_hover_text("Suivant : met le cue suivant en attente, sans le lancer (flèche Bas)")
                .clicked()
            {
                self.step(1);
            }
            ui.add_space(2.0);

            self.list(ui, running, &s);
        });
        self.editor_window(ui.ctx(), out);
    }

    /// La conduite : clic = mettre en attente, double-clic = GO sur ce cue, glisser = changer l'ordre.
    fn list(&mut self, ui: &mut egui::Ui, running: Option<usize>, s: &crate::link::OutputState) {
        let standby = self.standby;
        let len = self.cues.len();
        let mut moved: Option<(usize, usize)> = None;
        let mut action: Option<(usize, CueAction)> = None;
        egui::ScrollArea::vertical().id_salt("cl-list").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let mut rows: Vec<Rect> = Vec::with_capacity(len);
            for (i, c) in self.cues.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
                rows.push(rect);
                let row = ui.interact(rect, ui.make_persistent_id(("cl-row", i)), Sense::click_and_drag());
                let grip_r = Rect::from_min_max(rect.min, pos2(rect.left() + 38.0, rect.bottom()));
                let grip = ui
                    .interact(grip_r, ui.make_persistent_id(("cl-grip", i)), Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text("Glisser pour déplacer ce cue");
                let grip_hovered = grip.hovered();
                let resp = row.union(grip);
                let p = ui.painter_at(rect);
                let is_standby = i == standby;
                let is_running = running == Some(i);
                if is_standby {
                    p.rect_filled(rect, CornerRadius::same(3), ACCENT.gamma_multiply(0.18));
                    p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), CornerRadius::same(1), ACCENT);
                } else if resp.hovered() {
                    p.rect_filled(rect, CornerRadius::same(3), PANEL_HI);
                }
                if is_running && s.duration > 0.0 {
                    let frac = (s.time / s.duration).clamp(0.0, 1.0) as f32;
                    p.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width() * frac, rect.height())), CornerRadius::same(3), GO.gamma_multiply(0.22));
                }
                let col = if is_running { GO } else if is_standby { ACCENT } else { TEXT };
                if grip_hovered && !is_standby {
                    for dx in [10.0, 14.0] {
                        for dy in [-4.0, 0.0, 4.0] {
                            p.circle_filled(pos2(rect.left() + dx, rect.center().y + dy), 1.3, TEXT);
                        }
                    }
                } else if is_standby {
                    p.text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, "▶", FontId::proportional(11.0), ACCENT);
                }
                p.text(pos2(rect.left() + 34.0, rect.center().y), Align2::RIGHT_CENTER, format!("{}", i + 1), FontId::monospace(12.0), TEXT_DIM);
                let follow_r = Rect::from_center_size(pos2(rect.left() + 47.0, rect.center().y), vec2(18.0, 18.0));
                let fresp = ui.interact(follow_r, ui.make_persistent_id(("cl-follow", i)), Sense::click());
                let chained = c.follow == CueFollow::Continue;
                follow_icon(&p, follow_r, chained, fresp.hovered());
                let tip = if chained {
                    "Enchaîner : à la fin, le cue suivant part tout seul (clic : arrêter)"
                } else {
                    "Arrêter : à la fin, on attend le prochain GO (clic : enchaîner)"
                };
                if fresp.on_hover_text(tip).clicked() {
                    action = Some((i, CueAction::Follow(if chained { CueFollow::Stop } else { CueFollow::Continue })));
                }

                // À droite : temps restant s'il passe, transition, boucle.
                let mut right = rect.right() - 4.0;
                let lr = Rect::from_center_size(pos2(right - 10.0, rect.center().y), vec2(20.0, 16.0));
                let lresp = ui.interact(lr, ui.make_persistent_id(("cl-loop", i)), Sense::click());
                let lc = if c.clip.sound.looping { ACCENT } else if lresp.hovered() || resp.hovered() { TEXT_DIM } else { BORDER };
                loop_icon(&p, Rect::from_center_size(lr.center(), vec2(16.0, 9.0)), lc);
                if lresp.on_hover_text(if c.clip.sound.looping { "En boucle (clic : une seule fois)" } else { "Lire ce cue en boucle" }).clicked() {
                    action = Some((i, CueAction::Loop(!c.clip.sound.looping)));
                }
                right = lr.left() - 6.0;
                if c.transition.kind != TransitionKind::Cut {
                    let txt = format!("{} {:.1} s", if c.transition.kind == TransitionKind::Crossfade { "Fondu" } else { "Noir" }, c.transition.secs);
                    let g = p.layout_no_wrap(txt, FontId::proportional(11.0), TEXT_DIM);
                    p.galley(pos2(right - g.size().x, rect.center().y - g.size().y / 2.0), g.clone(), TEXT_DIM);
                    right -= g.size().x + 8.0;
                }
                if is_running && s.duration > 0.0 {
                    let g = p.layout_no_wrap(format!("-{}", fmt_time((s.duration - s.time).max(0.0))), FontId::monospace(11.0), GO);
                    p.galley(pos2(right - g.size().x, rect.center().y - g.size().y / 2.0), g.clone(), GO);
                    right -= g.size().x + 8.0;
                }
                let name_r = Rect::from_min_max(pos2(rect.left() + 62.0, rect.top() + 2.0), pos2(right, rect.bottom() - 2.0));
                let name = if c.path.is_none() { format!("■ {}", c.name) } else { c.name.clone() };
                let mut job = egui::text::LayoutJob::simple_singleline(name, FontId::proportional(13.5), col);
                job.wrap = egui::text::TextWrapping::truncate_at_width(name_r.width().max(10.0));
                let g = p.layout_job(job);
                p.galley(pos2(name_r.left(), rect.center().y - g.size().y / 2.0), g, col);

                if resp.double_clicked() {
                    action = Some((i, CueAction::Go));
                } else if resp.clicked() {
                    action = Some((i, CueAction::Standby));
                }
                resp.dnd_set_drag_payload(CuePayload { index: i, name: c.name.clone() });
                resp.context_menu(|ui| {
                    if ui.button("Lancer maintenant").clicked() {
                        action = Some((i, CueAction::Play));
                        ui.close();
                    }
                    if ui.button("Réglages…").clicked() {
                        action = Some((i, CueAction::Edit));
                        ui.close();
                    }
                    if ui.button("Mettre en attente").clicked() {
                        action = Some((i, CueAction::Standby));
                        ui.close();
                    }
                    if ui.button("Retirer de la conduite").clicked() {
                        action = Some((i, CueAction::Remove));
                        ui.close();
                    }
                });
            }
            let rest = ui.available_rect_before_wrap();
            let rest = Rect::from_min_size(rest.min, vec2(rest.width(), rest.height().max(ROW_H)));
            ui.allocate_rect(rest, Sense::hover());

            // Point de dépôt d'après la position du pointeur parmi les lignes.
            let view = ui.clip_rect();
            if egui::DragAndDrop::has_payload_of_type::<CuePayload>(ui.ctx()) {
                if let Some(pt) = ui.ctx().pointer_latest_pos().filter(|p| view.contains(*p)) {
                    let at = rows.iter().filter(|r| r.center().y < pt.y).count();
                    let y = match (rows.get(at), rows.last()) {
                        (Some(r), _) => r.top() - 0.5,
                        (None, Some(r)) => r.bottom() + 0.5,
                        (None, None) => rest.top(),
                    };
                    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(2.0, EDIT));
                    if ui.input(|i| i.pointer.any_released()) {
                        if let Some(pl) = egui::DragAndDrop::take_payload::<CuePayload>(ui.ctx()) {
                            moved = Some((pl.index, at));
                        }
                    }
                }
                // Étiquette du cue glissé.
                if let (Some(pl), Some(p)) = (egui::DragAndDrop::payload::<CuePayload>(ui.ctx()), ui.ctx().pointer_interact_pos()) {
                    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("cl-drag")));
                    let g = painter.layout_no_wrap(format!("{}  {}", pl.index + 1, pl.name), FontId::proportional(13.0), Color32::WHITE);
                    let r = Rect::from_min_size(p + vec2(12.0, 8.0), g.size() + vec2(12.0, 6.0));
                    painter.rect_filled(r, CornerRadius::same(4), EDIT.gamma_multiply(0.9));
                    painter.galley(r.min + vec2(6.0, 3.0), g, Color32::WHITE);
                }
            }
            // Le cue en attente reste visible.
            let seen_id = ui.make_persistent_id("cl-standby-seen");
            if ui.data(|d| d.get_temp::<usize>(seen_id)) != Some(standby) {
                if let Some(r) = rows.get(standby) {
                    ui.scroll_to_rect(r.expand2(vec2(0.0, ROW_H)), None);
                }
                ui.data_mut(|d| d.insert_temp(seen_id, standby));
            }
            if len == 0 {
                ui.painter().text(rest.left_top() + vec2(6.0, 6.0), Align2::LEFT_TOP, "Glissez ici des fichiers ou des dossiers.", FontId::proportional(13.0), TEXT_DIM);
            }
        });

        if let Some((from, to)) = moved {
            if from < self.cues.len() {
                let c = self.cues.remove(from);
                let to = if to > from { to - 1 } else { to }.min(self.cues.len());
                self.cues.insert(to, c);
                // Le cue en attente et celui en cours suivent leur cue.
                let fix = |k: usize| if k == from { to } else if from < k && k <= to { k - 1 } else if to <= k && k < from { k + 1 } else { k };
                self.standby = fix(self.standby).min(self.cues.len());
                self.running = self.running.map(fix);
            }
        }
        match action {
            Some((i, CueAction::Standby)) => self.standby = i,
            Some((i, CueAction::Play)) => self.pending_play = Some(i),
            Some((i, CueAction::Remove)) => {
                self.cues.remove(i);
                if self.standby > i {
                    self.standby -= 1;
                }
                self.running = self.running.and_then(|r| if r == i { None } else if r > i { Some(r - 1) } else { Some(r) });
            }
            Some((i, CueAction::Follow(f))) => self.cues[i].follow = f,
            Some((i, CueAction::Loop(l))) => self.cues[i].clip.sound.looping = l,
            Some((i, CueAction::Edit)) => self.editor = Some(i),
            Some((i, CueAction::Go)) => {
                self.standby = i;
                self.pending_go = true;
            }
            None => {}
        }
    }

    /// Réglages d'un cue : nom, fichier, boucle, transition, suite à donner.
    fn editor_window(&mut self, ctx: &egui::Context, out: &mut OutputLink) {
        if let Some(i) = self.pending_play.take() {
            self.play(i, out);
        }
        if std::mem::take(&mut self.pending_go) {
            self.go(out);
        }
        self.preview.poll();
        self.preview.settle(ctx);
        let Some(i) = self.editor else { return };
        let running = self.running == Some(i) && out.state.current.is_some();
        let Some(c) = self.cues.get_mut(i) else {
            self.editor = None;
            return;
        };
        let clip_before = c.clip.clone();
        let mut open = true;
        let mut done = false;
        let preview = &mut self.preview;
        egui::Window::new(format!("Cue {}", i + 1))
            .id(Id::unique("cl-editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(660.0)
            .default_height((ctx.content_rect().height() - 80.0).max(200.0))
            .max_height((ctx.content_rect().height() - 60.0).max(200.0))
            .show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("cl-editor-scroll").auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("cl-fields").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Nom");
                ui.text_edit_singleline(&mut c.name);
                ui.end_row();
                ui.label("Média");
                ui.horizontal(|ui| {
                    let txt = c.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Noir (pas de média)".into());
                    ui.label(RichText::new(txt).color(TEXT_DIM).small());
                    if ui.button("Changer…").clicked() {
                        if let Some(f) = rfd::FileDialog::new().add_filter("Médias", EXTENSIONS).pick_file() {
                            c.path = Some(f);
                        }
                    }
                });
                ui.end_row();
                ui.label("Transition");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut c.transition.kind, TransitionKind::Cut, "Coupe");
                    ui.radio_value(&mut c.transition.kind, TransitionKind::Black, "Noir");
                    ui.radio_value(&mut c.transition.kind, TransitionKind::Crossfade, "Fondu enchaîné").on_hover_text("Fondu enchaîné : le média suivant apparaît pendant que le précédent disparaît");
                    if c.transition.kind != TransitionKind::Cut {
                        ui.add(egui::DragValue::new(&mut c.transition.secs).range(0.1..=10.0).speed(0.05).suffix(" s").max_decimals(1));
                    }
                });
                ui.end_row();
                ui.label("À la fin");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut c.follow, CueFollow::Stop, "Arrêter (attendre le GO)");
                    ui.radio_value(&mut c.follow, CueFollow::Continue, "Enchaîner sur le cue suivant");
                });
                ui.end_row();
            });
            // Réglages du média, comme un élément du Lecteur ou un jingle de JingleStar.
            if let Some(path) = c.path.clone() {
                ui.add_space(6.0);
                ui.separator();
                super::media_editor::clip_editor(ui, ("cl", i), &path, &mut c.clip, preview);
            }
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("✓ Terminé", "✓ Done")).clicked() {
                    done = true;
                }
            });
            });
        });
        // Cue à l'antenne : volume, EQ et fondus suivent tout de suite.
        if running && c.clip != clip_before {
            out.send(Command::SetClip { clip: c.clip.clone() });
        }
        if !open || done {
            self.editor = None;
        }
    }
}

fn is_media(p: &Path) -> bool {
    p.extension().map(|e| EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

impl CueData {
    pub fn new(path: PathBuf) -> Self {
        Self { name: stem(&path), path: Some(path), ..Default::default() }
    }
    pub fn black() -> Self {
        Self { name: "Noir".into(), path: None, ..Default::default() }
    }
}
