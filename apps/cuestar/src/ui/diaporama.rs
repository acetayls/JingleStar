//! Module Diaporama : reprise du projet « Diaporama » (Electron) sur le moteur CueStar.
//!
//! v1 : dossier (sous-dossiers), ordre aléatoire remélangé à chaque tour, boucle, durée des
//! photos, ajustement (image entière, remplir, auto), vidéos lues en entier avec ou sans son.
//! Le minutage et l'enchaînement tournent dans le moteur de sortie.
//! À venir : transitions, Ken Burns, fond flou, musique de fond, commandes OSC.

use crate::link::OutputLink;
use crate::protocol::{Command, Fit};
use egui::{vec2, Key, RichText};
use serde::{Deserialize, Serialize};
use star_commun::theme::*;
use star_commun::widgets;
use std::path::{Path, PathBuf};

const MEDIA_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "bmp", "gif", "tif", "tiff", "webp", "heic", "mp4", "mov", "mkv", "avi", "webm", "m4v", "mpg", "mpeg",
    "ts", "wmv",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    folder: Option<PathBuf>,
    recursive: bool,
    shuffle: bool,
    looping: bool,
    photo_seconds: f64,
    fit: Fit,
    video_sound: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { folder: None, recursive: true, shuffle: false, looping: true, photo_seconds: 5.0, fit: Fit::Auto, video_sound: false }
    }
}

fn settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "CueStar").map(|d| d.config_dir().join("diaporama.json"))
}

pub struct Diaporama {
    s: Settings,
    files: Vec<PathBuf>,
}

impl Default for Diaporama {
    fn default() -> Self {
        let s: Settings = settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut d = Self { s, files: Vec::new() };
        d.rescan();
        d
    }
}

fn scan(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    for p in entries {
        if p.is_dir() {
            if recursive {
                scan(&p, true, out);
            }
        } else if p.extension().map(|e| MEDIA_EXT.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false) {
            out.push(p);
        }
    }
}

impl Diaporama {
    fn save(&self) {
        if let Some(p) = settings_path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, serde_json::to_string_pretty(&self.s).unwrap_or_default());
        }
    }

    fn rescan(&mut self) {
        self.files.clear();
        if let Some(f) = self.s.folder.clone() {
            scan(&f, self.s.recursive, &mut self.files);
        }
    }

    fn start(&self, out: &mut OutputLink) {
        out.send(Command::Slideshow {
            items: self.files.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            photo_seconds: self.s.photo_seconds,
            looping: self.s.looping,
            shuffle: self.s.shuffle,
            fit: self.s.fit,
            video_sound: self.s.video_sound,
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, out: &mut OutputLink, typing: bool) {
        let running = out.state.slide.is_some();
        if !typing {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Space)) {
                if !running {
                    self.start(out);
                } else {
                    out.send(if out.state.paused { Command::Play } else { Command::Pause });
                }
            }
            if running && ui.input(|i| i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowDown)) {
                out.send(Command::Next);
            }
            if running && ui.input(|i| i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowUp)) {
                out.send(Command::Prev);
            }
        }

        // Transport.
        ui.horizontal(|ui| {
            let go = if running { "⟲  RELANCER" } else { "▶  LANCER" };
            if widgets::big_button(ui, go, GO, vec2(150.0, 48.0)).on_hover_text("Lance le diaporama sur la sortie (Espace)").clicked() {
                self.start(out);
            }
            ui.add_enabled_ui(running, |ui| {
                if ui.add(egui::Button::new("⏮").min_size(vec2(48.0, 48.0))).on_hover_text("Précédent (←)").clicked() {
                    out.send(Command::Prev);
                }
                let paused = out.state.paused;
                if ui.add(egui::Button::new(if paused { "⏵" } else { "⏸" }).min_size(vec2(48.0, 48.0))).on_hover_text("Pause / reprise (Espace)").clicked() {
                    out.send(if paused { Command::Play } else { Command::Pause });
                }
                if ui.add(egui::Button::new("⏭").min_size(vec2(48.0, 48.0))).on_hover_text("Suivant (→)").clicked() {
                    out.send(Command::Next);
                }
                if ui.add(egui::Button::new("■").min_size(vec2(48.0, 48.0))).on_hover_text("Arrêter : écran noir (Échap)").clicked() {
                    out.send(Command::Stop);
                }
            });
            ui.add_space(12.0);
            ui.vertical(|ui| match &out.state.slide {
                Some((i, n, p)) => {
                    let name = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(RichText::new(name).size(16.0).strong());
                    let state = if out.state.paused { "  (pause)" } else { "" };
                    ui.label(RichText::new(format!("{} / {}{state}", i + 1, n)).color(TEXT_DIM));
                }
                None => {
                    ui.label(RichText::new("Diaporama arrêté").size(16.0).color(TEXT_DIM));
                    ui.label(RichText::new(format!("{} médias prêts", self.files.len())).color(TEXT_DIM));
                }
            });
        });
        ui.add_space(10.0);

        let before = (self.s.clone(), self.s.fit);
        egui::Frame::new().fill(PANEL).corner_radius(4).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("diapo").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Dossier");
                ui.horizontal(|ui| {
                    let txt = self.s.folder.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "aucun".into());
                    ui.label(RichText::new(txt).color(if self.s.folder.is_some() { TEXT } else { TEXT_DIM }));
                    if ui.button("Choisir…").clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.s.folder = Some(d);
                            self.rescan();
                        }
                    }
                    if ui.button("Relire").on_hover_text("Relit le dossier (fichiers ajoutés ou retirés)").clicked() {
                        self.rescan();
                    }
                });
                ui.end_row();

                ui.label("Lecture");
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.s.recursive, "Inclure les sous-dossiers").changed() {
                        self.rescan();
                    }
                    ui.checkbox(&mut self.s.shuffle, "Ordre aléatoire");
                    ui.checkbox(&mut self.s.looping, "Boucler");
                });
                ui.end_row();

                ui.label("Durée des photos");
                ui.add(egui::Slider::new(&mut self.s.photo_seconds, 1.0..=60.0).suffix(" s").step_by(0.5));
                ui.end_row();

                ui.label("Ajustement");
                egui::ComboBox::from_id_salt("fit").selected_text(self.s.fit.label()).width(260.0).show_ui(ui, |ui| {
                    for f in Fit::ALL {
                        ui.selectable_value(&mut self.s.fit, f, f.label());
                    }
                });
                ui.end_row();

                ui.label("Vidéos");
                ui.checkbox(&mut self.s.video_sound, "Avec le son (lues en entier)");
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Durée, ordre et son s'appliquent au prochain lancement ; l'ajustement change tout de suite.").color(TEXT_DIM));
        });
        if self.s.fit != before.1 && running {
            out.send(Command::SetFit { fit: self.s.fit });
        }
        if serde_json::to_string(&self.s).ok() != serde_json::to_string(&before.0).ok() {
            self.save();
        }

        ui.add_space(10.0);
        ui.label(RichText::new(format!("Médias ({})", self.files.len())).strong());
        egui::Frame::new().fill(SLOT_EMPTY).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if self.files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Choisis un dossier de photos et de vidéos").color(TEXT_DIM));
                });
                return;
            }
            let current = out.state.slide.as_ref().map(|s| s.2.clone());
            let row_h = 22.0;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, self.files.len(), |ui, range| {
                for i in range {
                    let p = &self.files[i];
                    let live = current.as_deref() == Some(&*p.to_string_lossy());
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let txt = RichText::new(format!("{:>4}  {}{name}", i + 1, if live { "▶ " } else { "" }));
                    let resp = ui.selectable_label(live, if live { txt.color(GO) } else { txt });
                    // Hors ordre aléatoire, un double-clic saute directement à ce média.
                    if resp.double_clicked() && running && !self.s.shuffle {
                        out.send(Command::Goto { index: i });
                    }
                }
            });
        });
    }
}
