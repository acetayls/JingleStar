//! Module Lecteur : une liste de médias, un GO, la position de lecture.

use crate::link::OutputLink;
use crate::protocol::Command;
use egui::{vec2, Align, Key, Layout, RichText, Sense};
use star_commun::theme::*;
use star_commun::widgets::{self, fmt_time};
use std::path::PathBuf;

const EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "mxf", "mpg", "mpeg", "ts", "wmv", "jpg", "jpeg", "png", "bmp", "gif", "tif",
    "tiff", "webp", "mp3", "wav", "flac", "aac", "m4a", "ogg",
];

#[derive(Default)]
pub struct Lecteur {
    items: Vec<PathBuf>,
    selected: Option<usize>,
    /// Média envoyé à la sortie en dernier (pour le surligner).
    playing: Option<usize>,
}

fn name(p: &PathBuf) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Lecteur {
    fn add(&mut self, files: impl IntoIterator<Item = PathBuf>) {
        for f in files {
            let ok = f.extension().map(|e| EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false);
            if ok {
                self.items.push(f);
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

    fn go(&mut self, out: &mut OutputLink) {
        let Some(i) = self.selected.filter(|i| *i < self.items.len()) else { return };
        out.send(Command::Load { path: self.items[i].to_string_lossy().into_owned(), paused: false });
        self.playing = Some(i);
        // Le GO suivant enchaîne sur le média d'après.
        if i + 1 < self.items.len() {
            self.selected = Some(i + 1);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, out: &mut OutputLink, typing: bool) {
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

        // Transport.
        ui.horizontal(|ui| {
            if widgets::big_button(ui, "▶  GO", GO, vec2(140.0, 48.0)).on_hover_text("Lance le média sélectionné (Espace)").clicked() {
                self.go(out);
            }
            let paused = out.state.paused;
            if ui.add(egui::Button::new(if paused { "⏵ Reprendre" } else { "⏸ Pause" }).min_size(vec2(110.0, 48.0))).clicked() {
                out.send(if paused { Command::Play } else { Command::Pause });
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                let s = &out.state;
                let title = s.current.as_ref().map(|p| name(&PathBuf::from(p))).unwrap_or_else(|| "—".into());
                ui.label(RichText::new(title).size(16.0).strong());
                ui.label(RichText::new(format!("{}  /  {}", fmt_time(s.time), fmt_time(s.duration))).monospace().color(TEXT_DIM));
            });
        });
        ui.add_space(4.0);
        self.progress(ui, out);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Médias").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Vider").clicked() {
                    self.items.clear();
                    self.selected = None;
                    self.playing = None;
                }
                if ui.button("+ Ajouter…").clicked() {
                    if let Some(files) = rfd::FileDialog::new().add_filter("Médias", EXTENSIONS).pick_files() {
                        self.add(files);
                    }
                }
            });
        });

        egui::Frame::new().fill(SLOT_EMPTY).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if self.items.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Glisse des vidéos, images ou sons ici, ou clique sur « Ajouter… »").color(TEXT_DIM));
                });
                return;
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut remove = None;
                for (i, p) in self.items.iter().enumerate() {
                    let sel = self.selected == Some(i);
                    let live = self.playing == Some(i) && out.state.current.is_some();
                    let fill = if live {
                        GO.gamma_multiply(0.35)
                    } else if sel {
                        ACCENT.gamma_multiply(0.25)
                    } else {
                        PANEL
                    };
                    let resp = egui::Frame::new()
                        .fill(fill)
                        .corner_radius(3)
                        .inner_margin(egui::Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{:>2}", i + 1)).monospace().color(TEXT_DIM));
                                ui.label(if live { "▶" } else { " " });
                                ui.label(name(p));
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        self.selected = Some(i);
                    }
                    if resp.double_clicked() {
                        self.selected = Some(i);
                        out.send(Command::Load { path: p.to_string_lossy().into_owned(), paused: false });
                        self.playing = Some(i);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Retirer").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                }
                if let Some(i) = remove {
                    self.items.remove(i);
                    self.selected = self.selected.filter(|s| *s < self.items.len());
                    self.playing = None;
                }
            });
        });
    }

    fn progress(&self, ui: &mut egui::Ui, out: &mut OutputLink) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::click_and_drag());
        let p = ui.painter();
        p.rect_filled(rect, 3, SLOT_EMPTY);
        let s = &out.state;
        if s.duration > 0.0 {
            let k = (s.time / s.duration).clamp(0.0, 1.0) as f32;
            let mut done = rect;
            done.set_width(rect.width() * k);
            p.rect_filled(done, 3, if s.paused { ACCENT } else { GO });
            if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked() || resp.drag_stopped()) {
                let k = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                out.send(Command::Seek { seconds: k * s.duration });
            }
        }
    }
}
