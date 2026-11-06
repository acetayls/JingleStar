//! Éditeur d'égaliseur : courbe de réponse avec un point déplaçable par bande,
//! et réglages numériques en dessous.

use std::fmt::Debug;
use std::hash::Hash;

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui};

use super::theme::*;
use crate::audio::eq::{BandKind, CutSlope, EqSettings, GAIN_RANGE, MAX_FREQ, MIN_FREQ};
use crate::i18n::tr;

fn band_color(k: BandKind) -> Color32 {
    match k {
        BandKind::LowCut => Color32::from_rgb(0x9a, 0x9a, 0xb0),
        BandKind::LowShelf => Color32::from_rgb(0xe0, 0x8a, 0x3c),
        BandKind::Peak1 => Color32::from_rgb(0x5c, 0xc8, 0x6a),
        BandKind::Peak2 => Color32::from_rgb(0x4a, 0xb4, 0xe8),
        BandKind::HighShelf => Color32::from_rgb(0xc8, 0x6c, 0xd8),
    }
}

fn band_name(k: BandKind) -> &'static str {
    match k {
        BandKind::LowCut => tr("Coupe-bas", "Low cut"),
        BandKind::LowShelf => tr("Shelf grave", "Low shelf"),
        BandKind::Peak1 => tr("Param. 1", "Peak 1"),
        BandKind::Peak2 => tr("Param. 2", "Peak 2"),
        BandKind::HighShelf => tr("Shelf aigu", "High shelf"),
    }
}

fn band_tag(k: BandKind) -> &'static str {
    match k {
        BandKind::LowCut => "LC",
        BandKind::LowShelf => "LS",
        BandKind::Peak1 => "1",
        BandKind::Peak2 => "2",
        BandKind::HighShelf => "HS",
    }
}

fn fmt_freq(f: f32) -> String {
    if f >= 1000.0 {
        format!("{:.1}k", f / 1000.0).replace(".0k", "k")
    } else {
        format!("{f:.0}")
    }
}

/// Affiche l'éditeur et modifie `eq` ; renvoie `true` si un réglage a changé.
pub fn eq_editor(ui: &mut Ui, id_salt: impl Hash + Debug, eq: &mut EqSettings, sample_rate: u32) -> bool {
    let before = *eq;
    let id = ui.make_persistent_id(("eq-editor", id_salt));
    let sel_id = id.with("selected");
    let mut selected: BandKind = ui.data(|d| d.get_temp(sel_id)).unwrap_or(BandKind::Peak1);

    ui.horizontal(|ui| {
        ui.checkbox(&mut eq.enabled, tr("Égaliseur actif", "EQ enabled"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(tr("Réinitialiser", "Reset")).clicked() {
                *eq = EqSettings::default();
            }
        });
    });

    curve(ui, id, eq, sample_rate, &mut selected);
    controls(ui, id, eq, &mut selected);

    ui.data_mut(|d| d.insert_temp(sel_id, selected));
    *eq != before
}

fn curve(ui: &mut Ui, id: egui::Id, eq: &mut EqSettings, sample_rate: u32, selected: &mut BandKind) {
    let size = vec2(ui.available_width().max(300.0), 170.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(0x12, 0x12, 0x14));
    let plot = Rect::from_min_max(rect.min + vec2(30.0, 8.0), rect.max - vec2(10.0, 16.0));
    let sr = sample_rate.max(8000) as f64;
    let span = (MAX_FREQ / MIN_FREQ).ln();
    let x_of = |f: f32| plot.left() + (f.clamp(MIN_FREQ, MAX_FREQ) / MIN_FREQ).ln() / span * plot.width();
    let f_of = |x: f32| MIN_FREQ * (((x - plot.left()) / plot.width()).clamp(0.0, 1.0) * span).exp();
    let y_of = |db: f32| plot.center().y - db.clamp(-GAIN_RANGE - 2.0, GAIN_RANGE + 2.0) / GAIN_RANGE * plot.height() / 2.0;
    let db_of = |y: f32| ((plot.center().y - y) / (plot.height() / 2.0) * GAIN_RANGE).clamp(-GAIN_RANGE, GAIN_RANGE);

    // Grille.
    let grid = Stroke::new(1.0, Color32::from_white_alpha(14));
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
        let x = x_of(f);
        p.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], grid);
        p.text(pos2(x, plot.bottom() + 2.0), Align2::CENTER_TOP, fmt_freq(f), FontId::proportional(9.0), TEXT_DIM);
    }
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = y_of(db);
        let s = if db == 0.0 { Stroke::new(1.0, Color32::from_white_alpha(40)) } else { grid };
        p.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], s);
        p.text(pos2(plot.left() - 4.0, y), Align2::RIGHT_CENTER, format!("{db:+.0}"), FontId::proportional(9.0), TEXT_DIM);
    }

    // La courbe reste visible (estompée) quand l'égaliseur est contourné.
    let mut shown = *eq;
    shown.enabled = true;
    let alpha = if eq.enabled { 1.0 } else { 0.35 };
    let cols = plot.width().max(1.0) as usize;
    let freqs: Vec<f32> = (0..=cols).map(|c| f_of(plot.left() + c as f32)).collect();

    // Bande sélectionnée, en surimpression.
    let stages = shown.band_stages(*selected, sr);
    if !stages.is_empty() {
        let col = band_color(*selected).gamma_multiply(0.18 * alpha);
        let zero = y_of(0.0);
        for (c, &f) in freqs.iter().enumerate() {
            let g: f64 = stages.iter().map(|s| s.magnitude(f as f64, sr)).product();
            let y = y_of(20.0 * g.max(1e-9).log10() as f32);
            let x = plot.left() + c as f32;
            p.line_segment([pos2(x, zero), pos2(x, y)], Stroke::new(1.0, col));
        }
    }

    // Réponse totale.
    let coeffs = shown.coeffs(sample_rate);
    let pts: Vec<Pos2> = freqs
        .iter()
        .enumerate()
        .map(|(c, &f)| pos2(plot.left() + c as f32, y_of(coeffs.response_db(f as f64, sr) as f32)))
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0, ACCENT.gamma_multiply(alpha))));

    // Points des bandes.
    let node = |k: BandKind| {
        let b = shown.band(k);
        pos2(x_of(b.freq), y_of(if k.has_gain() { b.gain_db } else { 0.0 }))
    };
    for k in BandKind::ALL {
        let b = eq.band(k);
        let c = node(k);
        let col = band_color(k);
        let r = if k == *selected { 9.0 } else { 7.5 };
        if b.on && eq.enabled {
            p.circle(c, r, col, Stroke::new(1.5, if k == *selected { Color32::WHITE } else { Color32::BLACK }));
            p.text(c, Align2::CENTER_CENTER, band_tag(k), FontId::proportional(9.0), Color32::BLACK);
        } else {
            p.circle(c, r, Color32::from_rgb(0x12, 0x12, 0x14), Stroke::new(1.5, col.gamma_multiply(0.6)));
            p.text(c, Align2::CENTER_CENTER, band_tag(k), FontId::proportional(9.0), col.gamma_multiply(0.7));
        }
    }

    // Interaction : on choisit le point le plus proche de l'appui.
    let nearest = |pos: Pos2| {
        BandKind::ALL
            .iter()
            .map(|&k| (k, node(k).distance(pos)))
            .filter(|(_, d)| *d < 14.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    };
    let drag_id = id.with("drag");
    let press = ui.input(|i| i.pointer.press_origin());
    if resp.drag_started() || resp.clicked() || resp.double_clicked() {
        let hit = press.or(resp.interact_pointer_pos()).and_then(nearest);
        if let Some(k) = hit {
            *selected = k;
        }
        if resp.drag_started() {
            ui.data_mut(|d| d.insert_temp(drag_id, hit));
        }
        if resp.double_clicked() {
            if let Some(k) = hit {
                let b = eq.band_mut(k);
                b.on = !b.on;
            }
        }
    }
    if resp.dragged() {
        let h: Option<BandKind> = ui.data(|d| d.get_temp(drag_id)).flatten();
        if let (Some(k), Some(pos)) = (h, resp.interact_pointer_pos()) {
            let b = eq.band_mut(k);
            b.on = true;
            b.freq = f_of(pos.x).round();
            if k.has_gain() {
                let g = db_of(pos.y);
                // Petit cran à 0 dB.
                b.gain_db = if g.abs() < 0.4 { 0.0 } else { (g * 10.0).round() / 10.0 };
            }
            eq.enabled = true;
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<BandKind>>(drag_id));
    }
    if let Some(pos) = resp.hover_pos() {
        let hovered = nearest(pos);
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        // Molette : largeur (Q) de la bande survolée.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if let Some(k) = hovered.filter(|&k| scroll != 0.0 && k != BandKind::LowCut) {
            let b = eq.band_mut(k);
            b.q = (b.q * (scroll * 0.004).exp()).clamp(0.1, 18.0);
        }
    }
    resp.on_hover_text(tr(
        "Glisser un point : fréquence et gain · molette : largeur (Q) · double-clic : activer/désactiver",
        "Drag a point: frequency and gain · wheel: width (Q) · double-click: on/off",
    ));
}

fn controls(ui: &mut Ui, id: egui::Id, eq: &mut EqSettings, selected: &mut BandKind) {
    egui::Grid::new(id.with("grid")).num_columns(6).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("");
        for k in BandKind::ALL {
            let b = eq.band_mut(k);
            let col = band_color(k);
            let button = if b.on {
                egui::Button::new(RichText::new(band_name(k)).color(Color32::BLACK).strong()).fill(col)
            } else {
                egui::Button::new(RichText::new(band_name(k)).color(TEXT_DIM))
            };
            let r = ui.add(button).on_hover_text(tr("Activer / désactiver la bande", "Enable / disable band"));
            if r.clicked() {
                b.on = !b.on;
                *selected = k;
            }
        }
        ui.end_row();

        ui.label(RichText::new(tr("Fréq.", "Freq.")).color(TEXT_DIM));
        for k in BandKind::ALL {
            let b = eq.band_mut(k);
            let speed = (b.freq * 0.005).max(0.5);
            let r = ui.add(egui::DragValue::new(&mut b.freq).range(MIN_FREQ..=MAX_FREQ).speed(speed).max_decimals(0).suffix(" Hz"));
            if r.changed() {
                b.on = true;
                *selected = k;
            }
        }
        ui.end_row();

        ui.label(RichText::new("Gain").color(TEXT_DIM));
        for k in BandKind::ALL {
            if !k.has_gain() {
                ui.label("");
                continue;
            }
            let b = eq.band_mut(k);
            let r = ui.add(egui::DragValue::new(&mut b.gain_db).range(-GAIN_RANGE..=GAIN_RANGE).speed(0.1).max_decimals(1).suffix(" dB"));
            if r.changed() {
                b.on = true;
                *selected = k;
            }
        }
        ui.end_row();

        ui.label(RichText::new(tr("Q / pente", "Q / slope")).color(TEXT_DIM));
        for k in BandKind::ALL {
            if k == BandKind::LowCut {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut eq.low_cut_slope, CutSlope::Db12, "12");
                    ui.selectable_value(&mut eq.low_cut_slope, CutSlope::Db24, "24");
                    ui.label(RichText::new("dB/oct").small().color(TEXT_DIM));
                });
                continue;
            }
            let b = eq.band_mut(k);
            if ui.add(egui::DragValue::new(&mut b.q).range(0.1..=18.0).speed(0.01).max_decimals(2)).changed() {
                *selected = k;
            }
        }
        ui.end_row();
    });
}
