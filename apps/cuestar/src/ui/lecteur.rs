//! Module Lecteur : la playlist, avec la charte et les boutons de la playlist de JingleStar.
//!
//! Les éléments s'enchaînent (dans l'ordre ou au hasard), la liste peut boucler en entier,
//! et chaque élément peut tourner en boucle. À droite, l'aperçu de l'élément suivant.

use crate::link::OutputLink;
use crate::protocol::{Clip, Command, Transition, TransitionKind};
use crate::thumb::Thumbs;
use egui::{pos2, vec2, Align, Color32, CornerRadius, Key, Layout, Rect, RichText, Sense};
use star_ui::theme::*;
use star_ui::widgets::{add_button, fmt_time, loop_icon, mode_chip, pressed, track_row, transport_button, TransportIcon};
use std::path::{Path, PathBuf};

const EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "mxf", "mpg", "mpeg", "ts", "wmv", "jpg", "jpeg", "png", "bmp", "gif", "tif",
    "tiff", "webp", "mp3", "wav", "flac", "aac", "m4a", "ogg",
];

fn is_media(p: &Path) -> bool {
    p.extension().map(|e| EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

/// Un élément : son média et ses réglages d'édition (boucle comprise).
type Item = crate::project::LecteurItem;

fn new_item(path: PathBuf) -> Item {
    Item { path, clip: Clip::default() }
}

pub struct Lecteur {
    items: Vec<Item>,
    selected: Option<usize>,
    /// Élément envoyé à la sortie en dernier.
    playing: Option<usize>,
    /// Élément prévu après celui en cours.
    queued: Option<usize>,
    loop_all: bool,
    shuffle: bool,
    transition: Transition,
    confirm_clear: bool,
    /// Dernière fin de média traitée.
    seen_end: u64,
    rng: u64,
    thumbs: Thumbs,
    /// Aperçu SUIVANT affiché, et sa largeur en points.
    show_preview: bool,
    preview_w: f32,
}

impl Default for Lecteur {
    fn default() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
        Self {
            items: Vec::new(),
            selected: None,
            playing: None,
            queued: None,
            loop_all: false,
            shuffle: false,
            transition: Transition::default(),
            confirm_clear: false,
            seen_end: 0,
            rng: seed,
            thumbs: Thumbs::default(),
            show_preview: true,
            preview_w: 420.0,
        }
    }
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn scan(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    v.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    for p in v {
        if p.is_dir() {
            scan(&p, out);
        } else if is_media(&p) {
            out.push(p);
        }
    }
}

impl Lecteur {
    /// Contenu enregistré dans le projet.
    pub fn to_data(&self) -> crate::project::LecteurData {
        crate::project::LecteurData {
            items: self.items.clone(),
            loop_all: self.loop_all,
            shuffle: self.shuffle,
            transition: self.transition,
        }
    }

    /// Projet ouvert : la liste est remplacée.
    pub fn load_data(&mut self, d: &crate::project::LecteurData) {
        self.items = d.items.clone();
        self.loop_all = d.loop_all;
        self.shuffle = d.shuffle;
        self.transition = d.transition;
        self.selected = (!self.items.is_empty()).then_some(0);
        self.playing = None;
        self.queued = None;
        self.confirm_clear = false;
    }

    /// Aléatoire et Tout boucler (pour les touches qui les basculent).
    pub fn flags(&self) -> (bool, bool) {
        (self.shuffle, self.loop_all)
    }

    /// Commande OSC « lecteur/… ».
    pub fn handle_osc(&mut self, cmd: &str, m: &crate::osc::Msg, out: &mut OutputLink) {
        match cmd {
            "go" => self.go(out),
            "play" => out.send(Command::Play),
            "pause" => out.send(Command::Pause),
            "toggle" => out.send(if out.state.paused { Command::Play } else { Command::Pause }),
            "stop" => self.stop(out),
            "next" => self.step(1, out),
            "prev" => self.step(-1, out),
            "goto" => {
                if let Some(i) = m.f64().filter(|v| *v >= 1.0) {
                    self.play(i as usize - 1, out);
                }
            }
            "shuffle" => {
                self.shuffle = m.flag();
                self.requeue();
            }
            "loop" => {
                self.loop_all = m.flag();
                self.requeue();
            }
            "transition" => {
                match m.text().unwrap_or_default().to_lowercase().as_str() {
                    "cut" | "coupe" => self.transition.kind = TransitionKind::Cut,
                    "black" | "noir" => self.transition.kind = TransitionKind::Black,
                    "fade" | "crossfade" | "fondu" => self.transition.kind = TransitionKind::Crossfade,
                    _ => {}
                }
                if let Some(rosc::OscType::Float(s)) = m.args.get(1) {
                    self.transition.secs = (*s as f64).clamp(0.1, 10.0);
                }
            }
            _ => {}
        }
    }

    fn add(&mut self, files: impl IntoIterator<Item = PathBuf>) {
        for f in files {
            if f.is_dir() {
                let mut v = Vec::new();
                scan(&f, &mut v);
                self.items.extend(v.into_iter().map(new_item));
            } else if is_media(&f) {
                self.items.push(new_item(f));
            }
        }
        if self.selected.is_none() && !self.items.is_empty() {
            self.selected = Some(0);
        }
    }

    pub fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() {
            self.add(dropped);
        }
    }

    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Élément qui suit `i` (None : fin de liste).
    fn next_after(&mut self, i: usize) -> Option<usize> {
        let n = self.items.len();
        if n == 0 {
            return None;
        }
        if self.shuffle {
            if n == 1 {
                return self.loop_all.then_some(0);
            }
            // Au hasard, jamais deux fois le même d'affilée.
            let r = (self.rand() % (n as u64 - 1)) as usize;
            return Some(if r >= i { r + 1 } else { r });
        }
        if i + 1 < n {
            Some(i + 1)
        } else if self.loop_all {
            Some(0)
        } else {
            None
        }
    }

    fn play(&mut self, i: usize, out: &mut OutputLink) {
        let Some(it) = self.items.get(i) else { return };
        // La liste s'enchaîne : la fin du média est signalée ; le dernier garde son image.
        let (path, looping, clip) = (it.path.to_string_lossy().into_owned(), it.clip.sound.looping, it.clip.clone());
        self.queued = self.next_after(i);
        out.send(Command::Load { path, paused: false, looping, hold: self.queued.is_none(), transition: self.transition, clip });
        self.playing = Some(i);
        self.seen_end = out.state.ended;
        self.selected = self.queued.or(Some(i));
    }

    fn go(&mut self, out: &mut OutputLink) {
        if out.state.paused && out.state.current.is_some() {
            out.send(Command::Play);
        } else if let Some(i) = self.selected.filter(|i| *i < self.items.len()) {
            self.play(i, out);
        }
    }

    fn step(&mut self, d: i32, out: &mut OutputLink) {
        let n = self.items.len() as i32;
        if n == 0 {
            return;
        }
        let from = self.playing.or(self.selected).unwrap_or(0) as i32;
        let i = (from + d).rem_euclid(n) as usize;
        self.play(i, out);
    }

    fn stop(&mut self, out: &mut OutputLink) {
        out.send(Command::Stop);
        if let Some(i) = self.playing.take() {
            self.selected = Some(i);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, out: &mut OutputLink, typing: bool) {
        // Fin d'un média : on enchaîne.
        if out.state.ended != self.seen_end {
            self.seen_end = out.state.ended;
            if self.playing.is_some() && out.state.slide.is_none() {
                match self.queued.filter(|n| *n < self.items.len()) {
                    Some(n) => self.play(n, out),
                    None => self.playing = None,
                }
            }
        }
        if !typing {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Space)) {
                self.go(out);
            }
            if let Some(s) = self.selected {
                if ui.input(|i| i.key_pressed(Key::ArrowDown)) && s + 1 < self.items.len() {
                    self.selected = Some(s + 1);
                }
                if ui.input(|i| i.key_pressed(Key::ArrowUp)) && s > 0 {
                    self.selected = Some(s - 1);
                }
            }
        }

        let full = ui.available_rect_before_wrap();
        if !self.show_preview {
            ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| self.playlist(ui, out, full));
            ui.allocate_rect(full, Sense::hover());
            return;
        }
        // Aperçu à droite, largeur réglable avec la poignée entre les deux.
        let gap = 12.0;
        let max_w = (full.width() - 320.0 - gap).max(140.0);
        self.preview_w = self.preview_w.clamp(140.0, max_w);
        let right = Rect::from_min_max(pos2(full.right() - self.preview_w, full.top()), full.max);
        let left = Rect::from_min_max(full.min, pos2(right.left() - gap, full.bottom()));
        let handle = Rect::from_min_max(pos2(left.right(), full.top()), pos2(right.left(), full.bottom()));
        let h = ui.interact(handle, ui.make_persistent_id("lec-split"), Sense::drag());
        if h.dragged() {
            self.preview_w -= h.drag_delta().x;
        }
        if h.hovered() || h.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let grip = if h.hovered() || h.dragged() { ACCENT } else { BORDER };
        ui.painter().rect_filled(Rect::from_center_size(handle.center(), vec2(3.0, 40.0)), 2, grip);
        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| self.playlist(ui, out, left));
        ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| self.next_preview(ui, right));
        ui.allocate_rect(full, Sense::hover());
    }

    /// La playlist, disposée comme celle de JingleStar.
    fn playlist(&mut self, ui: &mut egui::Ui, out: &mut OutputLink, area: Rect) {
        egui::Frame::new().fill(PANEL).corner_radius(6).inner_margin(8).show(ui, |ui| {
            let area = area.shrink(8.0);
            ui.set_width(area.width());
            ui.horizontal(|ui| {
                let add = add_button(ui, "Ajouter", true);
                egui::Popup::menu(&add).show(|ui| {
                    if ui.button("Des fichiers…").clicked() {
                        ui.close();
                        if let Some(files) = rfd::FileDialog::new().add_filter("Médias", EXTENSIONS).pick_files() {
                            self.add(files);
                        }
                    }
                    if ui.button("Un dossier…").clicked() {
                        ui.close();
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.add([d]);
                        }
                    }
                });
                if !self.items.is_empty() {
                    if self.confirm_clear {
                        if ui.add(egui::Button::new(RichText::new("Vider ?").color(Color32::WHITE)).fill(STOP)).clicked() {
                            self.items.clear();
                            self.selected = None;
                            self.playing = None;
                            self.queued = None;
                            self.confirm_clear = false;
                        }
                        if ui.button("Annuler").clicked() {
                            self.confirm_clear = false;
                        }
                    } else if ui.button("🗑 Vider").on_hover_text("Vider la playlist (avec confirmation)").clicked() {
                        self.confirm_clear = true;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.selectable_label(self.show_preview, "Aperçu").on_hover_text("Afficher ou masquer l'aperçu de l'élément suivant").clicked() {
                        self.show_preview = !self.show_preview;
                    }
                    let label = if self.items.is_empty() { "Aucun média".to_string() } else { format!("{} médias", self.items.len()) };
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(label).color(TEXT_DIM)).truncate());
                    });
                });
            });

            // Élément en cours.
            let s = &out.state;
            let playing_now = self.playing.filter(|_| s.current.is_some());
            let current_name = playing_now.and_then(|i| self.items.get(i)).map(|it| stem(&it.path)).unwrap_or_else(|| "—".into());
            let paused = s.paused && playing_now.is_some();
            let mut seek = None;
            egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x1c, 0x20)).corner_radius(4).inner_margin(6).show(ui, |ui| {
                let inner = (area.width() - 12.0).max(0.0);
                ui.set_width(inner);
                ui.set_max_width(inner);
                let col = if paused { ACCENT } else if playing_now.is_some() { GO } else { TEXT };
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if paused {
                            ui.label(RichText::new("EN PAUSE").strong().small().color(ACCENT));
                        }
                        if playing_now.and_then(|i| self.items.get(i)).is_some_and(|it| it.clip.sound.looping) {
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
                            loop_icon(ui.painter(), r, ACCENT);
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(&current_name).strong().size(15.0).color(col)).truncate()).on_hover_text(&current_name);
                        });
                    });
                });
                let (mut t, len) = if playing_now.is_some() { (s.time, s.duration) } else { (0.0, 0.0) };
                // Barre de lecture : un clic ou un glisser (appliqué au relâché) déplace la lecture.
                let (hit, bar) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::click_and_drag());
                let r = Rect::from_center_size(hit.center(), vec2(hit.width(), 6.0));
                let frac_at = |x: f32| ((x - r.left()) / r.width()).clamp(0.0, 1.0) as f64;
                if len > 0.0 {
                    if let Some(p) = bar.interact_pointer_pos() {
                        if bar.dragged() {
                            t = frac_at(p.x) * len;
                        }
                        if bar.clicked() || bar.drag_stopped() {
                            seek = Some(frac_at(p.x) * len);
                            t = frac_at(p.x) * len;
                        }
                    }
                    if bar.hovered() || bar.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
                ui.painter().rect_filled(r, CornerRadius::same(3), PANEL_HI);
                if len > 0.0 {
                    let w = r.width() * (t / len).clamp(0.0, 1.0) as f32;
                    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), CornerRadius::same(3), GO);
                    if bar.hovered() || bar.dragged() {
                        ui.painter().circle_filled(pos2(r.left() + w, r.center().y), 5.0, Color32::WHITE);
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(fmt_time(t)).monospace().color(TEXT_DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("-{}", fmt_time((len - t).max(0.0)))).monospace().color(TEXT_DIM));
                    });
                });
            });
            if let Some(sec) = seek {
                out.send(Command::Seek { seconds: sec });
            }

            // Transport : précédent, GO, pause, stop, suivant sur toute la largeur.
            let (row, _) = ui.allocate_exact_size(vec2(area.width(), 38.0), Sense::hover());
            let gap = 4.0;
            let k = ((row.width() - 4.0 * gap) / (2.0 * 40.0 + 2.0 * 48.0 + 64.0)).min(1.0);
            let (side, stop_w) = (40.0 * k, 48.0 * k);
            let go_w = row.width() - 2.0 * side - 2.0 * stop_w - 4.0 * gap;
            let mut x = row.left();
            let mut cell = |w: f32| {
                let r = Rect::from_min_size(pos2(x, row.top()), vec2(w, row.height()));
                x += w + gap;
                r
            };
            let (prev_r, go_r, pause_r, stop_r, next_r) = (cell(side), cell(go_w), cell(stop_w), cell(stop_w), cell(side));
            if transport_button(ui, prev_r, "lec-prev", TransportIcon::Prev, None, PANEL_HI, TEXT).on_hover_text("Élément précédent").clicked() {
                self.step(-1, out);
            }
            let go = transport_button(ui, go_r, "lec-go", TransportIcon::Play, Some("GO"), GO, Color32::from_rgb(0x0c, 0x1a, 0x0e))
                .on_hover_text(if paused { "Reprendre là où l'élément s'est arrêté" } else { "Lancer l'élément sélectionné (Espace)" });
            if pressed(ui, &go) {
                self.go(out);
            }
            let (pause_fill, pause_fg) = if paused { (ACCENT, Color32::BLACK) } else { (PANEL_HI, TEXT) };
            let pause = transport_button(ui, pause_r, "lec-pause", TransportIcon::Pause, None, pause_fill, pause_fg)
                .on_hover_text(if paused { "Reprendre" } else { "Pause : l'élément reprendra au même endroit" });
            if pressed(ui, &pause) && playing_now.is_some() {
                out.send(if paused { Command::Play } else { Command::Pause });
            }
            let stop = transport_button(ui, stop_r, "lec-stop", TransportIcon::Stop, None, STOP.gamma_multiply(0.75), Color32::WHITE).on_hover_text("Arrêter : écran noir");
            if pressed(ui, &stop) {
                self.stop(out);
            }
            if transport_button(ui, next_r, "lec-next", TransportIcon::Next, None, PANEL_HI, TEXT).on_hover_text("Élément suivant").clicked() {
                self.step(1, out);
            }

            // Modes : les éléments s'enchaînent toujours, dans l'ordre ou au hasard.
            let (row, _) = ui.allocate_exact_size(vec2(area.width(), 26.0), Sense::hover());
            let half = (row.width() - 4.0) / 2.0;
            let shuffle_r = Rect::from_min_size(row.min, vec2(half, row.height()));
            let loop_r = Rect::from_min_size(pos2(row.left() + half + 4.0, row.top()), vec2(half, row.height()));
            if mode_chip(ui, shuffle_r, "lec-shuffle", self.shuffle, "Aléatoire").on_hover_text("Enchaîner les éléments au hasard").clicked() {
                self.shuffle = !self.shuffle;
                self.requeue();
            }
            if mode_chip(ui, loop_r, "lec-loop", self.loop_all, "Tout boucler").on_hover_text("Reprendre au début à la fin de la liste").clicked() {
                self.loop_all = !self.loop_all;
                self.requeue();
            }

            // Transition entre deux éléments.
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Transition").color(TEXT_DIM));
                ui.radio_value(&mut self.transition.kind, TransitionKind::Cut, "Coupe").on_hover_text("Passage direct au média suivant");
                ui.radio_value(&mut self.transition.kind, TransitionKind::Black, "Noir").on_hover_text("Fondu au noir, puis fondu d'ouverture");
                ui.radio_value(&mut self.transition.kind, TransitionKind::Crossfade, "Fondu enchaîné").on_hover_text("Fondu enchaîné : le média suivant apparaît pendant que le précédent disparaît");
                if self.transition.kind != TransitionKind::Cut {
                    ui.add(egui::DragValue::new(&mut self.transition.secs).range(0.1..=10.0).speed(0.05).suffix(" s").max_decimals(1));
                }
            });

            // Liste des éléments, avec la boucle de chacun à droite.
            let row_w = (area.right() - ui.cursor().left()).max(40.0);
            let mut play = None;
            let mut remove = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_width(row_w).show(ui, |ui| {
                ui.set_max_width(row_w);
                if self.items.is_empty() {
                    ui.label(RichText::new("Ajoutez des fichiers ou un dossier avec « Ajouter », ou glissez-les ici.").color(TEXT_DIM));
                }
                ui.spacing_mut().item_spacing.y = 1.0;
                for i in 0..self.items.len() {
                    let name = stem(&self.items[i].path);
                    let is_cur = playing_now == Some(i);
                    let color = if !is_cur { TEXT } else if !paused { GO } else { ACCENT };
                    let sel = self.selected == Some(i) || is_cur;
                    let (resp, anchor) = track_row(ui, &format!("{:>3}  {}", i + 1, name), color, sel, Some("    "));
                    // Boucle de l'élément : clic sur le symbole à droite.
                    let lr = Rect::from_center_size(anchor - vec2(10.0, 0.0), vec2(20.0, 16.0));
                    let lresp = ui.interact(lr, ui.make_persistent_id(("lec-item-loop", i)), Sense::click());
                    let on = self.items[i].clip.sound.looping;
                    let lc = if on { ACCENT } else if lresp.hovered() || resp.hovered() { TEXT_DIM } else { BORDER };
                    loop_icon(ui.painter(), Rect::from_center_size(lr.center(), vec2(16.0, 9.0)), lc);
                    if lresp.on_hover_text(if on { "En boucle : clic pour lire une seule fois" } else { "Lire cet élément en boucle" }).clicked() {
                        self.items[i].clip.sound.looping = !on;
                    } else if resp.double_clicked() {
                        play = Some(i);
                    } else if resp.clicked() {
                        self.selected = Some(i);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Lire").clicked() {
                            play = Some(i);
                            ui.close();
                        }
                        if ui.button(if on { "Lire une seule fois" } else { "Lire en boucle" }).clicked() {
                            self.items[i].clip.sound.looping = !on;
                            ui.close();
                        }
                        if ui.button("Retirer").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                }
            });
            if let Some(i) = play {
                self.play(i, out);
            }
            if let Some(i) = remove {
                self.items.remove(i);
                self.selected = self.selected.filter(|s| *s < self.items.len());
                self.playing = None;
                self.queued = None;
            }
        });
    }

    /// Les modes ont changé : l'élément suivant est recalculé.
    fn requeue(&mut self) {
        if let Some(i) = self.playing {
            self.queued = self.next_after(i);
            self.selected = self.queued.or(Some(i));
        }
    }

    /// Aperçu de l'élément qui partira au prochain GO ou au prochain enchaînement.
    fn next_preview(&mut self, ui: &mut egui::Ui, area: Rect) {
        let next = self.selected.and_then(|i| self.items.get(i)).map(|it| (it.path.clone(), it.clip.sound.looping));
        egui::Frame::new().fill(PANEL).corner_radius(6).inner_margin(10).show(ui, |ui| {
            let w = (area.width() - 20.0).max(80.0);
            ui.set_width(w);
            ui.horizontal(|ui| {
                ui.label(RichText::new("SUIVANT").strong().color(ACCENT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Masquer l'aperçu (bouton « Aperçu » pour le réafficher)").clicked() {
                        self.show_preview = false;
                    }
                });
            });
            let size = vec2(w, w * 9.0 / 16.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(rect, 3, Color32::BLACK);
            match &next {
                Some((p, looping)) => {
                    let ctx = ui.ctx().clone();
                    if let Some(t) = self.thumbs.get(&ctx, p) {
                        let ts = t.size_vec2();
                        let k = (size.x / ts.x).min(size.y / ts.y);
                        let r = Rect::from_center_size(rect.center(), ts * k);
                        ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    } else {
                        let txt = if self.thumbs.failed(p) { "pas d'aperçu" } else { "…" };
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, txt, egui::FontId::proportional(14.0), TEXT_DIM);
                    }
                    ui.horizontal(|ui| {
                        if *looping {
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
                            loop_icon(ui.painter(), r, ACCENT);
                        }
                        ui.add(egui::Label::new(RichText::new(stem(p)).strong().size(15.0)).truncate());
                    });
                }
                None => {
                    ui.label(RichText::new("Fin de la liste").color(TEXT_DIM));
                }
            }
        });
    }
}
