//! Rendu d'une diapositive : chaîne d'héritage diapositive → disposition → masque → thème,
//! fond, formes, images, groupes, tableaux, SmartArt (dessin précalculé).

use crate::color::{child_color, ClrMap, ColorCtx, Rgba};
use crate::error::Result;
use crate::fill::{fill_from_ref, image_fill, line_from_ref, parse_fill, parse_line, Arrow, Fill, GradKind, ImageFill, Line};
use crate::fonts::Fonts;
use crate::geom;
use crate::package::Rel;
use crate::presentation::Presentation;
use crate::style::{Anchor, BodyProps, ListStyle, RunProps};
use crate::text::{self, LayoutOpts, EMU_PER_PT};
use crate::theme::Theme;
use crate::xml::Elem;
use image::RgbaImage;
use std::sync::Arc;
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, IntSize, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pattern, Pixmap, Point, RadialGradient, Rect,
    SpreadMode, Stroke, StrokeDash, Transform,
};

/// Profondeur maximale des groupes imbriqués (fichiers malformés).
const MAX_DEPTH: usize = 32;

/// Une partie (diapositive, disposition, masque, dessin SmartArt) et ses relations.
#[derive(Clone)]
struct Part {
    xml: Arc<Elem>,
    rels: Arc<Vec<Rel>>,
}

impl Part {
    fn load(pres: &Presentation, name: &str) -> Result<Part> {
        Ok(Part { xml: pres.pkg.xml(name)?, rels: pres.pkg.rels(name) })
    }
    fn rel(&self, kind: &str) -> Option<&Rel> {
        self.rels.iter().find(|r| r.kind == kind && !r.external)
    }
    fn tree(&self) -> Option<&Elem> {
        self.xml.path(&["cSld", "spTree"])
    }
}

/// Clé d'un espace réservé (`p:ph`).
#[derive(Clone, Debug)]
struct PhKey {
    ty: String,
    idx: Option<String>,
}

fn nv_props(el: &Elem) -> Option<&Elem> {
    el.children.iter().find(|c| c.name.starts_with("nv"))
}

fn ph_key(el: &Elem) -> Option<PhKey> {
    let ph = nv_props(el)?.path(&["nvPr", "ph"])?;
    Some(PhKey { ty: ph.attr("type").unwrap_or("obj").to_string(), idx: ph.attr("idx").map(str::to_string) })
}

fn same_kind(a: &str, b: &str) -> bool {
    fn norm(t: &str) -> &str {
        match t {
            "ctrTitle" => "title",
            "obj" => "body",
            t => t,
        }
    }
    norm(a) == norm(b)
}

/// Type d'espace réservé du masque correspondant.
fn master_kind(ty: &str) -> &str {
    match ty {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => ty,
        _ => "body",
    }
}

fn all_shapes<'a>(tree: &'a Elem, out: &mut Vec<&'a Elem>) {
    for c in &tree.children {
        match c.name.as_str() {
            "grpSp" => all_shapes(c, out),
            "AlternateContent" => {
                if let Some(alt) = c.child("Fallback").or_else(|| c.child("Choice")) {
                    all_shapes(alt, out)
                }
            }
            _ => out.push(c),
        }
    }
}

fn find_ph<'a>(tree: &'a Elem, key: &PhKey, master: bool) -> Option<&'a Elem> {
    let mut shapes = Vec::new();
    all_shapes(tree, &mut shapes);
    let keyed: Vec<(&Elem, PhKey)> = shapes.into_iter().filter_map(|s| Some((s, ph_key(s)?))).collect();
    if master {
        let want = master_kind(&key.ty);
        return keyed.iter().find(|(_, k)| master_kind(&k.ty) == want).map(|(s, _)| *s);
    }
    if let Some(idx) = &key.idx {
        if let Some((s, _)) = keyed.iter().find(|(_, k)| k.idx.as_deref() == Some(idx.as_str())) {
            return Some(s);
        }
    }
    keyed.iter().find(|(_, k)| same_kind(&k.ty, &key.ty)).map(|(s, _)| *s)
}

/// Styles de texte de base, par catégorie d'espace réservé.
struct TextStyles {
    title: ListStyle,
    body: ListStyle,
    other: ListStyle,
}

struct Ctx<'a> {
    pres: &'a Presentation,
    theme: Theme,
    map: ClrMap,
    slide: Part,
    layout: Option<Part>,
    master: Option<Part>,
    styles: TextStyles,
    slide_num: usize,
    table_styles: Option<Arc<Elem>>,
}

impl Ctx<'_> {
    fn cctx(&self) -> ColorCtx<'_> {
        ColorCtx { scheme: &self.theme.colors, map: &self.map, ph_clr: None }
    }
}

/// Chaîne d'héritage d'une forme de la diapositive (espaces réservés).
fn chain<'e>(ctx: &'e Ctx, el: &'e Elem, part: &Part, is_template: bool) -> Chain<'e> {
    let mut items = vec![(el, part.rels.clone())];
    let mut kind = None;
    if let (false, Some(key)) = (is_template, ph_key(el)) {
        kind = Some(key.ty.clone());
        // Seules les formes de la diapositive héritent (les gabarits ne dessinent pas leurs espaces réservés).
        if Arc::ptr_eq(&part.xml, &ctx.slide.xml) {
            let mut master_key = key.clone();
            if let Some(l) = &ctx.layout {
                if let Some(found) = l.tree().and_then(|t| find_ph(t, &key, false)) {
                    if let Some(k) = ph_key(found) {
                        if key.ty == "obj" && k.ty != "obj" {
                            kind = Some(k.ty.clone());
                        }
                        master_key = k;
                    }
                    items.push((found, l.rels.clone()));
                }
            }
            if let Some(m) = &ctx.master {
                if let Some(found) = m.tree().and_then(|t| find_ph(t, &master_key, true)) {
                    items.push((found, m.rels.clone()));
                }
            }
        }
    }
    Chain { items, kind }
}

struct Renderer<'a, 'f> {
    ctx: &'a Ctx<'a>,
    pix: Pixmap,
    fonts: &'f mut Fonts,
    /// Pixels par EMU.
    scale: f32,
    group_fill: Vec<Fill>,
    bg_fill: Option<(Fill, Arc<Vec<Rel>>)>,
}

pub fn render_slide(pres: &Presentation, index: usize, w: u32, h: u32, fonts: &mut Fonts) -> Result<RgbaImage> {
    let slide = Part::load(pres, &pres.slides[index].part)?;
    let layout = slide.rel("slideLayout").and_then(|r| Part::load(pres, &r.target).ok());
    let master = layout.as_ref().and_then(|l| l.rel("slideMaster")).and_then(|r| Part::load(pres, &r.target).ok());
    let theme = master
        .as_ref()
        .and_then(|m| m.rel("theme"))
        .and_then(|r| pres.pkg.xml(&r.target).ok())
        .map(|t| Theme::from_elem(&t))
        .unwrap_or_default();
    let base_map = master.as_ref().and_then(|m| m.xml.child("clrMap")).map(ClrMap::from_elem).unwrap_or_default();
    let map = base_map.overridden(layout.as_ref().and_then(|l| l.xml.child("clrMapOvr"))).overridden(slide.xml.child("clrMapOvr"));

    let pres_root = pres.pkg.xml(&pres.pres_part)?;
    let pres_rels = pres.pkg.rels(&pres.pres_part);
    let table_styles = pres_rels.iter().find(|r| r.kind == "tableStyles").and_then(|r| pres.pkg.xml(&r.target).ok());
    let mut ctx = Ctx {
        pres,
        theme,
        map,
        slide,
        layout,
        master,
        styles: TextStyles { title: ListStyle::default(), body: ListStyle::default(), other: ListStyle::default() },
        slide_num: index + 1,
        table_styles,
    };
    // Styles de texte : défauts → texte par défaut de la présentation → styles du masque.
    let mut base = ListStyle::default();
    {
        let cc = ctx.cctx();
        let tx1 = cc.scheme_color("tx1").unwrap_or(Rgba::BLACK);
        for l in base.0.iter_mut() {
            l.run.color = Some(tx1);
            l.run.size = Some(18.0);
        }
        base.apply(pres_root.child("defaultTextStyle"), &cc);
        let tx_styles = ctx.master.as_ref().and_then(|m| m.xml.child("txStyles"));
        let mk = |name: &str| {
            let mut s = base.clone();
            s.apply(tx_styles.and_then(|t| t.child(name)), &cc);
            s
        };
        let styles = TextStyles { title: mk("titleStyle"), body: mk("bodyStyle"), other: mk("otherStyle") };
        ctx.styles = styles;
    }

    let mut pix = Pixmap::new(w, h).ok_or(crate::Error::InvalidSize { width: w, height: h })?;
    pix.fill(tiny_skia::Color::WHITE);
    let scale = w as f32 / pres.slide_size_emu().0 as f32;
    let mut r = Renderer { ctx: &ctx, pix, fonts, scale, group_fill: Vec::new(), bg_fill: None };
    r.draw_background();
    let root = Transform::from_scale(scale, scale);
    let show_master_on_slide = ctx.slide.xml.attr("showMasterSp").map(|v| v != "0" && v != "false").unwrap_or(true);
    let show_master_on_layout = ctx.layout.as_ref().and_then(|l| l.xml.attr("showMasterSp")).map(|v| v != "0" && v != "false").unwrap_or(true);
    if show_master_on_slide {
        if show_master_on_layout {
            if let Some(m) = &ctx.master {
                if let Some(t) = m.tree() {
                    r.draw_tree(t, m, root, true, 0);
                }
            }
        }
        if let Some(l) = &ctx.layout {
            if let Some(t) = l.tree() {
                r.draw_tree(t, l, root, true, 0);
            }
        }
    }
    if let Some(t) = ctx.slide.tree() {
        r.draw_tree(t, &ctx.slide, root, false, 0);
    }
    Ok(to_image(r.pix))
}

fn to_image(pix: Pixmap) -> RgbaImage {
    let (w, h) = (pix.width(), pix.height());
    let mut data = pix.take();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(w, h, data).unwrap_or_else(|| RgbaImage::new(w, h))
}

/// Position d'une forme : (x, y, l, h, rotation en degrés, miroir horizontal, miroir vertical).
#[derive(Clone, Copy, Debug)]
struct Xfrm {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rot: f32,
    flip_h: bool,
    flip_v: bool,
}

fn parse_xfrm(x: &Elem) -> Option<Xfrm> {
    let off = x.child("off")?;
    let ext = x.child("ext")?;
    Some(Xfrm {
        x: off.attr_f64("x").unwrap_or(0.0) as f32,
        y: off.attr_f64("y").unwrap_or(0.0) as f32,
        w: ext.attr_f64("cx").unwrap_or(0.0).max(0.0) as f32,
        h: ext.attr_f64("cy").unwrap_or(0.0).max(0.0) as f32,
        rot: x.attr_f64("rot").unwrap_or(0.0) as f32 / 60_000.0,
        flip_h: x.attr_bool("flipH").unwrap_or(false),
        flip_v: x.attr_bool("flipV").unwrap_or(false),
    })
}

impl Xfrm {
    /// Repère local de la forme (0..w, 0..h) → repère parent.
    fn transform(&self, flips: bool) -> Transform {
        let (cx, cy) = (self.x + self.w / 2.0, self.y + self.h / 2.0);
        let mut t = Transform::from_translate(cx, cy).pre_concat(Transform::from_rotate(self.rot));
        if flips && (self.flip_h || self.flip_v) {
            t = t.pre_scale(if self.flip_h { -1.0 } else { 1.0 }, if self.flip_v { -1.0 } else { 1.0 });
        }
        t.pre_translate(-self.w / 2.0, -self.h / 2.0)
    }

    /// Repère du texte : la rotation s'applique, pas le miroir (un miroir vertical retourne le texte).
    fn text_transform(&self) -> Transform {
        let (cx, cy) = (self.x + self.w / 2.0, self.y + self.h / 2.0);
        let rot = self.rot + if self.flip_v { 180.0 } else { 0.0 };
        Transform::from_translate(cx, cy).pre_concat(Transform::from_rotate(rot)).pre_translate(-self.w / 2.0, -self.h / 2.0)
    }
}

/// Une forme et ses ancêtres d'héritage (espace réservé de la disposition, puis du masque),
/// chacun avec les relations de sa partie.
struct Chain<'a> {
    items: Vec<(&'a Elem, Arc<Vec<Rel>>)>,
    kind: Option<String>,
}

impl Chain<'_> {
    fn own(&self) -> &Elem {
        self.items[0].0
    }

    fn xfrm(&self) -> Option<Xfrm> {
        self.items.iter().find_map(|(e, _)| e.child("spPr").or_else(|| e.child("xfrm")).and_then(|s| if s.name == "xfrm" { parse_xfrm(s) } else { s.child("xfrm").and_then(parse_xfrm) }))
    }

    fn sp_pr_with(&self, f: impl Fn(&Elem) -> bool) -> Option<(&Elem, &Arc<Vec<Rel>>)> {
        self.items.iter().find_map(|(e, r)| e.child("spPr").filter(|s| f(s)).map(|s| (s, r)))
    }
}

impl<'a> Renderer<'a, '_> {
    fn cctx(&self) -> ColorCtx<'a> {
        self.ctx.cctx()
    }

    fn draw_background(&mut self) {
        let ctx = self.ctx;
        let parts = [Some(&ctx.slide), ctx.layout.as_ref(), ctx.master.as_ref()];
        let Some((bg, part)) = parts.into_iter().flatten().find_map(|p| p.xml.path(&["cSld", "bg"]).map(|b| (b, p))) else { return };
        let cc = self.cctx();
        let fill = if let Some(pr) = bg.child("bgPr") {
            parse_fill(pr, &cc, &part.rels)
        } else if let Some(r) = bg.child("bgRef") {
            fill_from_ref(&ctx.theme, r, &cc, &part.rels)
        } else {
            None
        };
        let Some(fill) = fill else { return };
        let (w, h) = ctx.pres.slide_size_emu();
        let (w, h) = (w as f32, h as f32);
        if let Some(path) = Rect::from_xywh(0.0, 0.0, w, h).map(PathBuilder::from_rect) {
            let tf = Transform::from_scale(self.scale, self.scale);
            self.fill_path(&path, &fill, &part.rels, w, h, tf);
        }
        self.bg_fill = Some((fill, part.rels.clone()));
    }

    fn draw_tree(&mut self, tree: &Elem, part: &Part, tf: Transform, skip_ph: bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for el in &tree.children {
            self.draw_element(el, part, tf, skip_ph, depth);
        }
    }

    fn draw_element(&mut self, el: &Elem, part: &Part, tf: Transform, skip_ph: bool, depth: usize) {
        if nv_props(el).and_then(|n| n.child("cNvPr")).and_then(|c| c.attr_bool("hidden")).unwrap_or(false) {
            return;
        }
        if skip_ph && ph_key(el).is_some() {
            return;
        }
        match el.name.as_str() {
            "sp" | "cxnSp" => {
                let ch = chain(self.ctx, el, part, skip_ph);
                self.draw_sp(&ch, tf);
            }
            "pic" => {
                let ch = chain(self.ctx, el, part, skip_ph);
                self.draw_pic(&ch, tf);
            }
            "grpSp" => self.draw_group(el, part, tf, skip_ph, depth),
            "graphicFrame" => self.draw_frame(el, part, tf, depth),
            "AlternateContent" => {
                // Graphiques récents (chartex) : la variante de repli n'est qu'un message d'avertissement.
                let choice = el.child("Choice");
                let chartex = choice.and_then(|c| c.attr("Requires")).map(|r| r.starts_with("cx")).unwrap_or(false);
                if chartex {
                    return;
                }
                if let Some(alt) = el.child("Fallback").or(choice) {
                    self.draw_tree(alt, part, tf, skip_ph, depth + 1);
                }
            }
            _ => {}
        }
    }

    fn style_el<'e>(&self, el: &'e Elem) -> Option<&'e Elem> {
        el.child("style")
    }

    fn shape_fill(&self, chain: &Chain) -> (Fill, Arc<Vec<Rel>>) {
        let cc = self.cctx();
        if chain.own().attr_bool("useBgFill").unwrap_or(false) {
            if let Some((f, r)) = &self.bg_fill {
                return (f.clone(), r.clone());
            }
        }
        for (e, rels) in &chain.items {
            if let Some(f) = e.child("spPr").and_then(|s| parse_fill(s, &cc, rels)) {
                return (f, rels.clone());
            }
        }
        let rels = chain.items[0].1.clone();
        if let Some(r) = self.style_el(chain.own()).and_then(|s| s.child("fillRef")) {
            if let Some(f) = fill_from_ref(&self.ctx.theme, r, &cc, &rels) {
                return (f, rels);
            }
        }
        (Fill::None, rels)
    }

    fn shape_line(&self, chain: &Chain) -> Option<Line> {
        let cc = self.cctx();
        let rels = &chain.items[0].1;
        let mut line = self.style_el(chain.own()).and_then(|s| s.child("lnRef")).and_then(|r| line_from_ref(&self.ctx.theme, r, &cc, rels)).unwrap_or_default();
        for (e, rels) in chain.items.iter().rev() {
            if let Some(ln) = e.path(&["spPr", "ln"]) {
                line = parse_line(ln, &cc, rels, line);
            }
        }
        (!matches!(line.fill, Fill::None)).then_some(line)
    }

    fn draw_sp(&mut self, chain: &Chain, parent: Transform) {
        // En diaporama, un espace réservé vide n'est pas affiché du tout (ni fond, ni contour).
        if chain.kind.is_some() && !has_text(chain.own()) {
            return;
        }
        let Some(x) = chain.xfrm() else { return };
        let tf = parent.pre_concat(x.transform(true));
        let geom_pr = chain.sp_pr_with(|s| s.child("prstGeom").is_some() || s.child("custGeom").is_some()).map(|(s, _)| s);
        let prst = geom::preset_name(geom_pr).to_string();
        let paths = geom::shape_paths(geom_pr, x.w, x.h);
        let (fill, fill_rels) = self.shape_fill(chain);
        if !geom::is_line_preset(&prst) && !matches!(fill, Fill::None) {
            self.draw_shadow(chain, &paths, tf);
        }
        if !geom::is_line_preset(&prst) {
            for p in paths.iter().filter(|p| p.fill) {
                self.fill_path(&p.path, &fill, &fill_rels, x.w, x.h, tf);
            }
        }
        if let Some(line) = self.shape_line(chain) {
            for p in paths.iter().filter(|p| p.stroke) {
                self.stroke_path(&p.path, &line, &fill_rels, tf);
            }
            if line.head.is_some() || line.tail.is_some() {
                self.draw_arrows(&prst, &line, x.w, x.h, tf);
            }
        }
        self.draw_text_body(chain, &x, parent);
    }

    fn draw_shadow(&mut self, chain: &Chain, paths: &[geom::GPath], tf: Transform) {
        let cc = self.cctx();
        let sp_pr = chain.sp_pr_with(|s| s.child("effectLst").is_some() || s.child("effectDag").is_some()).map(|(s, _)| s);
        if let Some(sh) = crate::effects::shadow_of(sp_pr, self.style_el(chain.own()), &self.ctx.theme, &cc) {
            let ps: Vec<&Path> = paths.iter().filter(|p| p.fill).map(|p| &p.path).collect();
            crate::effects::draw_shadow(&mut self.pix, &ps, &sh, tf, self.scale);
        }
    }

    fn draw_text_body(&mut self, chain: &Chain, x: &Xfrm, parent: Transform) {
        let own = chain.own();
        let Some(body) = own.child("txBody") else { return };
        if !has_text(own) {
            return;
        }
        let cc = self.cctx();
        // Styles de liste : catégorie → masque → disposition → style de la forme → forme.
        let mut levels = match chain.kind.as_deref() {
            Some("title" | "ctrTitle") => self.ctx.styles.title.clone(),
            Some("dt" | "ftr" | "sldNum" | "hdr") => self.ctx.styles.other.clone(),
            Some(_) => self.ctx.styles.body.clone(),
            None => self.ctx.styles.other.clone(),
        };
        let mut bp = BodyProps::default();
        for (e, _) in chain.items.iter().rev() {
            if let Some(b) = e.child("txBody") {
                if let Some(pr) = b.child("bodyPr") {
                    bp.merge(&BodyProps::parse(pr));
                }
                if !std::ptr::eq(*e, own) {
                    levels.apply(b.child("lstStyle"), &cc);
                }
            }
        }
        if let Some(fr) = self.style_el(own).and_then(|s| s.child("fontRef")) {
            if let Some(c) = child_color(fr, &cc) {
                levels.set_color(c);
            }
            let face = match fr.attr("idx") {
                Some("major") => Some("+mj-lt"),
                Some("minor") => Some("+mn-lt"),
                _ => None,
            };
            if let Some(f) = face {
                for l in levels.0.iter_mut() {
                    l.run.latin.get_or_insert_with(|| f.to_string());
                }
            }
        }
        levels.apply(body.child("lstStyle"), &cc);

        // Zone de texte : celle de la forme prédéfinie, ou `dsp:txXfrm` (SmartArt).
        let geom_pr = chain.sp_pr_with(|s| s.child("prstGeom").is_some() || s.child("custGeom").is_some()).map(|(s, _)| s);
        let (mut l, mut t, mut r, mut b) = geom::text_rect(geom_pr, x.w, x.h);
        let mut text_x = *x;
        if let Some(tx) = own.child("txXfrm").and_then(parse_xfrm) {
            text_x = tx;
            (l, t, r, b) = (0.0, 0.0, tx.w, tx.h);
        }
        let ins = |v: Option<f32>, d: f32| v.unwrap_or(d);
        l += ins(bp.ins_l, 91_440.0);
        r -= ins(bp.ins_r, 91_440.0);
        t += ins(bp.ins_t, 45_720.0);
        b -= ins(bp.ins_b, 45_720.0);
        let (iw, ih) = ((r - l).max(1.0), (b - t).max(1.0));
        let vert = bp.vert.as_deref().unwrap_or("horz");
        let (vertical, vert270) = (matches!(vert, "vert" | "eaVert" | "mongolianVert" | "wordArtVert" | "wordArtVertRtl"), vert == "vert270");
        let (lw, lh) = if vertical || vert270 { (ih, iw) } else { (iw, ih) };
        let paras = text::collect_paragraphs(body, &levels, &cc, self.ctx.slide_num);
        let (font_scale, line_reduction) = bp.autofit.unwrap_or((1.0, 0.0));
        let opts = LayoutOpts { width: lw / EMU_PER_PT, wrap: bp.wrap.unwrap_or(true), font_scale, line_reduction };
        let laid = text::layout(&paras, &opts, self.fonts, &self.ctx.theme);
        let avail = lh / EMU_PER_PT;
        let dy = match bp.anchor.unwrap_or(Anchor::Top) {
            Anchor::Top => 0.0,
            Anchor::Center => (avail - laid.height) / 2.0,
            Anchor::Bottom => avail - laid.height,
        };
        let mut tf = parent.pre_concat(text_x.text_transform());
        if let Some(rot) = bp.rot.filter(|r| *r != 0.0) {
            let (cx, cy) = (l + iw / 2.0, t + ih / 2.0);
            tf = tf.pre_translate(cx, cy).pre_concat(Transform::from_rotate(rot)).pre_translate(-cx, -cy);
        }
        tf = if vertical {
            tf.pre_translate(l + iw, t).pre_concat(Transform::from_rotate(90.0))
        } else if vert270 {
            tf.pre_translate(l, t + ih).pre_concat(Transform::from_rotate(-90.0))
        } else {
            tf.pre_translate(l, t)
        };
        let tf = tf.pre_scale(EMU_PER_PT, EMU_PER_PT).pre_translate(0.0, dy);
        text::draw(&laid, &mut self.pix, self.fonts, tf);
    }

    fn draw_pic(&mut self, chain: &Chain, parent: Transform) {
        let own = chain.own();
        if chain.kind.is_some() && own.child("blipFill").and_then(|b| b.child("blip")).is_none() {
            return;
        }
        let Some(x) = chain.xfrm() else { return };
        let tf = parent.pre_concat(x.transform(true));
        let geom_pr = chain.sp_pr_with(|s| s.child("prstGeom").is_some() || s.child("custGeom").is_some()).map(|(s, _)| s);
        let paths = geom::shape_paths(geom_pr, x.w, x.h);
        let rels = chain.items[0].1.clone();
        if let Some(img) = own.child("blipFill").and_then(|b| image_fill(b, &rels)) {
            self.draw_shadow(chain, &paths, tf);
            let fill = Fill::Image(img);
            for p in paths.iter().filter(|p| p.fill) {
                self.fill_path(&p.path, &fill, &rels, x.w, x.h, tf);
            }
        }
        if let Some(line) = self.shape_line(chain) {
            for p in paths.iter().filter(|p| p.stroke) {
                self.stroke_path(&p.path, &line, &rels, tf);
            }
        }
    }

    fn draw_group(&mut self, el: &Elem, part: &Part, parent: Transform, skip_ph: bool, depth: usize) {
        let pr = el.child("grpSpPr");
        let xf = pr.and_then(|p| p.child("xfrm"));
        let mut tf = parent;
        if let Some(x) = xf.and_then(parse_xfrm) {
            let ch_off = xf.and_then(|x| x.child("chOff"));
            let ch_ext = xf.and_then(|x| x.child("chExt"));
            let cx = ch_off.and_then(|c| c.attr_f64("x")).unwrap_or(x.x as f64) as f32;
            let cy = ch_off.and_then(|c| c.attr_f64("y")).unwrap_or(x.y as f64) as f32;
            let cw = ch_ext.and_then(|c| c.attr_f64("cx")).unwrap_or(x.w as f64) as f32;
            let chh = ch_ext.and_then(|c| c.attr_f64("cy")).unwrap_or(x.h as f64) as f32;
            let sx = if cw > 0.0 { x.w / cw } else { 1.0 };
            let sy = if chh > 0.0 { x.h / chh } else { 1.0 };
            tf = parent.pre_concat(x.transform(true)).pre_scale(sx, sy).pre_translate(-cx, -cy);
        }
        let cc = self.cctx();
        let gf = pr.and_then(|p| parse_fill(p, &cc, &part.rels));
        let pushed = gf.is_some();
        if let Some(f) = gf {
            let f = match f {
                Fill::Group => self.group_fill.last().cloned().unwrap_or(Fill::None),
                f => f,
            };
            self.group_fill.push(f);
        }
        self.draw_tree(el, part, tf, skip_ph, depth + 1);
        if pushed {
            self.group_fill.pop();
        }
    }

    fn draw_frame(&mut self, el: &Elem, part: &Part, parent: Transform, depth: usize) {
        let ch = chain(self.ctx, el, part, false);
        // Le cadre porte son `p:xfrm` directement.
        let Some(x) = el.child("xfrm").and_then(parse_xfrm).or_else(|| ch.xfrm()) else { return };
        let Some(data) = el.path(&["graphic", "graphicData"]) else { return };
        let uri = data.attr("uri").unwrap_or("");
        if uri.ends_with("/table") {
            if let Some(tbl) = data.child("tbl") {
                let tf = parent.pre_concat(x.transform(false));
                self.draw_table(tbl, part, tf, x.w);
            }
        } else if uri.ends_with("/diagram") {
            self.draw_diagram(data, part, parent.pre_concat(Transform::from_translate(x.x, x.y)), depth);
        } else {
            // Objet OLE, graphique… : on dessine l'aperçu s'il y en a un.
            let mut pics = Vec::new();
            data.find_all("pic", &mut pics);
            if let Some(pic) = pics.first() {
                let mut c = chain(self.ctx, pic, part, true);
                if c.xfrm().is_none() {
                    // Aperçu sans position : il occupe le cadre.
                    let tf = parent.pre_concat(x.transform(true));
                    let rels = part.rels.clone();
                    if let Some(img) = pic.child("blipFill").and_then(|b| image_fill(b, &rels)) {
                        if let Some(path) = Rect::from_xywh(0.0, 0.0, x.w.max(1.0), x.h.max(1.0)).map(PathBuilder::from_rect) {
                            self.fill_path(&path, &Fill::Image(img), &rels, x.w, x.h, tf);
                        }
                    }
                } else {
                    c.kind = None;
                    self.draw_pic(&c, parent);
                }
            }
        }
    }

    /// SmartArt : PowerPoint enregistre un dessin précalculé (`ppt/diagrams/drawingN.xml`).
    fn draw_diagram(&mut self, data: &Elem, part: &Part, tf: Transform, depth: usize) {
        let pres = self.ctx.pres;
        let Some(dm) = data.child("relIds").and_then(|r| r.attr("r:dm")) else { return };
        let Some(dm_part) = part.rels.iter().find(|r| r.id == dm).map(|r| r.target.clone()) else { return };
        let mut drawing = None;
        if let Ok(d) = pres.pkg.xml(&dm_part) {
            if let Some(ext) = d.find("dataModelExt") {
                if let Some(id) = ext.attr("relId") {
                    drawing = part.rels.iter().find(|r| r.id == id).map(|r| r.target.clone());
                }
            }
        }
        let drawing = drawing.or_else(|| {
            let guess = dm_part.replace("/data", "/drawing");
            pres.pkg.exists(&guess).then_some(guess)
        });
        let Some(drawing) = drawing else { return };
        let Ok(p) = Part::load(pres, &drawing) else { return };
        if let Some(tree) = p.xml.child("spTree") {
            let tree = tree.clone();
            self.draw_tree(&tree, &p, tf, true, depth + 1);
        }
    }

    // ------------------------------------------------------------------ tableaux

    fn draw_table(&mut self, tbl: &Elem, part: &Part, tf: Transform, _frame_w: f32) {
        let cc = self.cctx();
        let cols: Vec<f32> = tbl.child("tblGrid").map(|g| g.children("gridCol").map(|c| c.attr_f64("w").unwrap_or(0.0) as f32).collect()).unwrap_or_default();
        let rows: Vec<&Elem> = tbl.children("tr").collect();
        if cols.is_empty() || rows.is_empty() {
            return;
        }
        let tbl_pr = tbl.child("tblPr");
        let flag = |n: &str| tbl_pr.and_then(|p| p.attr_bool(n)).unwrap_or(false);
        let (first_row, last_row, first_col, last_col, band_row, band_col) = (flag("firstRow"), flag("lastRow"), flag("firstCol"), flag("lastCol"), flag("bandRow"), flag("bandCol"));
        let style_id = tbl_pr.and_then(|p| p.child("tableStyleId")).map(|s| s.text.trim().to_string());
        let builtin;
        let style: Option<&Elem> = match (&style_id, &self.ctx.table_styles) {
            (Some(id), Some(ts)) if ts.children("tblStyle").any(|s| s.attr("styleId") == Some(id)) => ts.children("tblStyle").find(|s| s.attr("styleId") == Some(id)),
            (Some(id), _) if id.eq_ignore_ascii_case(DEFAULT_TABLE_STYLE_ID) => {
                builtin = crate::xml::parse("tableStyle", DEFAULT_TABLE_STYLE.as_bytes()).ok();
                builtin.as_ref()
            }
            (None, Some(ts)) => ts.attr("def").and_then(|d| ts.children("tblStyle").find(|s| s.attr("styleId") == Some(d))),
            _ => None,
        };
        let (nr, nc) = (rows.len(), cols.len());
        // Parties du style qui s'appliquent à une cellule, de la plus faible à la plus forte.
        let parts_for = |r: usize, c: usize| -> Vec<(&'static str, Region)> {
            let mut v = vec![("wholeTbl", Region::Whole)];
            let body_r = if first_row { r as i64 - 1 } else { r as i64 };
            let body_c = if first_col { c as i64 - 1 } else { c as i64 };
            if band_row && body_r >= 0 {
                v.push((if body_r % 2 == 0 { "band1H" } else { "band2H" }, Region::Row));
            }
            if band_col && body_c >= 0 {
                v.push((if body_c % 2 == 0 { "band1V" } else { "band2V" }, Region::Col));
            }
            if last_col && c == nc - 1 {
                v.push(("lastCol", Region::Col));
            }
            if first_col && c == 0 {
                v.push(("firstCol", Region::Col));
            }
            if last_row && r == nr - 1 {
                v.push(("lastRow", Region::Row));
            }
            if first_row && r == 0 {
                v.push(("firstRow", Region::Row));
            }
            v
        };

        struct Cell<'c> {
            r: usize,
            c: usize,
            rs: usize,
            cs: usize,
            tc: &'c Elem,
            fill: Option<Fill>,
            borders: [Option<Line>; 4],
            laid: Option<(text::Laid, f32, f32, f32, f32, Anchor)>,
        }
        let mut cells: Vec<Cell> = Vec::new();
        let mut heights: Vec<f32> = rows.iter().map(|r| r.attr_f64("h").unwrap_or(0.0) as f32).collect();
        for (ri, tr) in rows.iter().enumerate() {
            for (ci, tc) in tr.children("tc").enumerate().take(nc) {
                if tc.attr_bool("hMerge").unwrap_or(false) || tc.attr_bool("vMerge").unwrap_or(false) {
                    continue;
                }
                let cs = tc.attr_i64("gridSpan").unwrap_or(1).clamp(1, (nc - ci) as i64) as usize;
                let rs = tc.attr_i64("rowSpan").unwrap_or(1).clamp(1, (nr - ri) as i64) as usize;
                let parts = parts_for(ri, ci);
                let mut fill = None;
                let mut borders: [Option<Line>; 4] = [None, None, None, None];
                let mut levels = self.ctx.styles.other.clone();
                if let Some(st) = style {
                    for (name, region) in &parts {
                        let Some(sp) = st.child(name) else { continue };
                        if let Some(tcs) = sp.child("tcStyle") {
                            if let Some(f) = tcs.child("fill").and_then(|f| parse_fill(f, &cc, &part.rels)) {
                                fill = Some(f);
                            } else if let Some(fr) = tcs.child("fillRef") {
                                fill = fill_from_ref(&self.ctx.theme, fr, &cc, &part.rels);
                            }
                            if let Some(bdr) = tcs.child("tcBdr") {
                                for (k, edge) in ["left", "right", "top", "bottom"].iter().enumerate() {
                                    let key = region.edge_key(edge, ri, ci, ri + rs - 1 == nr - 1, ci + cs - 1 == nc - 1);
                                    if let Some(e) = bdr.child(key) {
                                        borders[k] = border_line(e, &cc, &self.ctx.theme, &part.rels);
                                    }
                                }
                            }
                        }
                        if let Some(tx) = sp.child("tcTxStyle") {
                            let mut rp = RunProps { bold: tx.attr("b").map(|b| b == "on"), italic: tx.attr("i").map(|i| i == "on"), ..Default::default() };
                            rp.color = child_color(tx, &cc).or_else(|| tx.child("fontRef").and_then(|f| child_color(f, &cc)));
                            for l in levels.0.iter_mut() {
                                l.run.merge(&rp);
                            }
                        }
                    }
                }
                if let Some(pr) = tc.child("tcPr") {
                    if let Some(f) = parse_fill(pr, &cc, &part.rels) {
                        fill = Some(f);
                    }
                    for (k, n) in ["lnL", "lnR", "lnT", "lnB"].iter().enumerate() {
                        if let Some(ln) = pr.child(n) {
                            let l = parse_line(ln, &cc, &part.rels, Line { width: 12_700.0, ..Line::default() });
                            borders[k] = (!matches!(l.fill, Fill::None)).then_some(l);
                        }
                    }
                }
                // Texte : mise en page tout de suite, pour agrandir la ligne si besoin.
                let pr = tc.child("tcPr");
                let m = |n: &str, d: f32| pr.and_then(|p| p.attr_f64(n)).map(|v| v as f32).unwrap_or(d);
                let (ml, mr, mt, mb) = (m("marL", 91_440.0), m("marR", 91_440.0), m("marT", 45_720.0), m("marB", 45_720.0));
                let anchor = match pr.and_then(|p| p.attr("anchor")) {
                    Some("ctr") => Anchor::Center,
                    Some("b") => Anchor::Bottom,
                    _ => Anchor::Top,
                };
                let w: f32 = cols[ci..ci + cs].iter().sum();
                let laid = tc.child("txBody").map(|body| {
                    levels.apply(body.child("lstStyle"), &cc);
                    let paras = text::collect_paragraphs(body, &levels, &cc, self.ctx.slide_num);
                    let opts = LayoutOpts { width: ((w - ml - mr) / EMU_PER_PT).max(1.0), wrap: true, font_scale: 1.0, line_reduction: 0.0 };
                    (text::layout(&paras, &opts, self.fonts, &self.ctx.theme), ml, mr, mt, mb, anchor)
                });
                if let Some((l, _, _, mt, mb, _)) = &laid {
                    if rs == 1 {
                        let need = l.height * EMU_PER_PT + mt + mb;
                        heights[ri] = heights[ri].max(need);
                    }
                }
                cells.push(Cell { r: ri, c: ci, rs, cs, tc, fill, borders, laid });
            }
        }
        let col_x: Vec<f32> = std::iter::once(0.0).chain(cols.iter().scan(0.0, |s, w| {
            *s += w;
            Some(*s)
        })).collect();
        let row_y: Vec<f32> = std::iter::once(0.0).chain(heights.iter().scan(0.0, |s, h| {
            *s += h;
            Some(*s)
        })).collect();
        let rect_of = |c: &Cell| (col_x[c.c], row_y[c.r], col_x[c.c + c.cs] - col_x[c.c], row_y[c.r + c.rs] - row_y[c.r]);
        for c in &cells {
            let (x, y, w, h) = rect_of(c);
            if let (Some(f), Some(path)) = (&c.fill, Rect::from_xywh(x, y, w.max(1.0), h.max(1.0)).map(PathBuilder::from_rect)) {
                let t = tf.pre_translate(x, y);
                let local = Rect::from_xywh(0.0, 0.0, w.max(1.0), h.max(1.0)).map(PathBuilder::from_rect).unwrap_or(path);
                self.fill_path(&local, f, &part.rels, w, h, t);
            }
        }
        for c in &cells {
            let (x, y, w, h) = rect_of(c);
            let segs = [((x, y), (x, y + h)), ((x + w, y), (x + w, y + h)), ((x, y), (x + w, y)), ((x, y + h), (x + w, y + h))];
            for (k, l) in c.borders.iter().enumerate() {
                if let Some(l) = l {
                    let mut pb = PathBuilder::new();
                    pb.move_to(segs[k].0 .0, segs[k].0 .1);
                    pb.line_to(segs[k].1 .0, segs[k].1 .1);
                    if let Some(p) = pb.finish() {
                        self.stroke_path(&p, l, &part.rels, tf);
                    }
                }
            }
        }
        for c in &cells {
            let (x, y, _w, h) = rect_of(c);
            if let Some((laid, ml, _mr, mt, mb, anchor)) = &c.laid {
                let avail = (h - mt - mb) / EMU_PER_PT;
                let dy = match anchor {
                    Anchor::Top => 0.0,
                    Anchor::Center => (avail - laid.height) / 2.0,
                    Anchor::Bottom => avail - laid.height,
                };
                let t = tf.pre_translate(x + ml, y + mt).pre_scale(EMU_PER_PT, EMU_PER_PT).pre_translate(0.0, dy);
                text::draw(laid, &mut self.pix, self.fonts, t);
            }
            let _ = c.tc;
        }
    }

    // ------------------------------------------------------------------ peinture

    fn fill_path(&mut self, path: &Path, fill: &Fill, rels: &[Rel], w: f32, h: f32, tf: Transform) {
        let fill = match fill {
            Fill::Group => match self.group_fill.last() {
                Some(f) => f.clone(),
                None => return,
            },
            f => f.clone(),
        };
        let _ = rels;
        match &fill {
            Fill::None | Fill::Group => {}
            Fill::Solid(c) => {
                if c.a > 0.0 {
                    let mut p = Paint::default();
                    p.set_color(c.to_skia());
                    p.anti_alias = true;
                    self.pix.fill_path(path, &p, FillRule::EvenOdd, tf, None);
                }
            }
            Fill::Gradient(g) => {
                let stops: Vec<GradientStop> = g.stops.iter().map(|(p, c)| GradientStop::new(p.clamp(0.0, 1.0), c.to_skia())).collect();
                let shader = match g.kind {
                    GradKind::Linear { angle, scaled } => {
                        let a = angle.to_radians();
                        let (c, s) = (a.cos(), a.sin());
                        if scaled {
                            let half = (c.abs() + s.abs()) / 2.0;
                            LinearGradient::new(
                                Point::from_xy(0.5 - c * half, 0.5 - s * half),
                                Point::from_xy(0.5 + c * half, 0.5 + s * half),
                                stops,
                                SpreadMode::Pad,
                                Transform::from_scale(w.max(1.0), h.max(1.0)),
                            )
                        } else {
                            let half = (w * c.abs() + h * s.abs()) / 2.0;
                            let (cx, cy) = (w / 2.0, h / 2.0);
                            LinearGradient::new(Point::from_xy(cx - c * half, cy - s * half), Point::from_xy(cx + c * half, cy + s * half), stops, SpreadMode::Pad, Transform::identity())
                        }
                    }
                    GradKind::Path { to_rect } => {
                        let cx = w * (to_rect[0] + (1.0 - to_rect[2])) / 2.0;
                        let cy = h * (to_rect[1] + (1.0 - to_rect[3])) / 2.0;
                        let r = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].iter().map(|(x, y)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()).fold(1.0f32, f32::max);
                        RadialGradient::new(Point::from_xy(cx, cy), Point::from_xy(cx, cy), r, stops, SpreadMode::Pad, Transform::identity())
                    }
                };
                if let Some(shader) = shader {
                    let p = Paint { shader, anti_alias: true, ..Default::default() };
                    self.pix.fill_path(path, &p, FillRule::EvenOdd, tf, None);
                }
            }
            Fill::Image(img) => self.fill_image(path, img, w, h, tf),
        }
    }

    fn fill_image(&mut self, path: &Path, img: &ImageFill, w: f32, h: f32, tf: Transform) {
        let Some(src) = self.ctx.pres.image(&img.part) else { return };
        let (iw, ih) = (src.width() as f32, src.height() as f32);
        if iw < 1.0 || ih < 1.0 || w <= 0.0 || h <= 0.0 {
            return;
        }
        // Taille approximative de la zone à l'écran, pour réduire les grandes images proprement.
        let sx = (tf.sx * tf.sx + tf.ky * tf.ky).sqrt();
        let sy = (tf.kx * tf.kx + tf.sy * tf.sy).sqrt();
        if let Some((tsx, tsy, tx, ty)) = img.tile {
            // Mosaïque : l'image à 96 ppp, répétée.
            let Some(pm) = to_pixmap(&src, None, img.grayscale) else { return };
            let t = Transform::from_translate(tx, ty).pre_scale(tsx * 9525.0, tsy * 9525.0);
            let shader = Pattern::new(pm.as_ref(), SpreadMode::Repeat, FilterQuality::Bicubic, img.alpha, t);
            let p = Paint { shader, anti_alias: true, ..Default::default() };
            self.pix.fill_path(path, &p, FillRule::EvenOdd, tf, None);
            return;
        }
        let [l, t, r, b] = img.crop;
        let (u0, u1, v0, v1) = (l, 1.0 - r, t, 1.0 - b);
        if u1 - u0 <= 1e-6 || v1 - v0 <= 1e-6 {
            return;
        }
        let (cu0, cu1, cv0, cv1) = (u0.max(0.0), u1.min(1.0), v0.max(0.0), v1.min(1.0));
        if cu1 <= cu0 || cv1 <= cv0 {
            return;
        }
        let lx0 = (cu0 - u0) / (u1 - u0) * w;
        let lx1 = (cu1 - u0) / (u1 - u0) * w;
        let ly0 = (cv0 - v0) / (v1 - v0) * h;
        let ly1 = (cv1 - v0) / (v1 - v0) * h;
        let px0 = (cu0 * iw).floor() as u32;
        let py0 = (cv0 * ih).floor() as u32;
        let px1 = ((cu1 * iw).ceil() as u32).clamp(px0 + 1, src.width());
        let py1 = ((cv1 * ih).ceil() as u32).clamp(py0 + 1, src.height());
        let (cw, chh) = (px1 - px0, py1 - py0);
        let dest_w = ((lx1 - lx0) * sx).ceil().max(1.0) as u32;
        let dest_h = ((ly1 - ly0) * sy).ceil().max(1.0) as u32;
        let resize = (cw > dest_w * 3 / 2 || chh > dest_h * 3 / 2).then_some((dest_w.min(cw), dest_h.min(chh)));
        let Some(pm) = to_pixmap(&src, Some((px0, py0, cw, chh, resize)), img.grayscale) else { return };
        let (pw, ph) = (pm.width() as f32, pm.height() as f32);
        let pt = Transform::from_translate(lx0, ly0).pre_scale((lx1 - lx0) / pw, (ly1 - ly0) / ph);
        let shader = Pattern::new(pm.as_ref(), SpreadMode::Pad, FilterQuality::Bicubic, img.alpha, pt);
        let p = Paint { shader, anti_alias: true, ..Default::default() };
        let padded = u0 < 0.0 || v0 < 0.0 || u1 > 1.0 || v1 > 1.0;
        if padded {
            if let Some(rp) = Rect::from_ltrb(lx0, ly0, lx1, ly1).map(PathBuilder::from_rect) {
                self.pix.fill_path(&rp, &p, FillRule::EvenOdd, tf, None);
            }
        } else {
            self.pix.fill_path(path, &p, FillRule::EvenOdd, tf, None);
        }
    }

    fn stroke_path(&mut self, path: &Path, line: &Line, rels: &[Rel], tf: Transform) {
        let color = match &line.fill {
            Fill::Solid(c) => *c,
            Fill::Gradient(g) => g.stops.first().map(|s| s.1).unwrap_or(Rgba::BLACK),
            Fill::Group => match self.group_fill.last().and_then(|f| f.main_color()) {
                Some(c) => c,
                None => return,
            },
            _ => return,
        };
        let _ = rels;
        if color.a <= 0.0 {
            return;
        }
        let px = (tf.sx * tf.sx + tf.ky * tf.ky).sqrt().max(1e-9);
        // Trait d'au moins un pixel, comme à l'écran dans PowerPoint.
        let width = line.width.max(1.0 / px);
        let mut stroke = Stroke { width, line_cap: if line.round_cap { LineCap::Round } else { LineCap::Butt }, line_join: LineJoin::Round, ..Default::default() };
        if let Some(d) = &line.dash {
            let mut arr: Vec<f32> = d.iter().map(|v| v * width).collect();
            if arr.len() % 2 == 1 {
                arr.push(arr[arr.len() - 1]);
            }
            stroke.dash = StrokeDash::new(arr, 0.0);
        }
        let mut p = Paint::default();
        p.set_color(color.to_skia());
        p.anti_alias = true;
        self.pix.stroke_path(path, &p, &stroke, tf, None);
    }

    fn draw_arrows(&mut self, prst: &str, line: &Line, w: f32, h: f32, tf: Transform) {
        let color = match line.fill.main_color() {
            Some(c) => c,
            None => return,
        };
        let x = w / 2.0;
        // (pointe, point précédent) pour le début et la fin du trait.
        let (start, end) = match prst {
            "bentConnector2" => (((0.0, 0.0), (w, 0.0)), ((w, h), (w, 0.0))),
            "bentConnector3" | "curvedConnector3" => (((0.0, 0.0), (x, 0.0)), ((w, h), (x, h))),
            _ => (((0.0, 0.0), (w, h)), ((w, h), (0.0, 0.0))),
        };
        let px = (tf.sx * tf.sx + tf.ky * tf.ky).sqrt().max(1e-9);
        let lw = line.width.max(1.0 / px);
        for (arrow, (tip, from)) in [(line.head, start), (line.tail, end)] {
            let Some((kind, wm, lm)) = arrow else { continue };
            let (dx, dy) = (from.0 - tip.0, from.1 - tip.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 0.0 {
                continue;
            }
            let (ux, uy) = (dx / len, dy / len);
            let (nx, ny) = (-uy, ux);
            let (aw, al) = (lw * wm.max(2.0) / 2.0 + lw / 2.0, lw * lm.max(2.0));
            let base = (tip.0 + ux * al, tip.1 + uy * al);
            let mut pb = PathBuilder::new();
            match kind {
                Arrow::Oval => {
                    if let Some(r) = Rect::from_xywh(tip.0 - aw, tip.1 - aw, 2.0 * aw, 2.0 * aw) {
                        pb.push_oval(r);
                    }
                }
                Arrow::Diamond => {
                    pb.move_to(tip.0 - ux * al / 2.0, tip.1 - uy * al / 2.0);
                    pb.line_to(tip.0 + nx * aw, tip.1 + ny * aw);
                    pb.line_to(tip.0 + ux * al / 2.0, tip.1 + uy * al / 2.0);
                    pb.line_to(tip.0 - nx * aw, tip.1 - ny * aw);
                    pb.close();
                }
                Arrow::Stealth => {
                    pb.move_to(tip.0, tip.1);
                    pb.line_to(base.0 + nx * aw, base.1 + ny * aw);
                    pb.line_to(tip.0 + ux * al * 0.6, tip.1 + uy * al * 0.6);
                    pb.line_to(base.0 - nx * aw, base.1 - ny * aw);
                    pb.close();
                }
                Arrow::Triangle | Arrow::Open => {
                    pb.move_to(tip.0, tip.1);
                    pb.line_to(base.0 + nx * aw, base.1 + ny * aw);
                    pb.line_to(base.0 - nx * aw, base.1 - ny * aw);
                    pb.close();
                }
            }
            if let Some(p) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(color.to_skia());
                paint.anti_alias = true;
                self.pix.fill_path(&p, &paint, FillRule::Winding, tf, None);
            }
        }
    }
}

/// La forme a-t-elle du texte (au moins une série ou un champ) ?
fn has_text(el: &Elem) -> bool {
    el.child("txBody").map(|b| b.children("p").any(|p| p.children.iter().any(|c| matches!(c.name.as_str(), "r" | "fld") && c.child("t").map(|t| !t.text.is_empty()).unwrap_or(false)))).unwrap_or(false)
}

#[derive(Clone, Copy)]
enum Region {
    Whole,
    Row,
    Col,
}

impl Region {
    /// Bordure du style de tableau qui s'applique à un bord de cellule.
    fn edge_key(self, edge: &str, r: usize, c: usize, last_r: bool, last_c: bool) -> &'static str {
        let inner_h = |outer: &'static str, is_outer: bool| if is_outer { outer } else { "insideH" };
        let inner_v = |outer: &'static str, is_outer: bool| if is_outer { outer } else { "insideV" };
        match (self, edge) {
            (Region::Whole, "top") | (Region::Col, "top") => inner_h("top", r == 0),
            (Region::Whole, "bottom") | (Region::Col, "bottom") => inner_h("bottom", last_r),
            (Region::Whole, "left") | (Region::Row, "left") => inner_v("left", c == 0),
            (Region::Whole, "right") | (Region::Row, "right") => inner_v("right", last_c),
            (Region::Row, "top") => "top",
            (Region::Row, "bottom") => "bottom",
            (Region::Col, "left") => "left",
            (_, _) => "right",
        }
    }
}

fn border_line(e: &Elem, cc: &ColorCtx, theme: &Theme, rels: &[Rel]) -> Option<Line> {
    let l = if let Some(ln) = e.child("ln") {
        parse_line(ln, cc, rels, Line { width: 12_700.0, ..Line::default() })
    } else {
        line_from_ref(theme, e.child("lnRef")?, cc, rels)?
    };
    (!matches!(l.fill, Fill::None)).then_some(l)
}

/// Image (éventuellement recadrée, réduite, en niveaux de gris) en pixmap prémultipliée.
/// Zone d'une image : x, y, largeur, hauteur, et taille de réduction éventuelle.
type Crop = (u32, u32, u32, u32, Option<(u32, u32)>);

fn to_pixmap(src: &RgbaImage, region: Option<Crop>, gray: bool) -> Option<Pixmap> {
    let mut img = match region {
        Some((x, y, w, h, resize)) => {
            let view = image::imageops::crop_imm(src, x, y, w, h);
            match resize {
                Some((nw, nh)) => image::imageops::resize(&*view, nw.max(1), nh.max(1), image::imageops::FilterType::Triangle),
                None => view.to_image(),
            }
        }
        None => src.clone(),
    };
    if gray {
        for p in img.pixels_mut() {
            let y = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8;
            p[0] = y;
            p[1] = y;
            p[2] = y;
        }
    }
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h)?)
}

/// Style de tableau par défaut de PowerPoint (« Style moyen 2 - Accentuation 1 »), souvent
/// absent de `tableStyles.xml`.
const DEFAULT_TABLE_STYLE_ID: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";
const DEFAULT_TABLE_STYLE: &str = r#"<a:tblStyle xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" styleId="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}">
<a:wholeTbl><a:tcTxStyle><a:schemeClr val="dk1"/></a:tcTxStyle><a:tcStyle><a:tcBdr>
<a:left><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:left>
<a:right><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:right>
<a:top><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top>
<a:bottom><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom>
<a:insideH><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideH>
<a:insideV><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideV>
</a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="20000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:wholeTbl>
<a:band1H><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1H>
<a:band2H><a:tcStyle><a:tcBdr/></a:tcStyle></a:band2H>
<a:band1V><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1V>
<a:band2V><a:tcStyle><a:tcBdr/></a:tcStyle></a:band2V>
<a:lastCol><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastCol>
<a:firstCol><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstCol>
<a:lastRow><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:top><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastRow>
<a:firstRow><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:bottom><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstRow>
</a:tblStyle>"#;
