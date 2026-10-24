//! Widgets dessinés à la main : boutons de jingle, vumètres, éditeur de forme d'onde.

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke,
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
    /// Volume du jingle (dB), toujours affiché.
    pub volume_db: f32,
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
            p.text(pos2(bar.left(), text_y), Align2::LEFT_BOTTOM, elapsed, font.clone(), Color32::WHITE);
            p.text(pos2(bar.right(), text_y), Align2::RIGHT_BOTTOM, remaining, font, Color32::WHITE);
        } else if let Some(d) = &v.duration_label {
            p.text(pos2(rect.center().x, text_y), Align2::CENTER_BOTTOM, d, font, TEXT_DIM);
        }
        // Volume en haut à droite, en couleur s'il n'est pas à 0 dB.
        let vol_col = if v.volume_db.abs() < 0.05 { TEXT_DIM } else { ACCENT };
        let vol = p.text(rect.right_top() + vec2(-6.0, 7.0), Align2::RIGHT_TOP, fmt_db(v.volume_db, -40.0), FontId::monospace(10.0), vol_col);
        if v.looping {
            p.text(pos2(vol.left() - 4.0, vol.top()), Align2::RIGHT_TOP, "🔁", FontId::proportional(11.0), TEXT_DIM);
        }
        if v.eq {
            p.text(rect.left_bottom() + vec2(6.0, -5.0), Align2::LEFT_BOTTOM, "EQ", FontId::proportional(9.0), ACCENT);
        }
        if v.loading {
            p.text(rect.right_bottom() - vec2(6.0, 5.0), Align2::RIGHT_BOTTOM, "…", FontId::proportional(14.0), TEXT_DIM);
        }
        if v.error {
            p.text(rect.right_bottom() - vec2(6.0, 5.0), Align2::RIGHT_BOTTOM, "⚠", FontId::proportional(14.0), STOP);
        }
        let stroke = if v.playing {
            Stroke::new(2.0, GO)
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

/// Ligne de la liste des morceaux : numéro et nom à gauche (tronqué), gain d'équilibrage à droite.
pub fn track_row(ui: &mut Ui, label: &str, color: Color32, selected: bool, gain: Option<(String, Color32)>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, CornerRadius::same(3), ACCENT.gamma_multiply(0.18));
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(3), PANEL_HI);
    }
    let font = FontId::monospace(12.0);
    let mut right = rect.right() - 6.0;
    if let Some((txt, col)) = gain {
        let g = p.text(pos2(right, rect.center().y), Align2::RIGHT_CENTER, txt, FontId::monospace(11.0), col);
        right = g.left() - 8.0;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - rect.left() - 6.0).max(10.0));
    let g = p.layout_job(job);
    p.galley(pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0), g, color);
    resp
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
}

/// Éditeur façon QLab : poignées de début et de fin, traits de début et de fin de boucle,
/// rampes de fondu. Renvoie `true` si un point a été déplacé.
#[allow(clippy::too_many_arguments)]
pub fn waveform_editor(
    ui: &mut Ui,
    overview: &[f32],
    duration: f64,
    m: &mut Markers,
    looping: bool,
    fade_in: f64,
    fade_out: f64,
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
    let x_of = |t: f64| wave.left() + (t / duration).clamp(0.0, 1.0) as f32 * wave.width();
    let t_of = |x: f32| ((x - wave.left()) / wave.width()).clamp(0.0, 1.0) as f64 * duration;

    // Zone de boucle.
    if looping {
        let lr = Rect::from_x_y_ranges(x_of(m.loop_start)..=x_of(m.loop_end), wave.y_range());
        p.rect_filled(lr, CornerRadius::ZERO, ACCENT.gamma_multiply(0.12));
    }

    // Forme d'onde.
    let mid = wave.center().y;
    let n = overview.len().max(1);
    let cols = wave.width().max(1.0) as usize;
    for c in 0..cols {
        let a = c * n / cols;
        let b = ((c + 1) * n / cols).max(a + 1).min(n);
        let peak = overview[a.min(n - 1)..b].iter().cloned().fold(0.0f32, f32::max);
        let x = wave.left() + c as f32 + 0.5;
        let t = t_of(x);
        let inside = t >= m.start && t <= m.end;
        let col = if inside { Color32::from_rgb(0x7f, 0xb8, 0xe8) } else { Color32::from_rgb(0x3a, 0x48, 0x55) };
        let h = peak.min(1.0) * wave.height() / 2.0;
        p.line_segment([pos2(x, mid - h), pos2(x, mid + h)], Stroke::new(1.0, col));
    }

    // Parties exclues assombries.
    let shade = Color32::from_black_alpha(140);
    p.rect_filled(Rect::from_x_y_ranges(rect.left()..=x_of(m.start), rect.y_range()), CornerRadius::ZERO, shade);
    p.rect_filled(Rect::from_x_y_ranges(x_of(m.end)..=rect.right(), rect.y_range()), CornerRadius::ZERO, shade);

    // Rampes de fondu.
    let ramp = Stroke::new(1.5, Color32::from_rgb(0xe8, 0xe8, 0xe8));
    if fade_in > 0.0 {
        p.line_segment([pos2(x_of(m.start), wave.bottom()), pos2(x_of(m.start + fade_in), wave.top())], ramp);
    }
    if fade_out > 0.0 && !looping {
        p.line_segment([pos2(x_of(m.end - fade_out), wave.top()), pos2(x_of(m.end), wave.bottom())], ramp);
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
    let nearest = |pos: Pos2| {
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
    let mut changed = false;
    if resp.dragged() {
        let h: Option<Handle> = ui.data(|d| d.get_temp(drag_id)).flatten();
        if let (Some(h), Some(pos)) = (h, resp.interact_pointer_pos()) {
            let t = t_of(pos.x);
            let before = *m;
            match h {
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
            changed = *m != before;
        }
    }
    resp.on_hover_text(tr(
        "Glisser les poignées : début et fin en bas (vert, rouge), boucle en haut (orange)",
        "Drag the handles: start and end at the bottom (green, red), loop at the top (orange)",
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
        let overview = vec![0.5f32; 500];
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0));
        let mut frame = |events: Vec<Event>, m: &mut Markers| {
            let input = RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut wave_rect = Rect::NOTHING;
            let mut out = ctx.run_ui(input, |ui| {
                egui::Window::new("t").fixed_pos(pos2(0.0, 0.0)).default_width(620.0).resizable(true).show(ui.ctx(), |ui| {
                    waveform_editor(ui, &overview, 10.0, m, looping, 0.0, 0.0, None);
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
    fn handles_are_draggable() {
        // Géométrie : fenêtre en (0,0), marge interne, éditeur pleine largeur.
        let probe = drag(pos2(-50.0, -50.0), pos2(-50.0, -50.0), false);
        assert_eq!(probe.end, 10.0);
        let ctx_rect = {
            let ctx = egui::Context::default();
            let mut r = Rect::NOTHING;
            let overview = vec![0.5f32; 500];
            for _ in 0..2 {
                let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))), ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| {
                    egui::Window::new("t").fixed_pos(pos2(0.0, 0.0)).default_width(620.0).resizable(true).show(ui.ctx(), |ui| {
                        let mut m = Markers { start: 0.0, loop_start: 0.0, loop_end: 10.0, end: 10.0 };
                        waveform_editor(ui, &overview, 10.0, &mut m, true, 0.0, 0.0, None);
                        r = ui.min_rect();
                    });
                });
                out.textures_delta.clear();
            }
            r
        };
        let wave = ctx_rect.shrink2(vec2(HANDLE_PAD, 14.0));
        let y_low = ctx_rect.bottom() - 20.0;
        let y_top = ctx_rect.top() + 10.0;
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
