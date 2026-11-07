//! Widgets propres à JingleStar (boutons de jingle) ; les widgets communs viennent de star-ui.

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui};

pub use star_ui::widgets::*;

use super::theme::*;
use star_core::i18n::tr;

pub struct SlotView<'a> {
    pub index: usize,
    pub name: Option<&'a str>,
    pub color: [u8; 3],
    pub playing: bool,
    /// En train de s'éteindre en fondu de sortie.
    pub fading: bool,
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
        // Sans bandeau : la couleur remplit tout le bouton, sombre au repos et vive en lecture,
        // avec alors un texte foncé.
        p.rect_filled(rect, r, slot_fill(v.color, v.playing));
        let ink = if v.playing { PLAYING_INK } else { TEXT };

        let name = v.name.unwrap_or_default();
        let galley = p.layout(name.to_owned(), FontId::proportional(if v.playing { 15.0 } else { 14.0 }), ink, rect.width() - 12.0);
        let gpos = rect.center() - galley.size() / 2.0 - vec2(0.0, 8.0);
        p.galley(gpos, galley, ink);

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
            p.rect_filled(fill, CornerRadius::same(2), if v.fading { ACCENT } else { GO });
            if v.fading {
                // Fondu de sortie : rampe descendante orange, cernée de noir pour rester visible sur toute couleur.
                let (a, b) = (pos2(rect.left() + 4.0, rect.top() + 26.0), pos2(rect.right() - 4.0, bar.top() - 4.0));
                p.add(Shape::convex_polygon(vec![a, b, pos2(a.x, b.y)], ACCENT.gamma_multiply(0.18), Stroke::NONE));
                p.line_segment([a, b], Stroke::new(4.0, Color32::from_black_alpha(140)));
                p.line_segment([a, b], Stroke::new(2.0, ACCENT));
            }
            // « FONDU » remplace le temps restant, sur une pastille orange.
            let fade_label = tr("FONDU", "FADE");
            let right = if v.fading { fade_label } else { remaining.as_str() };
            let rfont = FontId::monospace(12.0);
            let rw = p.layout_no_wrap(right.to_owned(), rfont.clone(), ink).size().x;
            let draw_right = |x: f32, align: Align2| {
                if v.fading {
                    let left = match align { Align2::RIGHT_BOTTOM => x - rw, _ => x - rw / 2.0 };
                    let pill = Rect::from_min_max(pos2(left - 3.0, text_y - 14.0), pos2(left + rw + 3.0, text_y + 1.0));
                    p.rect_filled(pill, CornerRadius::same(2), ACCENT);
                    p.text(pos2(left, text_y), Align2::LEFT_BOTTOM, right, rfont.clone(), Color32::BLACK);
                } else {
                    p.text(pos2(x, text_y), align, right, rfont.clone(), ink);
                }
            };
            // Le temps restant prime : sur un bouton étroit, il s'affiche seul, centré.
            let both = p.layout_no_wrap(format!("{elapsed} {right}"), font.clone(), ink).size().x + 12.0;
            if both <= bar.width() {
                p.text(pos2(bar.left(), text_y), Align2::LEFT_BOTTOM, elapsed, font.clone(), ink);
                draw_right(bar.right(), Align2::RIGHT_BOTTOM);
            } else {
                draw_right(bar.center().x, Align2::CENTER_BOTTOM);
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
    let num_col = if v.playing && v.name.is_some() { PLAYING_INK } else { TEXT_DIM };
    p.text(rect.left_top() + vec2(6.0, 7.0), Align2::LEFT_TOP, format!("{}", v.index + 1), FontId::monospace(11.0), num_col);
    if v.selected {
        p.rect_stroke(rect.expand(1.0), r, Stroke::new(2.0, ACCENT), StrokeKind::Outside);
    }
    if v.drop_hover {
        p.rect_stroke(rect, r, Stroke::new(2.0, EDIT), StrokeKind::Inside);
    }
    resp
}

/// Volume d'un jingle en haut à droite de son bouton, en couleur s'il n'est pas à 0 dB ;
/// double-clic pour le saisir. À appeler après `jingle_button`, pour passer au-dessus du bouton.
/// En lecture, le fond est vif : le texte passe en foncé.
pub fn jingle_volume(ui: &mut Ui, id: Id, rect: Rect, volume_db: f32, playing: bool) -> Option<f32> {
    let col = if playing { PLAYING_INK } else if volume_db.abs() < 0.05 { TEXT_DIM } else { ACCENT };
    db_value(ui, id, rect.right_top() + vec2(-6.0, 7.0), Align2::RIGHT_TOP, volume_db, -40.0, -40.0..=10.0, FontId::monospace(10.0), col)
}
