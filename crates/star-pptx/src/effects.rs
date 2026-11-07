//! Ombre portée extérieure (`a:outerShdw`), seul effet rendu : décalée, éventuellement floue.

use crate::color::{child_color, ColorCtx, Rgba};
use crate::theme::Theme;
use crate::xml::Elem;
use tiny_skia::{FillRule, Paint, Path, Pixmap, PixmapPaint, Transform};

#[derive(Debug, Clone, Copy)]
pub struct Shadow {
    /// Décalage en EMU (repère de la diapositive).
    pub dx: f32,
    pub dy: f32,
    /// Rayon de flou en EMU.
    pub blur: f32,
    pub color: Rgba,
}

/// Ombre d'une forme : `spPr/a:effectLst`, sinon le style d'effet du thème (`a:effectRef`).
pub fn shadow_of(sp_pr: Option<&Elem>, style: Option<&Elem>, theme: &Theme, cc: &ColorCtx) -> Option<Shadow> {
    if let Some(list) = sp_pr.and_then(|s| s.child("effectLst")) {
        return parse_outer(list.child("outerShdw")?, cc);
    }
    if sp_pr.and_then(|s| s.child("effectDag")).is_some() {
        return None;
    }
    let r = style?.child("effectRef")?;
    let idx = r.attr_i64("idx")?;
    if idx <= 0 {
        return None;
    }
    let ph = child_color(r, cc);
    let cc = ColorCtx { ph_clr: ph.or(cc.ph_clr), ..*cc };
    let st = theme.effect_styles.get(idx as usize - 1)?;
    parse_outer(st.path(&["effectLst", "outerShdw"])?, &cc)
}

fn parse_outer(sh: &Elem, cc: &ColorCtx) -> Option<Shadow> {
    let color = child_color(sh, cc)?;
    if color.a <= 0.0 {
        return None;
    }
    let dist = sh.attr_f64("dist").unwrap_or(0.0) as f32;
    let dir = (sh.attr_f64("dir").unwrap_or(0.0) as f32 / 60_000.0).to_radians();
    Some(Shadow { dx: dist * dir.cos(), dy: dist * dir.sin(), blur: sh.attr_f64("blurRad").unwrap_or(0.0) as f32, color })
}

/// Dessine l'ombre des tracés `paths` (repère local `tf`) ; `scale` : pixels par EMU.
pub fn draw_shadow(pix: &mut Pixmap, paths: &[&Path], sh: &Shadow, tf: Transform, scale: f32) {
    let (ox, oy) = (sh.dx * scale, sh.dy * scale);
    let blur_px = sh.blur * scale;
    let mut paint = Paint::default();
    paint.set_color(sh.color.to_skia());
    paint.anti_alias = true;
    if blur_px < 1.0 {
        let t = tf.post_translate(ox, oy);
        for p in paths {
            pix.fill_path(p, &paint, FillRule::EvenOdd, t, None);
        }
        return;
    }
    // Calque à part, flouté puis posé.
    let mut bounds: Option<tiny_skia::Rect> = None;
    for p in paths {
        if let Some(b) = (*p).clone().transform(tf).map(|p| p.bounds()) {
            bounds = Some(match bounds {
                Some(a) => tiny_skia::Rect::from_ltrb(a.left().min(b.left()), a.top().min(b.top()), a.right().max(b.right()), a.bottom().max(b.bottom())).unwrap_or(a),
                None => b,
            });
        }
    }
    let Some(b) = bounds else { return };
    let pad = blur_px.ceil() + 2.0;
    let x0 = (b.left() + ox - pad).floor().max(-(pix.width() as f32));
    let y0 = (b.top() + oy - pad).floor().max(-(pix.height() as f32));
    let x1 = (b.right() + ox + pad).ceil().min(2.0 * pix.width() as f32);
    let y1 = (b.bottom() + oy + pad).ceil().min(2.0 * pix.height() as f32);
    let (w, h) = ((x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32);
    if (w as u64) * (h as u64) > 64_000_000 {
        return;
    }
    let Some(mut layer) = Pixmap::new(w, h) else { return };
    let t = tf.post_translate(ox - x0, oy - y0);
    for p in paths {
        layer.fill_path(p, &paint, FillRule::EvenOdd, t, None);
    }
    // Trois flous en boîte ≈ flou gaussien ; blurRad correspond à peu près à deux écarts-types.
    let r = ((blur_px / 2.0) * 0.9).round().max(1.0) as usize;
    let (w, h) = (w as usize, h as usize);
    let data = layer.data_mut();
    for _ in 0..3 {
        box_blur(data, w, h, r, true);
        box_blur(data, w, h, r, false);
    }
    pix.draw_pixmap(x0 as i32, y0 as i32, layer.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

fn box_blur(data: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let idx = |line: usize, i: usize| if horizontal { (line * w + i) * 4 } else { (i * w + line) * 4 };
    let mut buf = vec![[0u32; 4]; len];
    let div = (2 * r + 1) as u32;
    for line in 0..lines {
        for (i, b) in buf.iter_mut().enumerate() {
            let k = idx(line, i);
            *b = [data[k] as u32, data[k + 1] as u32, data[k + 2] as u32, data[k + 3] as u32];
        }
        let mut acc = [0u32; 4];
        // Fenêtre glissante, bords à zéro.
        for b in buf.iter().take(r.min(len)) {
            for c in 0..4 {
                acc[c] += b[c];
            }
        }
        for i in 0..len {
            if i + r < len {
                for c in 0..4 {
                    acc[c] += buf[i + r][c];
                }
            }
            if i > r {
                for c in 0..4 {
                    acc[c] -= buf[i - r - 1][c];
                }
            }
            let k = idx(line, i);
            for c in 0..4 {
                data[k + c] = (acc[c] / div) as u8;
            }
        }
    }
}
