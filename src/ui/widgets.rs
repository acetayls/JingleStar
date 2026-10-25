//! Widgets dessinés à la main : boutons de jingle, vumètres, éditeur de forme d'onde.

use std::ops::RangeInclusive;

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Key, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui,
};

use super::theme::*;
use crate::i18n::tr;

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Temps au dixième de seconde, pour les jingles : 0:03.4
pub fn fmt_tenths(secs: f64) -> String {
    let t = (secs.max(0.0) * 10.0).floor() / 10.0;
    format!("{}:{:04.1}", (t / 60.0) as u64, t % 60.0)
}

/// Volume en dB pour l'affichage : « +1.5 dB », « -∞ dB » sous le plancher.
pub fn fmt_db(db: f32, floor: f32) -> String {
    if db <= floor {
        "-∞ dB".into()
    } else if db.abs() < 0.05 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB")
    }
}

pub fn gain_to_db(g: f32) -> f32 {
    if g <= 1e-6 {
        -120.0
    } else {
        20.0 * g.log10()
    }
}

/// Lit une valeur en dB tapée au clavier : « -6 », « +3,5 », « -4.5 dB », « -inf ».
/// Sans signe, la valeur est négative (« 6 » donne -6 dB) : on baisse bien plus souvent
/// qu'on ne monte ; « +3 » pour monter. `-inf` donne le bas de la plage ; la valeur est
/// ramenée dans la plage.
pub fn parse_db(text: &str, range: &RangeInclusive<f32>) -> Option<f32> {
    let t = text.trim().to_lowercase().replace(',', ".").replace(' ', "");
    let t = t.strip_suffix("db").unwrap_or(&t);
    let v = match t {
        "-inf" | "-∞" | "inf" | "∞" => *range.start(),
        _ => {
            let v = t.parse::<f32>().ok().filter(|v| v.is_finite())?;
            if t.starts_with('+') || t.starts_with('-') { v } else { -v.abs() }
        }
    };
    Some(v.clamp(*range.start(), *range.end()))
}

/// Valeur en dB affichée en texte. Un clic ouvre un champ de saisie :
/// Entrée valide, Échap (ou un clic ailleurs) annule. Renvoie la valeur validée.
#[allow(clippy::too_many_arguments)]
pub fn db_value(
    ui: &mut Ui,
    id: Id,
    anchor: Pos2,
    align: Align2,
    db: f32,
    floor: f32,
    range: RangeInclusive<f32>,
    font: FontId,
    color: Color32,
) -> Option<f32> {
    db_value_shown(ui, id, anchor, align, db, floor, range, font, color, fmt_db(db, floor), None)
}

/// Comme `db_value`, avec un texte affiché au choix (« … », « ⚠ ») et une bulle d'aide en plus.
#[allow(clippy::too_many_arguments)]
pub fn db_value_shown(
    ui: &mut Ui,
    id: Id,
    anchor: Pos2,
    align: Align2,
    db: f32,
    floor: f32,
    range: RangeInclusive<f32>,
    font: FontId,
    color: Color32,
    text: String,
    tip: Option<String>,
) -> Option<f32> {
    let editing: Option<String> = ui.data(|d| d.get_temp(id));
    if let Some(mut buf) = editing {
        let w = ui.painter().layout_no_wrap("-60.0 dB".into(), font.clone(), color).size().x + 10.0;
        let rect = align.anchor_size(anchor, vec2(w, font.size + 6.0));
        let edit_id = id.with("edit");
        // `place` : le champ se pose sur la valeur sans décaler la mise en page.
        let resp = ui.place(
            rect,
            egui::TextEdit::singleline(&mut buf)
                .id(edit_id)
                .font(font)
                .align(Align2::CENTER_CENTER)
                .margin(vec2(2.0, 1.0)),
        );
        let first = !ui.data(|d| d.get_temp::<bool>(id.with("focused")).unwrap_or(false));
        if first {
            if let Some(mut st) = egui::text_edit::TextEditState::load(ui.ctx(), edit_id) {
                st.cursor.set_char_range(Some(egui::text::CCursorRange::select_all_str(&buf)));
                st.store(ui.ctx(), edit_id);
            }
            ui.data_mut(|d| d.insert_temp(id.with("focused"), true));
        }
        if !ui.memory(|m| m.has_focus(edit_id)) && !resp.lost_focus() {
            resp.request_focus();
        }
        // Entrée valide ; Échap, la perte du focus ou un clic hors du champ annulent.
        let (enter, escape, clicked_out) = ui.input(|i| {
            let out = i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p));
            (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape), out)
        });
        if enter || escape || clicked_out || (!first && resp.lost_focus()) {
            ui.data_mut(|d| {
                d.remove::<String>(id);
                d.remove::<bool>(id.with("focused"));
            });
            if resp.has_focus() || ui.memory(|m| m.has_focus(edit_id)) {
                ui.memory_mut(|m| m.surrender_focus(edit_id));
            }
            return if enter { parse_db(&buf, &range) } else { None };
        }
        ui.data_mut(|d| d.insert_temp(id, buf));
        return None;
    }
    let galley = ui.painter().layout_no_wrap(text.clone(), font.clone(), color);
    let rect = align.anchor_size(anchor, galley.size()).expand2(vec2(3.0, 1.0));
    let resp = ui.interact(rect, id, Sense::click());
    ui.painter().galley(rect.min + vec2(3.0, 1.0), galley, color);
    if resp.hovered() {
        ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, Color32::from_white_alpha(40)), StrokeKind::Inside);
    }
    let hint = tr(
        "Clic pour saisir une valeur (sans signe = négatif : 6 donne -6 dB, +3 pour monter)",
        "Click to type a value (no sign = negative: 6 gives -6 dB, +3 to go up)",
    );
    let resp = resp.on_hover_text(match tip {
        Some(t) => format!("{t}\n\n{hint}"),
        None => hint.to_string(),
    });
    if resp.clicked() || resp.double_clicked() {
        let start = if db <= floor { "-inf".to_string() } else { format!("{db:.1}") };
        ui.data_mut(|d| d.insert_temp(id, start));
    }
    None
}

/// Repères de niveau des faders et curseurs de volume.
pub const DB_MARKS: [f32; 3] = [3.0, 0.0, -6.0];

/// Position d'une valeur le long d'un curseur egui de `rect` (même marge que egui : un rayon de poignée).
pub fn slider_value_pos(rect: Rect, vertical: bool, range: &RangeInclusive<f32>, v: f32) -> f32 {
    let t = ((v - range.start()) / (range.end() - range.start())).clamp(0.0, 1.0);
    if vertical {
        let r = rect.width() / 2.5;
        egui::lerp((rect.bottom() - r)..=(rect.top() + r), t)
    } else {
        let r = rect.height() / 2.5;
        egui::lerp((rect.left() + r)..=(rect.right() - r), t)
    }
}

/// Petits traits aux repères de niveau (0 dB plus marqué), le long d'un curseur.
pub fn db_marks(ui: &Ui, rect: Rect, vertical: bool, range: &RangeInclusive<f32>, labels: bool) {
    let p = ui.painter();
    for m in DB_MARKS {
        if !range.contains(&m) {
            continue;
        }
        let zero = m == 0.0;
        let col = if zero { Color32::from_white_alpha(150) } else { Color32::from_white_alpha(80) };
        let len = if zero { 7.0 } else { 5.0 };
        let at = slider_value_pos(rect, vertical, range, m);
        let txt = if zero { "0".to_string() } else { format!("{m:+}") };
        if vertical {
            // Trait à droite du rail, valeur à gauche (le rail est au centre).
            let rail = ui.spacing().slider_rail_height / 2.0 + 2.0;
            let c = rect.center().x;
            p.line_segment([pos2(c + rail, at), pos2(c + rail + len, at)], Stroke::new(1.0, col));
            if !labels {
                p.line_segment([pos2(c - rail - len, at), pos2(c - rail, at)], Stroke::new(1.0, col));
            } else {
                p.text(pos2(c - rail + 1.0, at), Align2::RIGHT_CENTER, txt, FontId::monospace(8.0), col);
            }
        } else {
            p.line_segment([pos2(at, rect.bottom() - len), pos2(at, rect.bottom())], Stroke::new(1.0, col));
            if labels {
                p.text(pos2(at, rect.top()), Align2::CENTER_BOTTOM, txt, FontId::monospace(8.0), col);
            }
        }
    }
}

/// Vumètre stéréo vertical, échelle -60..0 dB.
pub fn meter(ui: &Ui, rect: Rect, db: [f32; 2]) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(2), Color32::from_rgb(0x12, 0x12, 0x14));
    let w = (rect.width() - 3.0) / 2.0;
    let frac = |d: f32| ((d + 60.0) / 60.0).clamp(0.0, 1.0);
    for (c, d) in db.iter().enumerate() {
        let x0 = rect.left() + 1.0 + c as f32 * (w + 1.0);
        let bar = Rect::from_min_max(pos2(x0, rect.top() + 1.0), pos2(x0 + w, rect.bottom() - 1.0));
        let h = bar.height();
        let level = frac(*d);
        // Segments vert / jaune / rouge.
        for (lo, hi, col) in [(0.0, frac(-12.0), METER_LOW), (frac(-12.0), frac(-3.0), METER_MID), (frac(-3.0), 1.0, METER_HIGH)] {
            let top = level.min(hi);
            if top > lo {
                let r = Rect::from_min_max(
                    pos2(bar.left(), bar.bottom() - top * h),
                    pos2(bar.right(), bar.bottom() - lo * h),
                );
                p.rect_filled(r, CornerRadius::ZERO, col);
            }
        }
    }
    // Graduations.
    for d in [-6.0, -12.0, -24.0, -40.0] {
        let y = rect.bottom() - frac(d) * rect.height();
        p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, Color32::from_black_alpha(140)));
    }
}

pub struct SlotView<'a> {
    pub index: usize,
    pub name: Option<&'a str>,
    pub color: [u8; 3],
    pub playing: bool,
    /// Lecture en cours : progression (0..1), temps écoulé et temps restant.
    pub progress: Option<(f32, String, String)>,
    /// À l'arrêt : durée de la zone jouée.
    pub duration_label: Option<String>,
    pub loading: bool,
    pub error: bool,
    pub looping: bool,
    pub selected: bool,
    pub edit_mode: bool,
    pub drop_hover: bool,
    /// Égaliseur actif sur ce jingle.
    pub eq: bool,
}

pub fn jingle_button(ui: &mut Ui, rect: Rect, v: &SlotView) -> Response {
    let resp = ui.interact(rect, ui.make_persistent_id(("jingle", v.index)), Sense::click());
    let p = ui.painter_at(rect.expand(2.0));
    let r = CornerRadius::same(5);
    let hovered = resp.hovered();

    if v.name.is_none() {
        p.rect_filled(rect, r, SLOT_EMPTY);
        p.rect_stroke(rect, r, Stroke::new(1.0, if hovered { BORDER.gamma_multiply(1.6) } else { BORDER }), StrokeKind::Inside);
        let label = if v.edit_mode { "+" } else { "" };
        p.text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(22.0), TEXT_DIM);
    } else {
        p.rect_filled(rect, r, slot_fill(v.color, v.playing));
        // Bandeau de couleur en haut.
        let band = Rect::from_min_size(rect.min, vec2(rect.width(), 4.0));
        p.rect_filled(band, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, rgb(v.color));

        let name = v.name.unwrap_or_default();
        let galley = p.layout(name.to_owned(), FontId::proportional(14.0), TEXT, rect.width() - 12.0);
        let gpos = rect.center() - galley.size() / 2.0 - vec2(0.0, 8.0);
        p.galley(gpos, galley, TEXT);

        // Barre de lecture, temps écoulé à gauche et restant à droite.
        let bar = Rect::from_min_max(
            pos2(rect.left() + 7.0, rect.bottom() - 25.0),
            pos2(rect.right() - 7.0, rect.bottom() - 21.0),
        );
        p.rect_filled(bar, CornerRadius::same(2), Color32::from_black_alpha(110));
        let text_y = rect.bottom() - 5.0;
        let font = FontId::monospace(11.0);
        if let Some((prog, elapsed, remaining)) = &v.progress {
            let fill = Rect::from_min_size(bar.min, vec2(bar.width() * prog.clamp(0.0, 1.0), bar.height()));
            p.rect_filled(fill, CornerRadius::same(2), GO);
            // Le temps restant prime : sur un bouton étroit, il s'affiche seul, centré.
            let both = p.layout_no_wrap(format!("{elapsed} {remaining}"), font.clone(), Color32::WHITE).size().x + 8.0;
            if both <= bar.width() {
                p.text(pos2(bar.left(), text_y), Align2::LEFT_BOTTOM, elapsed, font.clone(), TEXT);
                p.text(pos2(bar.right(), text_y), Align2::RIGHT_BOTTOM, remaining, FontId::monospace(12.0), Color32::WHITE);
            } else {
                p.text(pos2(bar.center().x, text_y), Align2::CENTER_BOTTOM, remaining, FontId::monospace(12.0), Color32::WHITE);
            }
        } else if let Some(d) = &v.duration_label {
            p.text(pos2(rect.center().x, text_y), Align2::CENTER_BOTTOM, d, font, TEXT_DIM);
        }
        // Le volume en haut à droite est dessiné (et modifiable) par `jingle_volume`.
        // En haut à gauche, après le numéro : boucle et égaliseur, bien visibles.
        let mut x = rect.left() + 6.0 + p.layout_no_wrap(format!("{}", v.index + 1), FontId::monospace(11.0), TEXT).size().x + 6.0;
        if v.looping {
            let badge = Rect::from_min_size(pos2(x, rect.top() + 6.0), vec2(24.0, 15.0));
            p.rect_filled(badge, CornerRadius::same(2), ACCENT);
            loop_icon(&p, badge.shrink2(vec2(4.0, 3.0)), Color32::BLACK);
            x = badge.right() + 4.0;
        }
        if v.eq {
            let g = p.layout_no_wrap("EQ".into(), FontId::proportional(10.0), Color32::BLACK);
            let badge = Rect::from_min_size(pos2(x, rect.top() + 7.0), g.size() + vec2(6.0, 2.0));
            p.rect_filled(badge, CornerRadius::same(2), ACCENT);
            p.galley(badge.min + vec2(3.0, 1.0), g, Color32::BLACK);
        }
        if v.loading {
            p.text(rect.right_bottom() - vec2(6.0, 5.0), Align2::RIGHT_BOTTOM, "…", FontId::proportional(14.0), TEXT_DIM);
        }
        if v.error {
            p.text(rect.right_bottom() - vec2(6.0, 5.0), Align2::RIGHT_BOTTOM, "⚠", FontId::proportional(14.0), STOP);
        }
        let stroke = if v.playing {
            // Cadre vert épais et halo : le jingle qui joue se repère de loin.
            p.rect_stroke(rect.expand(1.5), CornerRadius::same(6), Stroke::new(3.0, GO.gamma_multiply(0.35)), StrokeKind::Outside);
            Stroke::new(3.0, GO)
        } else if hovered {
            Stroke::new(1.0, Color32::from_white_alpha(90))
        } else {
            Stroke::new(1.0, Color32::from_black_alpha(120))
        };
        p.rect_stroke(rect, r, stroke, StrokeKind::Inside);
    }
    p.text(rect.left_top() + vec2(6.0, 7.0), Align2::LEFT_TOP, format!("{}", v.index + 1), FontId::monospace(11.0), TEXT_DIM);
    if v.selected {
        p.rect_stroke(rect.expand(1.0), r, Stroke::new(2.0, ACCENT), StrokeKind::Outside);
    }
    if v.drop_hover {
        p.rect_stroke(rect, r, Stroke::new(2.0, EDIT), StrokeKind::Inside);
    }
    resp
}

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

pub fn pressed(ui: &Ui, resp: &Response) -> bool {
    resp.hovered() && ui.input(|i| i.pointer.primary_pressed())
}

/// Volume d'un jingle en haut à droite de son bouton, en couleur s'il n'est pas à 0 dB ;
/// double-clic pour le saisir. À appeler après `jingle_button`, pour passer au-dessus du bouton.
pub fn jingle_volume(ui: &mut Ui, id: Id, rect: Rect, volume_db: f32) -> Option<f32> {
    let col = if volume_db.abs() < 0.05 { TEXT_DIM } else { ACCENT };
    db_value(ui, id, rect.right_top() + vec2(-6.0, 7.0), Align2::RIGHT_TOP, volume_db, -40.0, -40.0..=10.0, FontId::monospace(10.0), col)
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

/// Marge horizontale de la forme d'onde, pour saisir les poignées aux extrémités.
const HANDLE_PAD: f32 = 14.0;

/// Points d'édition d'un jingle, en secondes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Markers {
    pub start: f64,
    pub loop_start: f64,
    pub loop_end: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Handle {
    Start,
    LoopStart,
    LoopEnd,
    End,
    FadeIn,
    FadeOut,
}

/// Éditeur façon QLab : poignées de début et de fin, traits de début et de fin de boucle,
/// rampes de fondu avec leurs poignées en haut. Renvoie `true` si un point ou un fondu a été déplacé.
/// Le fondu de sortie n'a de poignée qu'en lecture unique (en boucle, il ne sert qu'à l'arrêt).
#[allow(clippy::too_many_arguments)]
pub fn waveform_editor(
    ui: &mut Ui,
    peaks: impl Fn(f64, f64, usize) -> Vec<f32>,
    duration: f64,
    m: &mut Markers,
    looping: bool,
    fade_in: &mut f64,
    fade_out: &mut f64,
    playhead: Option<f64>,
) -> bool {
    let size = vec2(ui.available_width(), 150.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let wave = rect.shrink2(vec2(HANDLE_PAD, 14.0));
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(0x12, 0x12, 0x14));
    if duration <= 0.0 {
        return false;
    }

    // Zoom : partie visible (v0..v1 s), gardée d'une image à l'autre pour ce son.
    let view_id = resp.id.with("view");
    let (mut v0, mut v1) = ui
        .data(|d| d.get_temp::<(f64, f64, f64)>(view_id))
        .filter(|v| v.2 == duration)
        .map(|v| (v.0, v.1))
        .unwrap_or((0.0, duration));
    let min_span = (duration / 2000.0).max(0.002).min(duration);
    if let Some(hover) = resp.hover_pos() {
        // Molette (ou Ctrl + molette, pincement) : zoom autour du pointeur. Défilement horizontal : déplacement.
        let (scroll, zoom) = ui.input_mut(|i| {
            let d = (i.smooth_scroll_delta, i.zoom_delta());
            i.smooth_scroll_delta = egui::Vec2::ZERO;
            d
        });
        let span = v1 - v0;
        let factor = (-(scroll.y as f64) * 0.004).exp() / zoom as f64;
        let new_span = (span * factor).clamp(min_span, duration);
        let anchor = v0 + ((hover.x - wave.left()) / wave.width()).clamp(0.0, 1.0) as f64 * span;
        v0 = anchor - (anchor - v0) * new_span / span - scroll.x as f64 / wave.width() as f64 * new_span;
        v1 = v0 + new_span;
    }
    let clamp_view = |v0: f64, v1: f64| {
        let span = (v1 - v0).clamp(min_span, duration);
        let v0 = v0.clamp(0.0, duration - span);
        (v0, v0 + span)
    };
    (v0, v1) = clamp_view(v0, v1);
    let zoomed = v1 - v0 < duration * 0.999;
    let x_of = |t: f64| wave.left() + ((t - v0) / (v1 - v0)) as f32 * wave.width();
    let t_of = |x: f32| (v0 + ((x - wave.left()) / wave.width()) as f64 * (v1 - v0)).clamp(0.0, duration);
    let clip_x = |x: f32| x.clamp(rect.left(), rect.right());

    // Zone de boucle.
    if looping {
        let lr = Rect::from_x_y_ranges(clip_x(x_of(m.loop_start))..=clip_x(x_of(m.loop_end)), wave.y_range());
        p.rect_filled(lr, CornerRadius::ZERO, ACCENT.gamma_multiply(0.12));
    }

    // Forme d'onde.
    let mid = wave.center().y;
    let cols = wave.width().max(1.0) as usize;
    // Les fondus se voient sur la forme d'onde : l'amplitude jouée est dessinée avec le gain
    // du fondu (linéaire, comme le mixeur), le son d'origine reste en filigrane derrière.
    let fade_gain = |t: f64| {
        let mut g = 1.0;
        if *fade_in > 0.0 && t < m.start + *fade_in {
            g *= ((t - m.start) / *fade_in).clamp(0.0, 1.0);
        }
        if !looping && *fade_out > 0.0 && t > m.end - *fade_out {
            g *= ((m.end - t) / *fade_out).clamp(0.0, 1.0);
        }
        g as f32
    };
    for (c, peak) in peaks(v0, v1, cols).into_iter().enumerate() {
        let x = wave.left() + c as f32 + 0.5;
        let t = t_of(x);
        let inside = t >= m.start && t <= m.end;
        let col = if inside { Color32::from_rgb(0x7f, 0xb8, 0xe8) } else { Color32::from_rgb(0x3a, 0x48, 0x55) };
        let h = peak.min(1.0) * wave.height() / 2.0;
        if inside {
            let g = fade_gain(t);
            if g < 1.0 {
                p.line_segment([pos2(x, mid - h), pos2(x, mid + h)], Stroke::new(1.0, col.gamma_multiply(0.22)));
            }
            let h = h * g;
            p.line_segment([pos2(x, mid - h), pos2(x, mid + h)], Stroke::new(1.0, col));
        } else {
            p.line_segment([pos2(x, mid - h), pos2(x, mid + h)], Stroke::new(1.0, col));
        }
    }

    // Parties exclues assombries.
    let shade = Color32::from_black_alpha(140);
    p.rect_filled(Rect::from_x_y_ranges(rect.left()..=clip_x(x_of(m.start)), rect.y_range()), CornerRadius::ZERO, shade);
    p.rect_filled(Rect::from_x_y_ranges(clip_x(x_of(m.end))..=rect.right(), rect.y_range()), CornerRadius::ZERO, shade);

    // Rampes de fondu (linéaires, comme dans le mixeur) : partie atténuée voilée, poignée carrée en haut.
    let span = (m.end - m.start).max(0.0);
    *fade_in = fade_in.clamp(0.0, span);
    *fade_out = fade_out.clamp(0.0, span);
    let fi_x = x_of(m.start + *fade_in);
    let fo_x = x_of(m.end - *fade_out);
    let ramp = Stroke::new(1.5, Color32::from_rgb(0xe8, 0xe8, 0xe8));
    let veil = Color32::from_black_alpha(110);
    if *fade_in > 0.0 {
        let (a, b) = (pos2(x_of(m.start), wave.bottom()), pos2(fi_x, wave.top()));
        p.add(Shape::convex_polygon(vec![pos2(a.x, wave.top()), b, a], veil, Stroke::NONE));
        p.line_segment([a, b], ramp);
    }
    if *fade_out > 0.0 && !looping {
        let (a, b) = (pos2(fo_x, wave.top()), pos2(x_of(m.end), wave.bottom()));
        p.add(Shape::convex_polygon(vec![a, pos2(b.x, wave.top()), b], veil, Stroke::NONE));
        p.line_segment([a, b], ramp);
    }
    let mut fade_handles = vec![(Handle::FadeIn, fi_x, *fade_in)];
    if !looping {
        fade_handles.push((Handle::FadeOut, fo_x, *fade_out));
    }
    for &(_, x, secs) in &fade_handles {
        let sq = Rect::from_center_size(pos2(x, rect.top() + 7.0), vec2(9.0, 9.0));
        p.rect_filled(sq, CornerRadius::same(1), Color32::from_rgb(0xe8, 0xe8, 0xe8));
        p.rect_stroke(sq, CornerRadius::same(1), Stroke::new(1.0, Color32::from_black_alpha(160)), StrokeKind::Inside);
        if secs > 0.0 {
            p.text(pos2(x, wave.top() + 3.0), Align2::CENTER_TOP, format!("{secs:.2} s"), FontId::monospace(9.0), Color32::from_rgb(0xe8, 0xe8, 0xe8));
        }
    }

    // Traits de boucle.
    if looping {
        for (t, label) in [(m.loop_start, tr("⟲ début", "⟲ start")), (m.loop_end, tr("fin ⟳", "end ⟳"))] {
            let x = x_of(t);
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0, ACCENT));
            let align = if label.starts_with('⟲') { Align2::LEFT_TOP } else { Align2::RIGHT_TOP };
            let off = if align == Align2::LEFT_TOP { 3.0 } else { -3.0 };
            p.text(pos2(x + off, rect.top() + 1.0), align, label, FontId::proportional(10.0), ACCENT);
        }
    }

    // Poignées de début (verte) et de fin (rouge).
    for (t, col, left) in [(m.start, GO, true), (m.end, STOP, false)] {
        let x = x_of(t);
        p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5, col));
        let dir = if left { 1.0 } else { -1.0 };
        let tri = vec![pos2(x, rect.bottom()), pos2(x + 10.0 * dir, rect.bottom()), pos2(x, rect.bottom() - 12.0)];
        p.add(Shape::convex_polygon(tri, col, Stroke::NONE));
    }

    if let Some(t) = playhead {
        let x = x_of(t);
        p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::WHITE));
    }

    // Glisser les poignées. Les points de boucle se saisissent dans la moitié haute,
    // le début et la fin dans la moitié basse : ils restent séparables même superposés.
    // Les poignées de fondu se saisissent dans la bande au-dessus de la forme d'onde.
    let nearest = |pos: Pos2| {
        if pos.y < wave.top() {
            let fade = fade_handles
                .iter()
                .map(|&(h, x, _)| (h, (x - pos.x).abs()))
                .filter(|(_, d)| *d < 10.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h);
            if fade.is_some() {
                return fade;
            }
        }
        let top = pos.y < rect.center().y;
        let mut handles = vec![(Handle::Start, m.start), (Handle::End, m.end)];
        if looping {
            let lp = [(Handle::LoopStart, m.loop_start), (Handle::LoopEnd, m.loop_end)];
            if top {
                handles.splice(0..0, lp);
            } else {
                handles.extend(lp);
            }
        }
        handles
            .into_iter()
            .map(|(h, t)| (h, (x_of(t) - pos.x).abs()))
            .filter(|(_, d)| *d < 14.0)
            // À distance égale, le premier de la liste (priorité selon la zone) l'emporte.
            .fold(None::<(Handle, f32)>, |best, (h, d)| match best {
                Some((_, bd)) if bd <= d => best,
                _ => Some((h, d)),
            })
            .map(|(h, _)| h)
    };
    let drag_id = resp.id.with("handle");
    if let Some(pos) = resp.hover_pos() {
        if nearest(pos).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    if resp.drag_started() {
        // On choisit la poignée là où le bouton a été enfoncé, pas après le seuil de glisser.
        let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
        let h = origin.and_then(nearest);
        ui.data_mut(|d| d.insert_temp(drag_id, h));
    }
    let dbl_empty = resp.double_clicked() && resp.interact_pointer_pos().and_then(nearest).is_none();
    let mut changed = false;
    let mut view = (v0, v1);
    if resp.dragged() {
        let h: Option<Handle> = ui.data(|d| d.get_temp(drag_id)).flatten();
        if h.is_none() && zoomed {
            // Glisser hors des poignées : déplace la partie visible.
            let dt = -(resp.drag_delta().x / wave.width()) as f64 * (v1 - v0);
            view = clamp_view(v0 + dt, v1 + dt);
        }
        if let (Some(h), Some(pos)) = (h, resp.interact_pointer_pos()) {
            let t = t_of(pos.x);
            let before = (*m, *fade_in, *fade_out);
            match h {
                Handle::FadeIn => *fade_in = (t - m.start).clamp(0.0, m.end - m.start),
                Handle::FadeOut => *fade_out = (m.end - t).clamp(0.0, m.end - m.start),
                Handle::Start => {
                    m.start = t.min(m.end);
                    m.loop_start = m.loop_start.max(m.start);
                    m.loop_end = m.loop_end.max(m.loop_start);
                }
                Handle::End => {
                    m.end = t.max(m.start);
                    m.loop_end = m.loop_end.min(m.end);
                    m.loop_start = m.loop_start.min(m.loop_end);
                }
                Handle::LoopStart => m.loop_start = t.clamp(m.start, m.loop_end),
                Handle::LoopEnd => m.loop_end = t.clamp(m.loop_start, m.end),
            }
            changed = (*m, *fade_in, *fade_out) != before;
        }
    }

    // Zoom : bouton pour tout afficher (ou double-clic) et repère de la partie visible.
    if zoomed {
        let btn = Rect::from_min_size(rect.right_top() + vec2(-86.0, 16.0), vec2(80.0, 18.0));
        if ui.place(btn, egui::Button::new(egui::RichText::new(tr("Tout afficher", "Show all")).small())).clicked() {
            view = (0.0, duration);
        }
    }
    if dbl_empty {
        view = (0.0, duration);
    }
    // Barre de défilement sous la forme d'onde : la partie visible se glisse le long du son,
    // un clic à côté y amène la vue.
    let (sb, sb_resp) = ui.allocate_exact_size(vec2(rect.width(), 10.0), Sense::click_and_drag());
    let span = view.1 - view.0;
    let thumb = Rect::from_x_y_ranges(
        sb.left() + (view.0 / duration) as f32 * sb.width()..=sb.left() + (view.1 / duration) as f32 * sb.width(),
        sb.y_range(),
    );
    let sp = ui.painter();
    sp.rect_filled(sb, CornerRadius::same(3), Color32::from_white_alpha(14));
    let hot = sb_resp.hovered() || sb_resp.dragged();
    sp.rect_filled(thumb, CornerRadius::same(3), Color32::from_white_alpha(if !zoomed { 40 } else if hot { 170 } else { 110 }));
    if zoomed {
        if sb_resp.dragged() {
            let dt = (sb_resp.drag_delta().x / sb.width()) as f64 * duration;
            view = clamp_view(view.0 + dt, view.1 + dt);
        } else if let Some(p) = sb_resp.interact_pointer_pos().filter(|_| sb_resp.clicked()) {
            let c = ((p.x - sb.left()) / sb.width()) as f64 * duration;
            view = clamp_view(c - span / 2.0, c + span / 2.0);
        }
    }
    ui.data_mut(|d| d.insert_temp(view_id, (view.0, view.1, duration)));
    resp.on_hover_text(tr(
        "Glisser les poignées : début et fin en bas (vert, rouge), boucle en haut (orange), fondus sur les carrés blancs.\nMolette : zoom, glisser ailleurs : se déplacer, double-clic : tout afficher.",
        "Drag the handles: start and end at the bottom (green, red), loop at the top (orange), fades on the white squares.\nWheel: zoom, drag elsewhere: scroll, double-click: show all.",
    ));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, RawInput};

    /// Simule un glisser de `from` vers `to` sur l'éditeur placé dans une fenêtre, comme dans l'appli.
    fn drag(from: Pos2, to: Pos2, looping: bool) -> Markers {
        let ctx = egui::Context::default();
        let mut m = Markers { start: 0.0, loop_start: 0.0, loop_end: 10.0, end: 10.0 };
        let overview = |_: f64, _: f64, n: usize| vec![0.5f32; n];
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0));
        let frame = |events: Vec<Event>, m: &mut Markers| {
            let input = RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut wave_rect = Rect::NOTHING;
            let mut out = ctx.run_ui(input, |ui| {
                egui::Window::new("t").fixed_pos(pos2(0.0, 0.0)).default_width(620.0).resizable(true).show(ui.ctx(), |ui| {
                    waveform_editor(ui, overview, 10.0, m, looping, &mut 0.0, &mut 0.0, None);
                    wave_rect = ui.min_rect();
                });
            });
            out.textures_delta.clear();
            wave_rect
        };
        let r = frame(vec![], &mut m);
        let _ = frame(vec![], &mut m);
        println!("zone {r:?}");
        let ev = |p: Pos2, pressed: Option<bool>| match pressed {
            Some(pr) => Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: pr, modifiers: Modifiers::NONE },
            None => Event::PointerMoved(p),
        };
        frame(vec![ev(from, None)], &mut m);
        frame(vec![ev(from, Some(true))], &mut m);
        for i in 1..=10 {
            let p = from + (to - from) * (i as f32 / 10.0);
            frame(vec![ev(p, None)], &mut m);
        }
        frame(vec![ev(to, Some(false))], &mut m);
        m
    }

    #[test]
    fn double_click_types_a_db_value() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
        let mut got = None;
        let frame = |events: Vec<Event>, got: &mut Option<f32>| {
            let input = RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let id = ui.make_persistent_id("db");
                if let Some(v) = db_value(ui, id, pos2(100.0, 100.0), Align2::CENTER_TOP, 0.0, -60.0, -60.0..=10.0, FontId::proportional(11.0), TEXT) {
                    *got = Some(v);
                }
            });
            out.textures_delta.clear();
        };
        let p = pos2(100.0, 106.0);
        let btn = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(vec![], &mut got);
        frame(vec![Event::PointerMoved(p)], &mut got);
        frame(vec![btn(true), btn(false), btn(true), btn(false)], &mut got);
        frame(vec![], &mut got);
        frame(vec![], &mut got);
        frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }], &mut got);
        frame(vec![Event::Text("-6,5".into())], &mut got);
        frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], &mut got);
        assert_eq!(got, Some(-6.5));
    }

    #[test]
    fn typed_db_values() {
        let r = -60.0..=10.0;
        assert_eq!(parse_db("-6", &r), Some(-6.0));
        assert_eq!(parse_db(" +3,5 dB", &r), Some(3.5));
        assert_eq!(parse_db("-4.5dB", &r), Some(-4.5));
        assert_eq!(parse_db("-inf", &r), Some(-60.0));
        assert_eq!(parse_db("-∞ dB", &r), Some(-60.0));
        assert_eq!(parse_db("+25", &r), Some(10.0));
        assert_eq!(parse_db("6", &r), Some(-6.0), "sans signe : négatif");
        assert_eq!(parse_db("+6", &r), Some(6.0));
        assert_eq!(parse_db("0", &r), Some(0.0));
        assert_eq!(parse_db("80", &r), Some(-60.0));
        assert_eq!(parse_db("abc", &r), None);
        assert_eq!(parse_db("", &r), None);
    }

    #[test]
    fn handles_are_draggable() {
        // Géométrie : fenêtre en (0,0), marge interne, éditeur pleine largeur.
        let probe = drag(pos2(-50.0, -50.0), pos2(-50.0, -50.0), false);
        assert_eq!(probe.end, 10.0);
        let ctx_rect = {
            let ctx = egui::Context::default();
            let mut r = Rect::NOTHING;
            let overview = |_: f64, _: f64, n: usize| vec![0.5f32; n];
            for _ in 0..2 {
                let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))), ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| {
                    egui::Window::new("t").fixed_pos(pos2(0.0, 0.0)).default_width(620.0).resizable(true).show(ui.ctx(), |ui| {
                        let mut m = Markers { start: 0.0, loop_start: 0.0, loop_end: 10.0, end: 10.0 };
                        waveform_editor(ui, overview, 10.0, &mut m, true, &mut 0.0, &mut 0.0, None);
                        r = ui.min_rect();
                    });
                });
                out.textures_delta.clear();
            }
            r
        };
        let wave = ctx_rect.shrink2(vec2(HANDLE_PAD, 14.0));
        let y_low = ctx_rect.bottom() - 20.0;
        let y_top = wave.top() + 15.0;
        let mid_x = wave.center().x;

        let m = drag(pos2(wave.right(), y_low), pos2(mid_x, y_low), false);
        assert!((m.end - 5.0).abs() < 0.3, "fin déplacée : {m:?}");

        let m = drag(pos2(wave.left(), y_low), pos2(mid_x, y_low), false);
        assert!((m.start - 5.0).abs() < 0.3, "début déplacé : {m:?}");

        let m = drag(pos2(wave.right(), y_top), pos2(mid_x, y_top), true);
        assert!((m.loop_end - 5.0).abs() < 0.3 && m.end == 10.0, "fin de boucle seule : {m:?}");

        let m = drag(pos2(wave.left(), y_top), pos2(mid_x, y_top), true);
        assert!((m.loop_start - 5.0).abs() < 0.3 && m.start == 0.0, "début de boucle seul : {m:?}");
    }
}

/// Logo de la barre haute : la grille de pads avec l'étoile au centre, puis « JINGLESTAR ».
/// Dessiné en vectoriel pour rester net à toutes les échelles d'affichage.
pub fn logo(ui: &mut Ui, height: f32) -> Response {
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
    let mut job = egui::text::LayoutJob::default();
    let font = FontId::new(height * 0.82, egui::FontFamily::Name("logo".into()));
    for (txt, color) in [("JINGLE", Color32::WHITE), ("STAR", ACCENT)] {
        job.append(
            txt,
            0.0,
            egui::TextFormat { font_id: font.clone(), color, extra_letter_spacing: height * 0.05, ..Default::default() },
        );
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let gap = height * 0.4;
    let (rect, resp) = ui.allocate_exact_size(vec2(height + gap + galley.size().x, height), Sense::hover());
    let p = ui.painter();
    // Grille 3×3 dessinée dans un carré de 100 unités, comme le fichier SVG.
    let u = height / 100.0;
    let at = |x: f32, y: f32| rect.min + vec2(x * u, y * u);
    let mut k = 0;
    for row in 0..3 {
        for col in 0..3 {
            let pad = Rect::from_min_size(at(4.0 + col as f32 * 32.0, 4.0 + row as f32 * 32.0), vec2(28.0 * u, 28.0 * u));
            let r = CornerRadius::same((4.5 * u).round().max(1.0) as u8);
            if row == 1 && col == 1 {
                p.rect_filled(pad, r, ACCENT);
                star(p, pad.center() + vec2(0.0, 0.6 * u), 11.5 * u, BG);
            } else {
                // À cette taille, des pads pleins se lisent mieux que les bandeaux du grand logo.
                p.rect_filled(pad, r, STRIPS[k]);
                k += 1;
            }
        }
    }
    let text_pos = pos2(rect.min.x + height + gap, rect.center().y - galley.size().y / 2.0);
    p.galley(text_pos, galley, Color32::WHITE);
    resp
}

/// Étoile à cinq branches pleine (égui ne remplit que les formes convexes :
/// on la compose d'un pentagone central et de cinq triangles).
fn star(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
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
