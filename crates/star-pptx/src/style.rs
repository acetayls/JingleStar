//! Propriétés de texte héritables : caractère (`a:rPr`, `a:defRPr`), paragraphe (`a:pPr`,
//! `a:lvlNpPr`), styles de liste (`a:lstStyle`, `p:titleStyle`…) et zone de texte (`a:bodyPr`).
//!
//! Chaque champ est optionnel : `None` signifie « hérité ». Les niveaux se superposent avec
//! `merge`, du plus général (présentation, masque) au plus précis (paragraphe, caractère).

use crate::color::{child_color, ColorCtx, Rgba};
use crate::fill::parse_fill;
use crate::xml::Elem;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Caps {
    None,
    All,
    Small,
}

#[derive(Debug, Clone, Default)]
pub struct RunProps {
    /// Taille en points.
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<Rgba>,
    pub latin: Option<String>,
    pub ea: Option<String>,
    pub sym: Option<String>,
    /// Décalage vertical en fraction de la taille (exposant > 0, indice < 0).
    pub baseline: Option<f32>,
    pub caps: Option<Caps>,
    /// Espacement des caractères, en points.
    pub spacing: Option<f32>,
    pub highlight: Option<Rgba>,
}

macro_rules! merge_fields {
    ($dst:expr, $src:expr, $($f:ident),*) => {
        $( if $src.$f.is_some() { $dst.$f = $src.$f.clone(); } )*
    };
}

impl RunProps {
    pub fn merge(&mut self, o: &RunProps) {
        merge_fields!(self, o, size, bold, italic, underline, strike, color, latin, ea, sym, baseline, caps, spacing, highlight);
    }

    pub fn parse(el: &Elem, ctx: &ColorCtx) -> RunProps {
        let mut r = RunProps {
            size: el.attr_f64("sz").map(|v| v as f32 / 100.0).filter(|v| *v > 0.0),
            bold: el.attr_bool("b"),
            italic: el.attr_bool("i"),
            underline: el.attr("u").map(|u| u != "none"),
            strike: el.attr("strike").map(|s| s != "noStrike"),
            baseline: el.attr_f64("baseline").map(|v| v as f32 / 100_000.0),
            caps: el.attr("cap").map(|c| match c {
                "all" => Caps::All,
                "small" => Caps::Small,
                _ => Caps::None,
            }),
            spacing: el.attr_f64("spc").map(|v| v as f32 / 100.0),
            ..Default::default()
        };
        if let Some(f) = parse_fill(el, ctx, &[]) {
            r.color = match f {
                crate::fill::Fill::None => Some(Rgba::BLACK.with_alpha(0.0)),
                f => f.main_color(),
            };
        }
        let face = |n: &str| el.child(n).and_then(|e| e.attr("typeface")).filter(|s| !s.is_empty()).map(str::to_string);
        r.latin = face("latin");
        r.ea = face("ea");
        r.sym = face("sym");
        r.highlight = el.child("highlight").and_then(|h| child_color(h, ctx));
        r
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// Fraction de l'interligne simple (1.0 = simple).
    Pct(f32),
    /// Valeur fixe en points.
    Pts(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Bullet {
    None,
    Char(String),
    AutoNum { scheme: String, start: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BulletSize {
    /// Fraction de la taille du texte.
    Pct(f32),
    Pts(f32),
}

#[derive(Debug, Clone, Default)]
pub struct ParaProps {
    pub align: Option<Align>,
    /// Marge gauche en EMU.
    pub mar_l: Option<f32>,
    /// Retrait de première ligne en EMU (négatif : puce en retrait négatif).
    pub indent: Option<f32>,
    pub line_spacing: Option<Spacing>,
    pub space_before: Option<Spacing>,
    pub space_after: Option<Spacing>,
    pub bullet: Option<Bullet>,
    /// `Some(None)` : couleur du texte (`a:buClrTx`).
    pub bullet_color: Option<Option<Rgba>>,
    /// `Some(None)` : taille du texte (`a:buSzTx`).
    pub bullet_size: Option<Option<BulletSize>>,
    /// `Some(None)` : police du texte (`a:buFontTx`).
    pub bullet_font: Option<Option<String>>,
    pub run: RunProps,
}

fn spacing(el: Option<&Elem>) -> Option<Spacing> {
    let el = el?;
    if let Some(p) = el.child("spcPct") {
        return Some(Spacing::Pct(p.attr_f64("val")? as f32 / 100_000.0));
    }
    if let Some(p) = el.child("spcPts") {
        return Some(Spacing::Pts(p.attr_f64("val")? as f32 / 100.0));
    }
    None
}

impl ParaProps {
    pub fn merge(&mut self, o: &ParaProps) {
        merge_fields!(self, o, align, mar_l, indent, line_spacing, space_before, space_after, bullet, bullet_color, bullet_size, bullet_font);
        self.run.merge(&o.run);
    }

    /// `a:pPr` ou `a:lvlNpPr` (avec son `a:defRPr`).
    pub fn parse(el: &Elem, ctx: &ColorCtx) -> ParaProps {
        let mut p = ParaProps {
            align: el.attr("algn").map(|a| match a {
                "ctr" => Align::Center,
                "r" => Align::Right,
                "just" | "dist" | "justLow" | "thaiDist" => Align::Justify,
                _ => Align::Left,
            }),
            mar_l: el.attr_f64("marL").map(|v| v as f32),
            indent: el.attr_f64("indent").map(|v| v as f32),
            line_spacing: spacing(el.child("lnSpc")),
            space_before: spacing(el.child("spcBef")),
            space_after: spacing(el.child("spcAft")),
            ..Default::default()
        };
        for c in &el.children {
            match c.name.as_str() {
                "buNone" => p.bullet = Some(Bullet::None),
                "buChar" => p.bullet = Some(Bullet::Char(c.attr("char").unwrap_or("•").to_string())),
                "buAutoNum" => {
                    p.bullet = Some(Bullet::AutoNum {
                        scheme: c.attr("type").unwrap_or("arabicPeriod").to_string(),
                        start: c.attr_i64("startAt").unwrap_or(1).clamp(1, 32767) as u32,
                    })
                }
                "buBlip" => p.bullet = Some(Bullet::Char("•".into())),
                "buClrTx" => p.bullet_color = Some(None),
                "buClr" => p.bullet_color = Some(child_color(c, ctx)),
                "buSzTx" => p.bullet_size = Some(None),
                "buSzPct" => p.bullet_size = c.attr_f64("val").map(|v| Some(BulletSize::Pct(v as f32 / 100_000.0))),
                "buSzPts" => p.bullet_size = c.attr_f64("val").map(|v| Some(BulletSize::Pts(v as f32 / 100.0))),
                "buFontTx" => p.bullet_font = Some(None),
                "buFont" => p.bullet_font = Some(c.attr("typeface").map(str::to_string)),
                "defRPr" => p.run = RunProps::parse(c, ctx),
                _ => {}
            }
        }
        p
    }
}

/// Styles des 9 niveaux d'une liste.
#[derive(Debug, Clone, Default)]
pub struct ListStyle(pub [ParaProps; 9]);

impl ListStyle {
    /// Superpose un `a:lstStyle` (ou `p:titleStyle`…) : `a:defPPr` vaut pour tous les niveaux.
    pub fn apply(&mut self, el: Option<&Elem>, ctx: &ColorCtx) {
        let Some(el) = el else { return };
        if let Some(d) = el.child("defPPr") {
            let d = ParaProps::parse(d, ctx);
            for l in self.0.iter_mut() {
                l.merge(&d);
            }
        }
        for (i, l) in self.0.iter_mut().enumerate() {
            if let Some(e) = el.child(&format!("lvl{}pPr", i + 1)) {
                l.merge(&ParaProps::parse(e, ctx));
            }
        }
    }

    /// Couleur de texte imposée à tous les niveaux (`p:style/a:fontRef`).
    pub fn set_color(&mut self, c: Rgba) {
        for l in self.0.iter_mut() {
            l.run.color = Some(c);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Default)]
pub struct BodyProps {
    /// Marges intérieures en EMU (gauche, haut, droite, bas).
    pub ins_l: Option<f32>,
    pub ins_t: Option<f32>,
    pub ins_r: Option<f32>,
    pub ins_b: Option<f32>,
    pub anchor: Option<Anchor>,
    pub anchor_ctr: Option<bool>,
    pub wrap: Option<bool>,
    /// (échelle de police, réduction d'interligne) de `a:normAutofit`.
    pub autofit: Option<(f32, f32)>,
    pub vert: Option<String>,
    /// Rotation du texte en degrés.
    pub rot: Option<f32>,
    pub upright: Option<bool>,
}

impl BodyProps {
    pub fn merge(&mut self, o: &BodyProps) {
        merge_fields!(self, o, ins_l, ins_t, ins_r, ins_b, anchor, anchor_ctr, wrap, autofit, vert, rot, upright);
    }

    pub fn parse(el: &Elem) -> BodyProps {
        let ins = |n: &str| el.attr_f64(n).map(|v| v as f32);
        let mut b = BodyProps {
            ins_l: ins("lIns"),
            ins_t: ins("tIns"),
            ins_r: ins("rIns"),
            ins_b: ins("bIns"),
            anchor: el.attr("anchor").map(|a| match a {
                "ctr" => Anchor::Center,
                "b" => Anchor::Bottom,
                _ => Anchor::Top,
            }),
            anchor_ctr: el.attr_bool("anchorCtr"),
            wrap: el.attr("wrap").map(|w| w != "none"),
            vert: el.attr("vert").map(str::to_string),
            rot: el.attr_f64("rot").map(|r| r as f32 / 60_000.0),
            upright: el.attr_bool("upright"),
            ..Default::default()
        };
        if let Some(a) = el.child("normAutofit") {
            let scale = a.attr_f64("fontScale").unwrap_or(100_000.0) as f32 / 100_000.0;
            let red = a.attr_f64("lnSpcReduction").unwrap_or(0.0) as f32 / 100_000.0;
            b.autofit = Some((scale.clamp(0.05, 1.0), red.clamp(0.0, 0.9)));
        } else if el.child("noAutofit").is_some() || el.child("spAutoFit").is_some() {
            b.autofit = Some((1.0, 0.0));
        }
        b
    }
}
