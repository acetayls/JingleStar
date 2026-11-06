//! Texte : paragraphes et séries de caractères (`a:p`, `a:r`), mise en page avec cosmic-text
//! (mise en forme, coupure des lignes, polices de repli), tracé des glyphes en contours.
//!
//! Toute la mise en page se fait en points ; la transformation finale passe en pixels.

use crate::color::{ColorCtx, Rgba};
use crate::fonts::{map_symbol_char, Fonts};
use crate::style::{Align, Bullet, BulletSize, Caps, ListStyle, ParaProps, RunProps, Spacing};
use crate::theme::Theme;
use crate::xml::Elem;
use cosmic_text::{Attrs, Buffer, CacheKeyFlags, Family, Metrics, Shaping, Style, Weight, Wrap};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

pub const EMU_PER_PT: f32 = 12_700.0;

/// Taille par défaut d'un texte sans aucune indication (18 pt).
const DEFAULT_SIZE: f32 = 18.0;

pub struct Run {
    pub text: String,
    pub props: RunProps,
}

pub struct Para {
    /// Niveau de liste (0 à 8).
    pub level: usize,
    pub props: ParaProps,
    pub runs: Vec<Run>,
    /// Propriétés de fin de paragraphe (taille d'un paragraphe vide).
    pub end: RunProps,
}

impl Para {
    pub fn has_text(&self) -> bool {
        self.runs.iter().any(|r| !r.text.trim().is_empty())
    }
}

/// Paragraphes d'un `txBody`, propriétés héritées résolues avec `levels`.
pub fn collect_paragraphs(tx_body: &Elem, levels: &ListStyle, ctx: &ColorCtx, slide_num: usize) -> Vec<Para> {
    let mut out = Vec::new();
    for p in tx_body.children("p") {
        let ppr = p.child("pPr");
        let lvl = ppr.and_then(|e| e.attr_i64("lvl")).unwrap_or(0).clamp(0, 8) as usize;
        let mut props = levels.0[lvl].clone();
        if let Some(e) = ppr {
            props.merge(&ParaProps::parse(e, ctx));
        }
        let mut runs = Vec::new();
        collect_runs(p, &props.run, ctx, slide_num, &mut runs);
        let mut end = props.run.clone();
        if let Some(e) = p.child("endParaRPr") {
            end.merge(&RunProps::parse(e, ctx));
        }
        out.push(Para { level: lvl, props, runs, end });
    }
    out
}

fn collect_runs(p: &Elem, base: &RunProps, ctx: &ColorCtx, slide_num: usize, runs: &mut Vec<Run>) {
    for c in &p.children {
        let run_props = || {
            let mut r = base.clone();
            if let Some(e) = c.child("rPr") {
                r.merge(&RunProps::parse(e, ctx));
            }
            r
        };
        match c.name.as_str() {
            "r" => {
                let text = c.child("t").map(|t| t.text.clone()).unwrap_or_default();
                runs.push(Run { text, props: run_props() });
            }
            "br" => runs.push(Run { text: "\n".into(), props: run_props() }),
            "fld" => {
                let mut text = c.child("t").map(|t| t.text.clone()).unwrap_or_default();
                if c.attr("type") == Some("slidenum") {
                    text = slide_num.to_string();
                }
                runs.push(Run { text, props: run_props() });
            }
            "AlternateContent" => {
                if let Some(alt) = c.child("Fallback").or_else(|| c.child("Choice")) {
                    collect_runs(alt, base, ctx, slide_num, runs);
                }
            }
            _ => {}
        }
    }
}

/// Style d'affichage d'un glyphe (indexé par la métadonnée cosmic-text).
#[derive(Clone)]
struct GlyphStyle {
    color: Rgba,
    underline: bool,
    strike: bool,
    /// Décalage vers le haut en points (exposant).
    rise: f32,
    size: f32,
    highlight: Option<Rgba>,
}

struct LaidBullet {
    buf: Buffer,
    x: f32,
    color: Rgba,
}

struct LaidPara {
    buf: Buffer,
    top: f32,
    left: f32,
    /// Largeur disponible pour l'alignement quand le texte ne revient pas à la ligne.
    avail: f32,
    align: Align,
    wrap: bool,
    styles: Vec<GlyphStyle>,
    bullet: Option<LaidBullet>,
}

/// Bloc de texte mis en page, en points, origine en haut à gauche de la zone utile.
pub struct Laid {
    paras: Vec<LaidPara>,
    pub height: f32,
}

pub struct LayoutOpts {
    /// Largeur utile en points.
    pub width: f32,
    pub wrap: bool,
    pub font_scale: f32,
    pub line_reduction: f32,
}

struct Counter {
    level: usize,
    scheme: String,
    n: u32,
}

/// Famille et graisse d'un texte, d'après la police latine (ou celle du thème).
fn family_for(fonts: &mut Fonts, theme: &Theme, props: &RunProps) -> String {
    let face = props.latin.as_deref().and_then(|f| theme.resolve_font(f)).or_else(|| theme.minor_latin.clone()).unwrap_or_else(|| "Calibri".into());
    fonts.resolve(&face)
}

pub fn layout(paras: &[Para], opts: &LayoutOpts, fonts: &mut Fonts, theme: &Theme) -> Laid {
    let mut laid = Vec::with_capacity(paras.len());
    let mut y = 0.0f32;
    let mut counters: Vec<Counter> = Vec::new();
    for (i, para) in paras.iter().enumerate() {
        let pp = &para.props;
        let scale = opts.font_scale;
        let first = para.runs.iter().find(|r| !r.text.is_empty() && r.text != "\n").map(|r| &r.props).unwrap_or(&para.end);
        let first_size = first.size.unwrap_or(DEFAULT_SIZE) * scale;
        let end_size = para.end.size.unwrap_or(DEFAULT_SIZE) * scale;
        let first_family = family_for(fonts, theme, first);
        let (ratio, _) = fonts.line_metrics(&first_family, first.bold.unwrap_or(false));

        let line_pct = |size: f32, ratio: f32| -> f32 {
            match pp.line_spacing {
                Some(Spacing::Pts(v)) => v * scale,
                Some(Spacing::Pct(p)) => size * ratio * (p - opts.line_reduction).max(0.1),
                None => size * ratio * (1.0 - opts.line_reduction).max(0.1),
            }
        };
        let para_spacing = |s: Option<Spacing>| match s {
            Some(Spacing::Pts(v)) => v,
            Some(Spacing::Pct(p)) => p * first_size * ratio,
            None => 0.0,
        };
        if i > 0 {
            y += para_spacing(pp.space_before);
        }

        let mar_l = pp.mar_l.unwrap_or(0.0) / EMU_PER_PT;
        let indent = pp.indent.unwrap_or(0.0) / EMU_PER_PT;
        let avail = (opts.width - mar_l).max(1.0);

        // Séries : texte, attributs cosmic-text, style de dessin.
        let mut styles = Vec::new();
        let mut spans: Vec<(String, String, RunProps, f32)> = Vec::new();
        for r in &para.runs {
            if r.text.is_empty() {
                continue;
            }
            let p = &r.props;
            let mut size = p.size.unwrap_or(DEFAULT_SIZE) * scale;
            let mut text = r.text.replace(['\r', '\u{000B}'], "\n").replace('\t', "    ");
            match p.caps {
                Some(Caps::All) => text = text.to_uppercase(),
                Some(Caps::Small) => {
                    text = text.to_uppercase();
                    size *= 0.8;
                }
                _ => {}
            }
            let baseline = p.baseline.unwrap_or(0.0);
            let rise = baseline * size;
            if baseline != 0.0 {
                size *= 0.66;
            }
            let family = family_for(fonts, theme, p);
            let text = if p.sym.is_some() || p.latin.as_deref().map(|f| f.to_lowercase().starts_with("wingdings") || f == "Symbol").unwrap_or(false) {
                map_symbol_char(p.latin.as_deref().or(p.sym.as_deref()), &text)
            } else {
                text
            };
            styles.push(GlyphStyle {
                color: p.color.unwrap_or(Rgba::BLACK),
                underline: p.underline.unwrap_or(false),
                strike: p.strike.unwrap_or(false),
                rise,
                size,
                highlight: p.highlight,
            });
            spans.push((text, family, p.clone(), size));
        }
        let end_family = family_for(fonts, theme, &para.end);
        let (end_ratio, _) = fonts.line_metrics(&end_family, para.end.bold.unwrap_or(false));
        let mut buf = Buffer::new_empty(Metrics::new(end_size.max(0.5), line_pct(end_size, end_ratio).max(0.5)));
        buf.set_wrap(if opts.wrap { Wrap::WordOrGlyph } else { Wrap::None });
        buf.set_size(if opts.wrap { Some(avail) } else { None }, None);
        let mut line_heights = Vec::with_capacity(spans.len());
        for (_, family, p, size) in &spans {
            let (r, _) = fonts.line_metrics(family, p.bold.unwrap_or(false));
            line_heights.push(line_pct(*size, r).max(0.5));
        }
        let attrs: Vec<Attrs> = spans
            .iter()
            .enumerate()
            .map(|(k, (_, family, p, size))| {
                let mut a = Attrs::new()
                    .family(Family::Name(family))
                    .weight(if p.bold.unwrap_or(false) { Weight::BOLD } else { Weight::NORMAL })
                    .style(if p.italic.unwrap_or(false) { Style::Italic } else { Style::Normal })
                    .metadata(k)
                    .metrics(Metrics::new(size.max(0.5), line_heights[k]));
                if let Some(s) = p.spacing.filter(|s| *s != 0.0) {
                    a = a.letter_spacing(s / size.max(0.5));
                }
                a
            })
            .collect();
        let align = pp.align.unwrap_or(Align::Left);
        let ct_align = match align {
            Align::Left => cosmic_text::Align::Left,
            Align::Center => cosmic_text::Align::Center,
            Align::Right => cosmic_text::Align::Right,
            Align::Justify => cosmic_text::Align::Justified,
        };
        let default_attrs = Attrs::new().family(Family::Name(&end_family)).metrics(Metrics::new(end_size.max(0.5), line_pct(end_size, end_ratio).max(0.5)));
        buf.set_rich_text(spans.iter().zip(attrs.iter()).map(|((t, ..), a)| (t.as_str(), a.clone())), &default_attrs, Shaping::Advanced, if opts.wrap { Some(ct_align) } else { None });
        buf.shape_until_scroll(&mut fonts.system, false);

        // Puce ou numéro.
        let level = para.level;
        let bullet_text = match &pp.bullet {
            Some(Bullet::Char(c)) if para.has_text() => {
                let font = match &pp.bullet_font {
                    Some(Some(f)) => Some(f.as_str()),
                    _ => first.latin.as_deref(),
                };
                Some(map_symbol_char(font, c))
            }
            Some(Bullet::AutoNum { scheme, start }) if para.has_text() => {
                counters.retain(|c| c.level <= level);
                let n = match counters.iter_mut().find(|c| c.level == level) {
                    Some(c) if &c.scheme == scheme => {
                        c.n += 1;
                        c.n
                    }
                    Some(c) => {
                        c.scheme = scheme.clone();
                        c.n = *start;
                        c.n
                    }
                    None => {
                        counters.push(Counter { level, scheme: scheme.clone(), n: *start });
                        *start
                    }
                };
                Some(autonum(scheme, n))
            }
            _ => {
                if para.has_text() {
                    counters.retain(|c| c.level < level);
                }
                None
            }
        };
        let bullet = bullet_text.map(|text| {
            let size = match pp.bullet_size {
                Some(Some(BulletSize::Pct(p))) => first_size * p,
                Some(Some(BulletSize::Pts(v))) => v * scale,
                _ => first_size,
            }
            .max(0.5);
            let family = match &pp.bullet_font {
                Some(Some(f)) if !f.to_lowercase().starts_with("wingdings") && f != "Symbol" => fonts.resolve(theme.resolve_font(f).as_deref().unwrap_or(f)),
                _ => first_family.clone(),
            };
            let color = match pp.bullet_color {
                Some(Some(c)) => c,
                _ => first.color.unwrap_or(Rgba::BLACK),
            };
            let mut b = Buffer::new_empty(Metrics::new(size, size * 1.2));
            b.set_wrap(Wrap::None);
            b.set_size(None, None);
            let is_num = matches!(pp.bullet, Some(Bullet::AutoNum { .. }));
            let attrs = Attrs::new()
                .family(Family::Name(&family))
                .weight(if is_num && first.bold.unwrap_or(false) { Weight::BOLD } else { Weight::NORMAL })
                .metrics(Metrics::new(size, size * 1.2));
            b.set_rich_text([(text.as_str(), attrs.clone())], &attrs, Shaping::Advanced, None);
            b.shape_until_scroll(&mut fonts.system, false);
            LaidBullet { buf: b, x: (mar_l + indent).max(0.0), color }
        });

        let mut h = 0.0;
        for run in buf.layout_runs() {
            h += run.line_height;
        }
        if h == 0.0 {
            h = line_pct(end_size, end_ratio);
        }
        laid.push(LaidPara { buf, top: y, left: mar_l, avail, align, wrap: opts.wrap, styles, bullet });
        y += h;
        if i + 1 < paras.len() {
            y += para_spacing(pp.space_after);
        }
    }
    Laid { paras: laid, height: y }
}

/// Texte d'un numéro automatique (`arabicPeriod`, `alphaUcParenR`, `romanLcPeriod`…).
pub fn autonum(scheme: &str, n: u32) -> String {
    let (body, rest) = if let Some(r) = scheme.strip_prefix("arabic") {
        (n.to_string(), r)
    } else if let Some(r) = scheme.strip_prefix("alphaLc") {
        (alpha(n).to_lowercase(), r)
    } else if let Some(r) = scheme.strip_prefix("alphaUc") {
        (alpha(n), r)
    } else if let Some(r) = scheme.strip_prefix("romanLc") {
        (roman(n).to_lowercase(), r)
    } else if let Some(r) = scheme.strip_prefix("romanUc") {
        (roman(n), r)
    } else {
        (n.to_string(), "Period")
    };
    match rest {
        "ParenR" => format!("{body})"),
        "ParenBoth" => format!("({body})"),
        "Plain" => body,
        "Minus" => format!("- {body} -"),
        _ => format!("{body}."),
    }
}

fn alpha(n: u32) -> String {
    // a…z, puis aa, bb… comme PowerPoint.
    let n = n.max(1) - 1;
    let c = (b'A' + (n % 26) as u8) as char;
    std::iter::repeat_n(c, (n / 26) as usize + 1).collect()
}

fn roman(mut n: u32) -> String {
    let table = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c.to_skia());
    p.anti_alias = true;
    p
}

/// Ajoute un tracé au lot de sa couleur.
type AddPath<'a> = dyn FnMut(Rgba, &mut dyn FnMut(&mut PathBuilder)) + 'a;

/// Glyphes d'un tampon cosmic-text ajoutés au tracé de leur couleur.
#[allow(clippy::too_many_arguments)]
fn draw_buffer(
    pixmap: &mut Pixmap,
    fonts: &mut Fonts,
    buf: &Buffer,
    origin: (f32, f32),
    line_offset: &dyn Fn(f32) -> f32,
    styles: &[GlyphStyle],
    fixed_color: Option<Rgba>,
    tf: Transform,
) {
    let mut batches: Vec<(Rgba, PathBuilder)> = Vec::new();
    let mut add = |color: Rgba, f: &mut dyn FnMut(&mut PathBuilder)| {
        let key = color;
        let idx = match batches.iter().position(|(c, _)| *c == key) {
            Some(i) => i,
            None => {
                batches.push((key, PathBuilder::new()));
                batches.len() - 1
            }
        };
        f(&mut batches[idx].1);
    };
    let mut highlights: Vec<(Rgba, Rect)> = Vec::new();
    for run in buf.layout_runs() {
        let dx = line_offset(run.line_w);
        let baseline = origin.1 + run.line_y;
        let mut deco: Option<(usize, f32, f32)> = None; // (style, x0, x1)
        let flush_deco = |d: Option<(usize, f32, f32)>, add: &mut AddPath| {
            if let Some((si, x0, x1)) = d {
                let st = &styles[si];
                let mut lines = Vec::new();
                if st.underline {
                    lines.push((baseline - st.rise + st.size * 0.1, st.size * 0.055));
                }
                if st.strike {
                    lines.push((baseline - st.rise - st.size * 0.28, st.size * 0.05));
                }
                for (y, t) in lines {
                    if let Some(r) = Rect::from_xywh(x0, y, (x1 - x0).max(0.1), t.max(0.3)) {
                        add(st.color, &mut |pb: &mut PathBuilder| pb.push_rect(r));
                    }
                }
            }
        };
        for g in run.glyphs {
            let st = styles.get(g.metadata);
            let color = fixed_color.or(st.map(|s| s.color)).unwrap_or(Rgba::BLACK);
            let rise = st.map(|s| s.rise).unwrap_or(0.0);
            let x = origin.0 + dx + g.x + g.font_size * g.x_offset;
            let y = baseline + g.y - g.font_size * g.y_offset - rise;
            if let Some(hl) = st.and_then(|s| s.highlight) {
                if let Some(r) = Rect::from_xywh(origin.0 + dx + g.x, origin.1 + run.line_top, g.w.max(0.1), run.line_height.max(0.1)) {
                    highlights.push((hl, r));
                }
            }
            if fixed_color.is_none() {
                if let Some(s) = st {
                    let decorated = s.underline || s.strike;
                    let x0 = origin.0 + dx + g.x;
                    let x1 = x0 + g.w;
                    match &mut deco {
                        Some((si, _, e)) if *si == g.metadata => *e = x1,
                        _ => {
                            flush_deco(deco.take(), &mut add);
                            if decorated {
                                deco = Some((g.metadata, x0, x1));
                            }
                        }
                    }
                }
            }
            if color.a <= 0.0 {
                continue;
            }
            let italic = g.cache_key_flags.contains(CacheKeyFlags::FAKE_ITALIC);
            let bold = g.font_weight.0 >= 600;
            if let Some(path) = fonts.glyph_path(g.font_id, g.font_weight, g.glyph_id, g.font_size, italic, bold) {
                if let Some(p) = path.transform(Transform::from_translate(x, y)) {
                    add(color, &mut |pb: &mut PathBuilder| pb.push_path(&p));
                }
            }
        }
        flush_deco(deco.take(), &mut add);
    }
    for (c, r) in highlights {
        pixmap.fill_rect(r, &paint(c), tf, None);
    }
    for (c, pb) in batches {
        if let Some(path) = pb.finish() {
            pixmap.fill_path(&path, &paint(c), FillRule::Winding, tf, None);
        }
    }
}

/// Dessine le texte ; `tf` passe du repère du bloc (points) aux pixels.
pub fn draw(laid: &Laid, pixmap: &mut Pixmap, fonts: &mut Fonts, tf: Transform) {
    for p in &laid.paras {
        let (align, wrap, avail) = (p.align, p.wrap, p.avail);
        let offset = move |line_w: f32| -> f32 {
            if wrap {
                return 0.0;
            }
            match align {
                Align::Center => (avail - line_w) / 2.0,
                Align::Right => avail - line_w,
                _ => 0.0,
            }
        };
        if let Some(b) = &p.bullet {
            // La puce s'aligne sur la ligne de base de la première ligne.
            let first = p.buf.layout_runs().next().map(|r| r.line_y).unwrap_or(0.0);
            let bfirst = b.buf.layout_runs().next().map(|r| r.line_y).unwrap_or(0.0);
            // Texte centré ou à droite : la puce précède le début du texte.
            let start = p.buf.layout_runs().next().map(|r| offset(r.line_w) + r.glyphs.first().map(|g| g.x).unwrap_or(0.0)).unwrap_or(0.0);
            let bx = b.x + start;
            let zero = |_: f32| 0.0;
            draw_buffer(pixmap, fonts, &b.buf, (bx, p.top + first - bfirst), &zero, &[], Some(b.color), tf);
        }
        draw_buffer(pixmap, fonts, &p.buf, (p.left, p.top), &offset, &p.styles, None, tf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering() {
        assert_eq!(autonum("arabicPeriod", 3), "3.");
        assert_eq!(autonum("alphaLcParenR", 2), "b)");
        assert_eq!(autonum("romanUcPeriod", 4), "IV.");
        assert_eq!(autonum("arabicParenBoth", 1), "(1)");
        assert_eq!(autonum("alphaUcPeriod", 27), "AA.");
    }
}
