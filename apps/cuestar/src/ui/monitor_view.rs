//! Fenêtre flottante de monitoring : retour de l'image envoyée sur la sortie, à la cadence
//! de la vidéo, dans un cadre noir au format de l'écran de sortie.

use crate::link::OutputLink;
use crate::protocol::Command;
use egui::{pos2, vec2, Color32, Rect, RichText};
use star_commun::theme::*;
use star_commun::widgets::fmt_time;
use std::path::Path;

#[derive(Default)]
pub struct MonitorView {
    pub open: bool,
    texture: Option<egui::TextureHandle>,
    serial: u64,
}

impl MonitorView {
    pub fn toggle(&mut self, out: &mut OutputLink) {
        self.open = !self.open;
        self.sync(out);
    }

    /// Dit à la sortie s'il faut envoyer des images (aussi après une relance de la sortie).
    pub fn sync(&self, out: &mut OutputLink) {
        out.send(Command::Monitor { on: self.open });
    }

    pub fn show(&mut self, ctx: &egui::Context, out: &mut OutputLink) {
        if !self.open {
            return;
        }
        // Nouvelle image ? On remplace la texture.
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
        // Vraie fenêtre du système : elle peut sortir de la régie (autre écran, par exemple).
        let vp = egui::ViewportBuilder::default().with_title("CueStar — Monitoring").with_inner_size([480.0, 300.0]).with_min_inner_size([160.0, 110.0]);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("cuestar-monitoring"), vp, |ui, _| {
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                open = false;
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(6)).show(ui, |ui| {
                let s = &out.state;
                // Cadre noir au format de l'écran de sortie, centré dans la place disponible.
                let screen = s.monitor.as_ref().filter(|m| m.width > 0 && m.height > 0).map(|m| m.height as f32 / m.width as f32).unwrap_or(9.0 / 16.0);
                let avail = ui.available_rect_before_wrap();
                let area = Rect::from_min_max(avail.min, pos2(avail.right(), (avail.bottom() - 24.0).max(avail.top() + 20.0)));
                let w = area.width().min(area.height() / screen);
                let frame = Rect::from_center_size(area.center(), vec2(w, w * screen));
                ui.painter().rect_filled(frame, 0, Color32::BLACK);
                if let Some(t) = &self.texture {
                    // L'image garde son propre format à l'intérieur du cadre.
                    let ts = t.size_vec2();
                    let k = (frame.width() / ts.x).min(frame.height() / ts.y);
                    let r = Rect::from_center_size(frame.center(), ts * k);
                    ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                ui.allocate_rect(area, egui::Sense::hover());
                ui.horizontal(|ui| {
                    let name = s
                        .current
                        .as_ref()
                        .filter(|p| !p.starts_with("av://"))
                        .and_then(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_else(|| "—".into());
                    let col = if s.current.is_some() { if s.paused { ACCENT } else { GO } } else { TEXT_DIM };
                    ui.label(RichText::new("●").color(col));
                    ui.add(egui::Label::new(name).truncate());
                    if s.duration > 0.0 {
                        ui.label(RichText::new(format!("{} / {}", fmt_time(s.time), fmt_time(s.duration))).monospace().color(TEXT_DIM));
                    }
                });
            });
        });
        if open != self.open {
            self.open = open;
            self.sync(out);
        }
    }
}
