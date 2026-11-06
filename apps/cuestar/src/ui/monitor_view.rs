//! Fenêtre flottante de monitoring : retour de l'image envoyée sur la sortie.

use crate::link::OutputLink;
use crate::protocol::Command;
use egui::{vec2, RichText};
use star_commun::theme::*;
use star_commun::widgets::fmt_time;
use std::path::Path;

pub struct MonitorView {
    pub open: bool,
    fps: u32,
    texture: Option<egui::TextureHandle>,
    serial: u64,
}

impl Default for MonitorView {
    fn default() -> Self {
        Self { open: false, fps: 10, texture: None, serial: 0 }
    }
}

impl MonitorView {
    pub fn toggle(&mut self, out: &mut OutputLink) {
        self.open = !self.open;
        self.sync(out);
    }

    /// Dit à la sortie s'il faut envoyer des vignettes (aussi après une relance de la sortie).
    pub fn sync(&self, out: &mut OutputLink) {
        out.send(Command::Monitor { fps: if self.open { self.fps } else { 0 } });
    }

    pub fn show(&mut self, ctx: &egui::Context, out: &mut OutputLink) {
        if !self.open {
            return;
        }
        // Nouvelle vignette ? On remplace la texture.
        if let Ok(f) = out.monitor.lock() {
            if f.serial != self.serial && f.w > 0 {
                self.serial = f.serial;
                let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba);
                match &mut self.texture {
                    Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                    None => self.texture = Some(ctx.load_texture("monitoring", img, egui::TextureOptions::LINEAR)),
                }
            }
        }
        let mut open = self.open;
        let mut fps_changed = false;
        // Vraie fenêtre du système : elle peut sortir de la régie (autre écran, par exemple).
        let vp = egui::ViewportBuilder::default().with_title("CueStar — Monitoring").with_inner_size([480.0, 340.0]).with_min_inner_size([200.0, 150.0]);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("cuestar-monitoring"), vp, |ui, _| {
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                open = false;
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(8)).show(ui, |ui| {
                let aspect = self.texture.as_ref().map(|t| t.size_vec2()).map(|s| s.y / s.x.max(1.0)).unwrap_or(9.0 / 16.0);
                let w = ui.available_width().min((ui.available_height() - 56.0).max(40.0) / aspect).max(80.0);
                let size = vec2(w, w * aspect);
                match &self.texture {
                    Some(t) => {
                        ui.add(egui::Image::new((t.id(), size)));
                    }
                    None => {
                        let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        ui.painter().rect_filled(r, 2, egui::Color32::BLACK);
                    }
                }
                let s = &out.state;
                ui.horizontal(|ui| {
                    let name = s
                        .current
                        .as_ref()
                        .filter(|p| !p.starts_with("av://"))
                        .and_then(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_else(|| "—".into());
                    let col = if s.current.is_some() { if s.paused { ACCENT } else { GO } } else { TEXT_DIM };
                    ui.label(RichText::new("●").color(col));
                    ui.label(name);
                    if s.duration > 0.0 {
                        ui.label(RichText::new(format!("{} / {}", fmt_time(s.time), fmt_time(s.duration))).monospace().color(TEXT_DIM));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Images/s").color(TEXT_DIM));
                    fps_changed = ui.add(egui::Slider::new(&mut self.fps, 1..=25)).changed();
                });
            });
        });
        if fps_changed {
            self.sync(out);
        }
        if open != self.open {
            self.open = open;
            self.sync(out);
        }
    }
}
