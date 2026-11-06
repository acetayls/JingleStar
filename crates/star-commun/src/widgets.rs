//! Petits widgets communs.

use crate::theme::*;
use egui::{vec2, Color32, FontId, RichText, Sense, Stroke};

/// Logotype « NomStar » : le nom en clair, « Star » en accent, précédé d'une étoile dessinée.
pub fn logo(ui: &mut egui::Ui, name: &str, height: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(height, height), Sense::hover());
    star(ui.painter(), rect.center(), height * 0.48, ACCENT);
    ui.add_space(4.0);
    let font = FontId::new(height * 0.9, logo_family());
    let r = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(RichText::new(name).font(font.clone()).color(TEXT));
        ui.label(RichText::new("Star").font(font).color(ACCENT));
    });
    resp.union(r.response)
}

/// Étoile à cinq branches pleine.
pub fn star(p: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    let pts: Vec<egui::Pos2> = (0..10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let rr = if i % 2 == 0 { r } else { r * 0.42 };
            c + vec2(a.cos() * rr, a.sin() * rr)
        })
        .collect();
    // Polygone concave : on le dessine en triangles depuis le centre.
    for i in 0..10 {
        let mesh_pts = vec![c, pts[i], pts[(i + 1) % 10]];
        p.add(egui::Shape::convex_polygon(mesh_pts, color, Stroke::NONE));
    }
}

/// Gros bouton coloré (GO, STOP…).
pub fn big_button(ui: &mut egui::Ui, text: &str, fill: Color32, size: egui::Vec2) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).strong().size(16.0).color(PLAYING_INK)).fill(fill).min_size(size))
}

/// Pastille d'état : un rond de couleur et un texte.
pub fn status_dot(ui: &mut egui::Ui, color: Color32, text: impl Into<String>) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, color);
        ui.label(RichText::new(text.into()).color(TEXT));
    })
    .response
}

/// Durée en m:ss (ou h:mm:ss).
pub fn fmt_time(s: f64) -> String {
    if !s.is_finite() || s < 0.0 {
        return "--:--".into();
    }
    let t = s as u64;
    let (h, m, sec) = (t / 3600, (t / 60) % 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}
