//! Petits widgets communs.

use crate::theme::*;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui};

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

// Repris tels quels de JingleStar (src/ui/widgets.rs).

// ------------------------------------------------------- commandes de playlist

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportIcon {
    Prev,
    Play,
    Pause,
    Stop,
    Next,
}

/// Symbole de transport dessiné à la main, dans un carré de côté `s` centré sur `c`.
fn paint_transport_icon(p: &egui::Painter, c: Pos2, s: f32, icon: TransportIcon, col: Color32) {
    let h = s / 2.0;
    let tri = |pts: Vec<Pos2>| Shape::convex_polygon(pts, col, Stroke::NONE);
    match icon {
        TransportIcon::Play => {
            p.add(tri(vec![pos2(c.x - h * 0.7, c.y - h), pos2(c.x + h * 0.9, c.y), pos2(c.x - h * 0.7, c.y + h)]));
        }
        TransportIcon::Pause => {
            let bar = |x0: f32| Rect::from_min_max(pos2(x0, c.y - h * 0.85), pos2(x0 + h * 0.6, c.y + h * 0.85));
            p.rect_filled(bar(c.x - h * 0.75), CornerRadius::same(1), col);
            p.rect_filled(bar(c.x + h * 0.15), CornerRadius::same(1), col);
        }
        TransportIcon::Stop => {
            p.rect_filled(Rect::from_center_size(c, vec2(s * 0.8, s * 0.8)), CornerRadius::same(1), col);
        }
        TransportIcon::Prev => {
            p.rect_filled(Rect::from_min_max(pos2(c.x - h, c.y - h * 0.8), pos2(c.x - h * 0.65, c.y + h * 0.8)), CornerRadius::ZERO, col);
            p.add(tri(vec![pos2(c.x + h * 0.8, c.y - h * 0.8), pos2(c.x + h * 0.8, c.y + h * 0.8), pos2(c.x - h * 0.5, c.y)]));
        }
        TransportIcon::Next => {
            p.rect_filled(Rect::from_min_max(pos2(c.x + h * 0.65, c.y - h * 0.8), pos2(c.x + h, c.y + h * 0.8)), CornerRadius::ZERO, col);
            p.add(tri(vec![pos2(c.x - h * 0.8, c.y - h * 0.8), pos2(c.x + h * 0.5, c.y), pos2(c.x - h * 0.8, c.y + h * 0.8)]));
        }
    }
}

/// Bouton de la barre de transport : fond plein, symbole et texte facultatif.
pub fn transport_button(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, icon: TransportIcon, label: Option<&str>, fill: Color32, fg: Color32) -> Response {
    let resp = ui.interact(rect, ui.make_persistent_id(id), Sense::click());
    let p = ui.painter_at(rect.expand(1.0));
    let r = CornerRadius::same(4);
    let fill = if resp.is_pointer_button_down_on() {
        fill.gamma_multiply(0.8)
    } else if resp.hovered() {
        fill.gamma_multiply(1.2)
    } else {
        fill
    };
    p.rect_filled(rect, r, fill);
    p.rect_stroke(rect, r, Stroke::new(1.0, Color32::from_black_alpha(110)), StrokeKind::Inside);
    let s = 14.0;
    match label {
        Some(text) => {
            let galley = p.layout_no_wrap(text.to_owned(), FontId::proportional(14.0), fg);
            let total = s + 7.0 + galley.size().x;
            let x0 = rect.center().x - total / 2.0;
            paint_transport_icon(&p, pos2(x0 + s / 2.0, rect.center().y), s, icon, fg);
            p.galley(pos2(x0 + s + 7.0, rect.center().y - galley.size().y / 2.0), galley, fg);
        }
        None => paint_transport_icon(&p, rect.center(), s, icon, fg),
    }
    resp
}

/// Interrupteur de mode avec voyant : ambre et lumineux quand il est actif.
pub fn mode_chip(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, on: bool, label: &str) -> Response {
    let resp = ui.interact(rect, ui.make_persistent_id(id), Sense::click());
    let p = ui.painter_at(rect.expand(1.0));
    let r = CornerRadius::same(4);
    let (fill, stroke, text) = if on {
        (ACCENT.gamma_multiply(0.14), ACCENT.gamma_multiply(0.6), TEXT)
    } else {
        (PANEL_HI, BORDER, TEXT_DIM)
    };
    p.rect_filled(rect, r, if resp.hovered() { fill.gamma_multiply(1.3) } else { fill });
    p.rect_stroke(rect, r, Stroke::new(1.0, stroke), StrokeKind::Inside);
    let galley = p.layout_no_wrap(label.to_owned(), FontId::proportional(13.0), text);
    let total = 7.0 + 6.0 + galley.size().x;
    let x0 = rect.center().x - total / 2.0;
    let led = pos2(x0 + 3.5, rect.center().y);
    if on {
        p.circle_filled(led, 6.0, ACCENT.gamma_multiply(0.18));
        p.circle_filled(led, 3.5, ACCENT);
    } else {
        p.circle_filled(led, 3.5, Color32::from_rgb(0x4a, 0x4a, 0x54));
    }
    p.galley(pos2(x0 + 13.0, rect.center().y - galley.size().y / 2.0), galley, text);
    resp
}

/// Contrôle segmenté : une rangée d'options exclusives. Renvoie l'option cliquée.
pub fn segmented(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug + Copy, labels: &[&str], selected: usize) -> Option<usize> {
    let n = labels.len() as f32;
    let w = rect.width() / n;
    let p = ui.painter_at(rect.expand(1.0));
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + i as f32 * w, rect.top()), vec2(w, rect.height()));
        let resp = ui.interact(r, ui.make_persistent_id((id, i)), Sense::click());
        let corner = CornerRadius {
            nw: if i == 0 { 4 } else { 0 },
            sw: if i == 0 { 4 } else { 0 },
            ne: if i + 1 == labels.len() { 4 } else { 0 },
            se: if i + 1 == labels.len() { 4 } else { 0 },
        };
        let on = i == selected;
        let fill = if on { ACCENT.gamma_multiply(0.55) } else { PANEL_HI };
        p.rect_filled(r, corner, if resp.hovered() && !on { fill.gamma_multiply(1.3) } else { fill });
        if i > 0 {
            p.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, BORDER));
        }
        let col = if on { TEXT } else { TEXT_DIM };
        p.text(r.center(), Align2::CENTER_CENTER, *label, FontId::proportional(12.5), col);
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, BORDER), StrokeKind::Inside);
    clicked
}

/// Vrai à l'image où le bouton gauche s'enfonce sur ce widget : GO, arrêt et jingles partent
/// à l'appui, sans attendre le relâché (un clic dure couramment 80 à 150 ms).
/// Symbole de boucle dessiné (deux flèches qui se suivent sur un anneau arrondi),
/// plus net à petite taille qu'un emoji.
pub fn loop_icon(p: &egui::Painter, r: Rect, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let rad = r.height() / 2.0;
    let (cl, cr) = (pos2(r.left() + rad, r.center().y), pos2(r.right() - rad, r.center().y));
    // Demi-cercles aux extrémités, puis les deux côtés droits interrompus par les flèches.
    let arc = |c: Pos2, from: f32| -> Vec<Pos2> {
        (0..=8).map(|i| {
            let a = from + std::f32::consts::PI * i as f32 / 8.0;
            c + vec2(a.cos(), a.sin()) * rad
        }).collect()
    };
    p.add(egui::Shape::line(arc(cr, -std::f32::consts::FRAC_PI_2), stroke));
    p.add(egui::Shape::line(arc(cl, std::f32::consts::FRAC_PI_2), stroke));
    let (top, bottom) = (r.top(), r.bottom());
    p.line_segment([pos2(cl.x, top), pos2(cr.x - 1.0, top)], stroke);
    p.line_segment([pos2(cr.x, bottom), pos2(cl.x + 1.0, bottom)], stroke);
    let head = 3.0;
    // Flèche vers la droite en haut, vers la gauche en bas.
    let tip = pos2(r.center().x + 2.0, top);
    p.add(egui::Shape::convex_polygon(vec![tip + vec2(head, 0.0), tip + vec2(-head * 0.6, -head), tip + vec2(-head * 0.6, head)], color, Stroke::NONE));
    let tip = pos2(r.center().x - 2.0, bottom);
    p.add(egui::Shape::convex_polygon(vec![tip + vec2(-head, 0.0), tip + vec2(head * 0.6, head), tip + vec2(head * 0.6, -head)], color, Stroke::NONE));
}

/// Bouton « Ajouter » : un « + » orange épais dessiné à gauche du libellé, cadre orange.
pub fn add_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let col = if enabled { ACCENT } else { TEXT_DIM };
    let text = egui::RichText::new(format!("      {label}")).size(14.0);
    let resp = ui.add_enabled(enabled, egui::Button::new(text).stroke(Stroke::new(1.0, col.gamma_multiply(0.6))));
    let c = pos2(resp.rect.left() + 15.0, resp.rect.center().y);
    let stroke = Stroke::new(3.0, col);
    ui.painter().line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], stroke);
    ui.painter().line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], stroke);
    resp
}

pub fn pressed(ui: &Ui, resp: &Response) -> bool {
    resp.hovered() && ui.input(|i| i.pointer.primary_pressed())
}


/// Ligne de la liste des morceaux : numéro et nom à gauche (tronqué), place réservée au gain à droite
/// (dessiné ensuite par l'appelant, à la position renvoyée).
pub fn track_row(ui: &mut Ui, label: &str, color: Color32, selected: bool, gain: Option<&str>) -> (Response, Pos2) {
    // Glissable : un morceau peut être déposé sur un bouton de jingle.
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.max_rect().width().min(ui.available_width()), 20.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, CornerRadius::same(3), ACCENT.gamma_multiply(0.18));
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(3), PANEL_HI);
    }
    let font = FontId::monospace(12.0);
    let anchor = pos2(rect.right() - 6.0, rect.center().y);
    let mut right = anchor.x;
    if let Some(txt) = gain {
        right -= p.layout_no_wrap(txt.to_owned(), FontId::monospace(11.0), TEXT).size().x + 8.0;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - rect.left() - 6.0).max(10.0));
    let g = p.layout_job(job);
    p.galley(pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0), g, color);
    (resp, anchor)
}

/// Le logo des applis Star : la grille de pads seule.
pub fn logo_mark(ui: &mut Ui, height: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(height, height), Sense::hover());
    logo_grid(ui.painter(), rect);
    resp
}

fn logo_grid(p: &egui::Painter, rect: Rect) {
    const STRIPS: [Color32; 8] = [
        Color32::from_rgb(0x4a, 0x8f, 0xdb),
        Color32::from_rgb(0x4f, 0xae, 0x58),
        Color32::from_rgb(0xe2, 0xb2, 0x32),
        Color32::from_rgb(0xe2, 0x7a, 0x30),
        Color32::from_rgb(0x9b, 0x6b, 0xd3),
        Color32::from_rgb(0x3f, 0xb8, 0xaf),
        Color32::from_rgb(0xd9, 0x53, 0x4f),
        Color32::from_rgb(0x8a, 0x8a, 0x90),
    ];
    // Grille 3×3 dessinée dans un carré de 100 unités, comme le fichier SVG.
    let u = rect.height() / 100.0;
    let at = |x: f32, y: f32| rect.min + vec2(x * u, y * u);
    let mut k = 0;
    for row in 0..3 {
        for col in 0..3 {
            let pad = Rect::from_min_size(at(4.0 + col as f32 * 32.0, 4.0 + row as f32 * 32.0), vec2(28.0 * u, 28.0 * u));
            let r = CornerRadius::same((4.5 * u).round().max(1.0) as u8);
            if row == 1 && col == 1 {
                p.rect_filled(pad, r, ACCENT);
                star_jingle(p, pad.center() + vec2(0.0, 0.6 * u), 11.5 * u, BG);
            } else {
                // À cette taille, des pads pleins se lisent mieux que les bandeaux du grand logo.
                p.rect_filled(pad, r, STRIPS[k]);
                k += 1;
            }
        }
    }
}

/// Étoile à cinq branches pleine (égui ne remplit que les formes convexes :
/// on la compose d'un pentagone central et de cinq triangles).
fn star_jingle(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let pt = |i: usize, rad: f32| {
        let a = (-90.0 + 36.0 * i as f32).to_radians();
        c + vec2(a.cos(), a.sin()) * rad
    };
    let inner: Vec<Pos2> = (0..5).map(|i| pt(2 * i + 1, r * 0.45)).collect();
    p.add(Shape::convex_polygon(inner.clone(), color, Stroke::NONE));
    for i in 0..5 {
        let tip = pt(2 * i, r);
        p.add(Shape::convex_polygon(vec![inner[(i + 4) % 5], tip, inner[i]], color, Stroke::NONE));
    }
}
