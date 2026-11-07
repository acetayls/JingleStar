//! Éditeur d'un élément du Lecteur ou d'un cue de la Cuelist, comme celui d'un jingle de
//! JingleStar : volume, lecture une fois ou en boucle, fondus au noir, frise de vignettes avec
//! ses poignées d'entrée et de sortie, égaliseur de la bande son. Et l'aperçu où l'on se
//! déplace dans le média sans toucher à la sortie.

use crate::preview::Preview;
use crate::protocol::Clip;
use egui::{pos2, vec2, Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui};
use star_core::i18n::tr;
use star_ui::sound_editor::{self, WaveSource};
use star_ui::theme::*;
use star_ui::widgets::{fmt_time, WaveView};
use std::path::Path;

/// Vignettes de la frise.
const STRIP_COUNT: usize = 24;

pub fn is_image(p: &Path) -> bool {
    p.extension().map(|e| ["jpg", "jpeg", "png", "bmp", "gif", "tif", "tiff", "webp"].contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

pub fn is_audio(p: &Path) -> bool {
    p.extension().map(|e| ["mp3", "wav", "flac", "aac", "m4a", "ogg"].contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

/// Fond de l'éditeur : vignettes de la vidéo côte à côte sur la partie visible, assombries
/// pendant les fondus (le noir que verra la salle).
fn strip_background(tiles: Vec<Option<egui::TextureHandle>>, duration: f64) -> impl FnOnce(&WaveView) {
    move |w: &WaveView| {
        let p = w.painter;
        let r = w.rect;
        let n = tiles.len().max(1);
        let tile_w = (r.height() * 16.0 / 9.0).max(8.0);
        let mut x = r.left();
        while x < r.right() {
            let cell = Rect::from_min_size(pos2(x, r.top()), vec2(tile_w.min(r.right() - x), r.height()));
            let t = w.t_of(cell.center().x);
            let i = ((t / duration.max(1e-6)) * n as f64).floor().clamp(0.0, (n - 1) as f64) as usize;
            match tiles.get(i).and_then(|t| t.as_ref()) {
                Some(tex) => {
                    let full = Rect::from_min_size(cell.min, vec2(tile_w, r.height()));
                    let uv_w = (cell.width() / tile_w).clamp(0.0, 1.0);
                    p.image(tex.id(), cell, Rect::from_min_max(pos2(0.0, 0.0), pos2(uv_w, 1.0)), Color32::WHITE);
                    let _ = full;
                }
                None => {
                    p.rect_filled(cell.shrink(1.0), CornerRadius::same(2), PANEL_HI);
                }
            }
            x += tile_w;
        }
        // Fondus : voile noir selon le gain, colonne par colonne.
        let step = 3.0;
        let mut x = r.left();
        while x < r.right() {
            let g = (w.fade_gain)(w.t_of(x + step / 2.0));
            if g < 0.999 {
                let a = ((1.0 - g.clamp(0.0, 1.0)) * 235.0) as u8;
                p.rect_filled(Rect::from_x_y_ranges(x..=(x + step).min(r.right()), r.y_range()), CornerRadius::ZERO, Color32::from_black_alpha(a));
            }
            x += step;
        }
    }
}

/// Image de l'aperçu, ajustée dans `rect` (16:9), ou la vignette fixe en attendant.
pub fn preview_image(ui: &mut Ui, preview: &mut Preview, path: &Path, rect: Rect, fallback: Option<&egui::TextureHandle>) {
    ui.painter().rect_filled(rect, 3, Color32::BLACK);
    let ctx = ui.ctx().clone();
    let live = if preview.state.path.as_deref() == Some(path) { preview.texture(&ctx).cloned() } else { None };
    match live.as_ref().or(fallback) {
        Some(t) => {
            let ts = t.size_vec2();
            let k = (rect.width() / ts.x).min(rect.height() / ts.y);
            let r = Rect::from_center_size(rect.center(), ts * k);
            ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            let txt = if is_audio(path) { tr("son seul", "audio only") } else { "…" };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, txt, egui::FontId::proportional(14.0), TEXT_DIM);
        }
    }
}

/// Barre de déplacement de l'aperçu, avec les points d'entrée et de sortie de l'élément,
/// puis le temps et les boutons lecture, image précédente et suivante.
/// Glisser : saut aux images clés (fluide) ; au relâché, position exacte.
pub fn scrub_bar(ui: &mut Ui, id: impl std::hash::Hash, preview: &mut Preview, path: &Path, clip: &Clip) {
    let ctx = ui.ctx().clone();
    let len = if preview.state.path.as_deref() == Some(path) { preview.state.duration } else { 0.0 };
    let mut t = if len > 0.0 { preview.state.time.clamp(0.0, len) } else { 0.0 };
    let (hit, bar) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::click_and_drag());
    let r = Rect::from_center_size(hit.center(), vec2(hit.width(), 6.0));
    let frac_at = |x: f32| ((x - r.left()) / r.width()).clamp(0.0, 1.0) as f64;
    let x_at = |t: f64| r.left() + r.width() * (t / len.max(1e-6)).clamp(0.0, 1.0) as f32;
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(3), PANEL_HI);
    if len > 0.0 {
        if let Some(pos) = bar.interact_pointer_pos() {
            let to = frac_at(pos.x) * len;
            if bar.dragged() {
                preview.seek(&ctx, to, false);
                t = to;
            }
            if bar.clicked() || bar.drag_stopped() {
                preview.seek(&ctx, to, true);
                t = to;
            }
        }
        if bar.hovered() || bar.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let p = ui.painter();
        // Partie jouée de l'élément, entre ses poignées.
        let s = &clip.sound;
        let (st, _, _, end) = s.markers(len);
        p.rect_filled(Rect::from_x_y_ranges(x_at(st)..=x_at(end), r.y_range()), CornerRadius::same(3), GO.gamma_multiply(0.35));
        for (x, c) in [(x_at(st), GO), (x_at(end), STOP)] {
            p.line_segment([pos2(x, hit.top()), pos2(x, hit.bottom())], Stroke::new(2.0, c));
        }
        let x = x_at(t);
        p.line_segment([pos2(x, hit.top()), pos2(x, hit.bottom())], Stroke::new(2.0, Color32::WHITE));
    }
    ui.horizontal(|ui| {
        let playing = !preview.state.paused && len > 0.0;
        let small = |ui: &mut Ui, icon, salt: &str, tip: &str| {
            let (r, _) = ui.allocate_exact_size(vec2(30.0, 22.0), Sense::hover());
            star_ui::widgets::transport_button(ui, r, salt, icon, None, PANEL_HI, TEXT).on_hover_text(tip).clicked()
        };
        use star_ui::widgets::TransportIcon;
        let (icon, tip) = if playing {
            (TransportIcon::Pause, tr("Mettre l'aperçu en pause", "Pause the preview"))
        } else {
            (TransportIcon::Play, tr("Lire l'aperçu (sans son, la sortie ne bouge pas)", "Play the preview (muted, the output is not touched)"))
        };
        if small(ui, icon, "pv-play", tip) {
            preview.play(&ctx, !playing);
        }
        if small(ui, TransportIcon::Prev, "pv-back", tr("Image précédente", "Previous frame")) {
            preview.step(&ctx, true);
        }
        if small(ui, TransportIcon::Next, "pv-fwd", tr("Image suivante", "Next frame")) {
            preview.step(&ctx, false);
        }
        ui.label(RichText::new(fmt_time(t)).monospace().color(TEXT_DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("-{}", fmt_time((len - t).max(0.0)))).monospace().color(TEXT_DIM));
        });
    });
    let _ = id;
}

/// Corps de l'éditeur d'un élément : réglages, aperçu, frise avec ses poignées, égaliseur.
/// À placer dans une fenêtre ; `clip` est modifié en place.
pub fn clip_editor(ui: &mut Ui, id_salt: impl std::hash::Hash + std::fmt::Debug + Copy, path: &Path, clip: &mut Clip, preview: &mut Preview) {
    let ctx = ui.ctx().clone();
    let image = is_image(path);
    preview.open(&ctx, path, clip.sound.start);
    preview.poll();

    egui::Grid::new(("clip-fields", id_salt)).num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        if image {
            ui.label(tr("Affichage", "Display"));
            ui.horizontal(|ui| {
                let mut timed = clip.image_secs.is_some();
                ui.radio_value(&mut timed, false, tr("Jusqu'au média suivant", "Until the next media"));
                ui.radio_value(&mut timed, true, tr("Pendant", "For"));
                if timed {
                    let mut d = clip.image_secs.unwrap_or(5.0);
                    ui.add(egui::DragValue::new(&mut d).range(0.1..=3600.0).speed(0.1).suffix(" s").max_decimals(1));
                    clip.image_secs = Some(d);
                } else {
                    clip.image_secs = None;
                }
            });
            ui.end_row();
            ui.label(tr("Fondus au noir", "Fades to black"));
            ui.horizontal(|ui| {
                ui.label(tr("entrée", "in"));
                ui.add(egui::DragValue::new(&mut clip.sound.fade_in).range(0.0..=30.0).speed(0.05).suffix(" s").max_decimals(2));
                ui.label(tr("sortie", "out"));
                ui.add_enabled(
                    clip.image_secs.is_some(),
                    egui::DragValue::new(&mut clip.sound.fade_out).range(0.0..=30.0).speed(0.05).suffix(" s").max_decimals(2),
                )
                .on_disabled_hover_text(tr("Le fondu de sortie d'une image demande une durée d'affichage", "An image's fade out needs a display time"));
            });
            ui.end_row();
        } else {
            sound_editor::sound_rows(ui, &mut clip.sound, true);
        }
    });
    if !image {
        ui.label(
            RichText::new(tr("Les fondus partent du noir et y reviennent, image et son ensemble.", "Fades go from and to black, picture and sound together."))
                .small()
                .color(TEXT_DIM),
        );
    }
    ui.add_space(6.0);

    // Aperçu : l'image à la position de la tête de lecture.
    let w = ui.available_width().min(360.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, w * 9.0 / 16.0), Sense::hover());
    preview_image(ui, preview, path, rect, None);
    if !image {
        scrub_bar(ui, ("clip-scrub", id_salt), preview, path, clip);
    }
    if let Some(e) = &preview.state.missing {
        ui.label(RichText::new(format!("{} ({e})", tr("Aperçu indisponible", "Preview unavailable"))).small().color(METER_MID));
    }
    if image {
        return;
    }

    ui.add_space(6.0);
    let shown = preview.shows(path);
    let duration = preview.state.duration;
    let wave = (shown && duration > 0.0).then(|| {
        if !is_audio(path) {
            preview.request_strip(&ctx, path, STRIP_COUNT);
        }
        let tiles: Vec<Option<egui::TextureHandle>> = if is_audio(path) { Vec::new() } else { preview.strip_textures(&ctx).to_vec() };
        WaveSource { duration, background: Box::new(strip_background(tiles, duration)), playhead: Some(preview.state.time) }
    });
    let status = (!shown).then(|| RichText::new(tr("Lecture du média…", "Reading the media…")).color(TEXT_DIM));
    let before = (clip.sound.start, clip.sound.end);
    if let Some(t) = sound_editor::sound_waveform_and_eq(ui, ("clip", id_salt), &mut clip.sound, wave, status, 48_000) {
        preview.seek(&ctx, t, true);
    }
    // Une poignée déplacée : l'aperçu montre l'image à cet endroit.
    let dragging = ui.input(|i| i.pointer.any_down());
    if clip.sound.start != before.0 {
        preview.seek(&ctx, clip.sound.start, !dragging);
    } else if clip.sound.end != before.1 {
        if let Some(e) = clip.sound.end {
            preview.seek(&ctx, e, !dragging);
        }
    }
}
