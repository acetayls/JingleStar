//! Remplissages (`a:solidFill`, `a:gradFill`, `a:blipFill`, `a:pattFill`…) et traits (`a:ln`).

use crate::color::{child_color, ColorCtx, Rgba};
use crate::package::Rel;
use crate::theme::Theme;
use crate::xml::Elem;

#[derive(Debug, Clone)]
pub enum Fill {
    None,
    Solid(Rgba),
    Gradient(Gradient),
    Image(ImageFill),
    /// Remplissage du groupe parent.
    Group,
}

#[derive(Debug, Clone)]
pub struct Gradient {
    /// (position 0..1, couleur), triés.
    pub stops: Vec<(f32, Rgba)>,
    pub kind: GradKind,
}

#[derive(Debug, Clone)]
pub enum GradKind {
    /// Angle en degrés (sens horaire), `scaled` : l'angle suit les proportions de la forme.
    Linear { angle: f32, scaled: bool },
    /// Dégradé « chemin » (cercle, rectangle, forme) : centre décrit par `fillToRect` (l, t, r, b).
    Path { to_rect: [f32; 4] },
}

#[derive(Debug, Clone)]
pub struct ImageFill {
    /// Nom de la partie image dans l'archive.
    pub part: String,
    /// Recadrage (l, t, r, b) en fractions de l'image (négatif : marge).
    pub crop: [f32; 4],
    pub alpha: f32,
    /// Mosaïque : (échelle x, échelle y, décalage x, décalage y en EMU).
    pub tile: Option<(f32, f32, f32, f32)>,
    pub grayscale: bool,
}

impl Fill {
    /// Couleur représentative (texte ou puce dont le remplissage est un dégradé…).
    pub fn main_color(&self) -> Option<Rgba> {
        match self {
            Fill::Solid(c) => Some(*c),
            Fill::Gradient(g) => g.stops.first().map(|s| s.1),
            _ => None,
        }
    }
}

const FILL_ELEMS: &[&str] = &["noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill"];

/// Premier remplissage décrit dans `parent` (ex. `p:spPr`, `p:bgPr`, `a:tcPr`, `a:rPr`).
pub fn parse_fill(parent: &Elem, ctx: &ColorCtx, rels: &[Rel]) -> Option<Fill> {
    let el = parent.children.iter().find(|c| FILL_ELEMS.contains(&c.name.as_str()))?;
    parse_fill_elem(el, ctx, rels)
}

pub fn parse_fill_elem(el: &Elem, ctx: &ColorCtx, rels: &[Rel]) -> Option<Fill> {
    Some(match el.name.as_str() {
        "noFill" => Fill::None,
        "solidFill" => Fill::Solid(child_color(el, ctx).unwrap_or(Rgba::BLACK)),
        "grpFill" => Fill::Group,
        "gradFill" => {
            let mut stops: Vec<(f32, Rgba)> = el
                .child("gsLst")
                .map(|l| {
                    l.children("gs")
                        .filter_map(|gs| Some(((gs.attr_f64("pos").unwrap_or(0.0) / 100_000.0) as f32, child_color(gs, ctx)?)))
                        .collect()
                })
                .unwrap_or_default();
            if stops.is_empty() {
                return Some(Fill::None);
            }
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            let kind = if let Some(p) = el.child("path") {
                let r = p.child("fillToRect");
                let f = |n: &str| r.and_then(|r| r.attr_f64(n)).unwrap_or(50_000.0) as f32 / 100_000.0;
                GradKind::Path { to_rect: [f("l"), f("t"), f("r"), f("b")] }
            } else {
                let lin = el.child("lin");
                GradKind::Linear {
                    angle: lin.and_then(|l| l.attr_f64("ang")).unwrap_or(0.0) as f32 / 60_000.0,
                    scaled: lin.and_then(|l| l.attr_bool("scaled")).unwrap_or(false),
                }
            };
            Fill::Gradient(Gradient { stops, kind })
        }
        "blipFill" => match image_fill(el, rels) {
            Some(f) => Fill::Image(f),
            None => Fill::None,
        },
        "pattFill" => {
            let fg = el.child("fgClr").and_then(|c| child_color(c, ctx)).unwrap_or(Rgba::BLACK);
            let bg = el.child("bgClr").and_then(|c| child_color(c, ctx)).unwrap_or(Rgba::WHITE);
            // Motif rendu en teinte moyenne : à l'écran, la différence est faible.
            let d = pattern_density(el.attr("prst").unwrap_or("pct50"));
            let mix = |a: f32, b: f32| a * d + b * (1.0 - d);
            Fill::Solid(Rgba { r: mix(fg.r, bg.r), g: mix(fg.g, bg.g), b: mix(fg.b, bg.b), a: mix(fg.a, bg.a) })
        }
        _ => return None,
    })
}

fn pattern_density(prst: &str) -> f32 {
    if let Some(p) = prst.strip_prefix("pct").and_then(|v| v.parse::<f32>().ok()) {
        return p / 100.0;
    }
    if prst.starts_with("dk") || prst.starts_with("solid") {
        0.5
    } else if prst.starts_with("lt") || prst.starts_with("narrow") || prst.starts_with("dash") {
        0.25
    } else {
        0.35
    }
}

/// Image d'un `blipFill` (`a:blip r:embed`), recadrage et mosaïque.
pub fn image_fill(blip_fill: &Elem, rels: &[Rel]) -> Option<ImageFill> {
    let blip = blip_fill.child("blip")?;
    let id = blip.attr("r:embed").or_else(|| blip.attr("r:link"))?;
    let rel = rels.iter().find(|r| r.id == id && !r.external)?;
    let mut alpha = 1.0;
    let mut grayscale = false;
    for e in &blip.children {
        match e.name.as_str() {
            "alphaModFix" => alpha = (e.attr_f64("amt").unwrap_or(100_000.0) / 100_000.0) as f32,
            "grayscl" => grayscale = true,
            _ => {}
        }
    }
    let src = blip_fill.child("srcRect");
    let f = |n: &str| src.and_then(|s| s.attr_f64(n)).unwrap_or(0.0) as f32 / 100_000.0;
    let tile = blip_fill.child("tile").map(|t| {
        let g = |n: &str, d: f64| t.attr_f64(n).unwrap_or(d) as f32;
        (g("sx", 100_000.0) / 100_000.0, g("sy", 100_000.0) / 100_000.0, g("tx", 0.0), g("ty", 0.0))
    });
    Some(ImageFill { part: rel.target.clone(), crop: [f("l"), f("t"), f("r"), f("b")], alpha: alpha.clamp(0.0, 1.0), tile, grayscale })
}

/// Remplissage d'après une référence de style (`a:fillRef idx` / `p:bgRef idx`) du thème.
pub fn fill_from_ref(theme: &Theme, r: &Elem, ctx: &ColorCtx, rels: &[Rel]) -> Option<Fill> {
    let idx = r.attr_i64("idx")?;
    let ph = child_color(r, ctx);
    let ctx = ColorCtx { ph_clr: ph.or(ctx.ph_clr), ..*ctx };
    let style = match idx {
        0 => return Some(Fill::None),
        1..=999 => theme.fill_styles.get(idx as usize - 1),
        _ => theme.bg_fill_styles.get(idx as usize - 1001),
    };
    match style {
        Some(s) => parse_fill_elem(s, &ctx, rels),
        None => ph.map(Fill::Solid),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arrow {
    Triangle,
    Stealth,
    Open,
    Oval,
    Diamond,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub fill: Fill,
    /// Épaisseur en EMU.
    pub width: f32,
    /// Motif de tirets en multiples de l'épaisseur.
    pub dash: Option<Vec<f32>>,
    pub round_cap: bool,
    pub head: Option<(Arrow, f32, f32)>,
    pub tail: Option<(Arrow, f32, f32)>,
}

impl Default for Line {
    fn default() -> Self {
        Line { fill: Fill::None, width: 9525.0, dash: None, round_cap: false, head: None, tail: None }
    }
}

/// Complète `base` avec les réglages d'un `a:ln`.
pub fn parse_line(ln: &Elem, ctx: &ColorCtx, rels: &[Rel], base: Line) -> Line {
    let mut l = base;
    if let Some(w) = ln.attr_f64("w") {
        l.width = w as f32;
    }
    if let Some(f) = parse_fill(ln, ctx, rels) {
        l.fill = f;
    }
    if let Some(c) = ln.attr("cap") {
        l.round_cap = c == "rnd";
    }
    if let Some(d) = ln.child("prstDash").and_then(|d| d.attr("val")) {
        l.dash = match d {
            "solid" => None,
            "dot" | "sysDot" => Some(vec![1.0, 1.0]),
            "sysDash" => Some(vec![3.0, 1.0]),
            "dash" => Some(vec![4.0, 3.0]),
            "dashDot" | "sysDashDot" => Some(vec![4.0, 3.0, 1.0, 3.0]),
            "lgDash" => Some(vec![8.0, 3.0]),
            "lgDashDot" => Some(vec![8.0, 3.0, 1.0, 3.0]),
            "lgDashDotDot" | "sysDashDotDot" => Some(vec![8.0, 3.0, 1.0, 3.0, 1.0, 3.0]),
            _ => None,
        };
    }
    let arrow = |e: &Elem| -> Option<(Arrow, f32, f32)> {
        let kind = match e.attr("type")? {
            "triangle" => Arrow::Triangle,
            "stealth" => Arrow::Stealth,
            "arrow" => Arrow::Open,
            "oval" => Arrow::Oval,
            "diamond" => Arrow::Diamond,
            _ => return None,
        };
        let size = |n: &str| match e.attr(n) {
            Some("sm") => 2.0,
            Some("lg") => 5.0,
            _ => 3.0,
        };
        Some((kind, size("w"), size("len")))
    };
    if let Some(h) = ln.child("headEnd") {
        l.head = arrow(h);
    }
    if let Some(t) = ln.child("tailEnd") {
        l.tail = arrow(t);
    }
    l
}

/// Trait d'après une référence de style (`a:lnRef idx`).
pub fn line_from_ref(theme: &Theme, r: &Elem, ctx: &ColorCtx, rels: &[Rel]) -> Option<Line> {
    let idx = r.attr_i64("idx")?;
    if idx <= 0 {
        return None;
    }
    let ph = child_color(r, ctx);
    let ctx = ColorCtx { ph_clr: ph.or(ctx.ph_clr), ..*ctx };
    let ln = theme.line_styles.get(idx as usize - 1)?;
    let mut base = Line::default();
    if let Some(c) = ph {
        base.fill = Fill::Solid(c);
    }
    Some(parse_line(ln, &ctx, rels, base))
}
