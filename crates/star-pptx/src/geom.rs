//! Géométries : formes prédéfinies courantes (`a:prstGeom`) et géométries libres (`a:custGeom`).
//!
//! Les tracés sont produits dans le repère local de la forme (0..w, 0..h, en EMU).

use crate::xml::Elem;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};
use tiny_skia::{Path, PathBuilder, Rect};

/// Un sous-tracé et ce qu'il faut en faire.
pub struct GPath {
    pub path: Path,
    pub fill: bool,
    pub stroke: bool,
}

/// Valeurs d'ajustement (`a:avLst/a:gd name="adj" fmla="val 16667"`).
fn adjusts(geom: &Elem) -> HashMap<String, f32> {
    let mut m = HashMap::new();
    for list in ["avLst", "gdLst"] {
        if let Some(l) = geom.child(list) {
            for gd in l.children("gd") {
                if let (Some(n), Some(f)) = (gd.attr("name"), gd.attr("fmla")) {
                    if let Some(v) = f.strip_prefix("val ").and_then(|v| v.trim().parse::<f32>().ok()) {
                        m.insert(n.to_string(), v);
                    }
                }
            }
        }
    }
    m
}

/// Tracés de la forme décrite par `spPr` (prstGeom ou custGeom). Rectangle par défaut.
pub fn shape_paths(sp_pr: Option<&Elem>, w: f32, h: f32) -> Vec<GPath> {
    let (w, h) = (w.max(0.0), h.max(0.0));
    if let Some(cg) = sp_pr.and_then(|s| s.child("custGeom")) {
        let paths = custom_paths(cg, w, h);
        if !paths.is_empty() {
            return paths;
        }
    }
    let (name, adj) = match sp_pr.and_then(|s| s.child("prstGeom")) {
        Some(p) => (p.attr("prst").unwrap_or("rect"), adjusts(p)),
        None => ("rect", HashMap::new()),
    };
    preset_paths(name, &adj, w, h)
}

/// Nom de la forme prédéfinie (pour savoir s'il s'agit d'un trait).
pub fn preset_name(sp_pr: Option<&Elem>) -> &str {
    sp_pr.and_then(|s| s.child("prstGeom")).and_then(|p| p.attr("prst")).unwrap_or("rect")
}

pub fn is_line_preset(name: &str) -> bool {
    matches!(name, "line" | "straightConnector1" | "bentConnector2" | "bentConnector3" | "bentConnector4" | "bentConnector5" | "curvedConnector2" | "curvedConnector3" | "curvedConnector4" | "curvedConnector5" | "arc" | "lineInv")
}

/// Zone de texte d'une forme prédéfinie (l, t, r, b), dans le repère de la forme.
pub fn text_rect(sp_pr: Option<&Elem>, w: f32, h: f32) -> (f32, f32, f32, f32) {
    let ss = w.min(h);
    let adj = sp_pr.and_then(|s| s.child("prstGeom")).map(adjusts).unwrap_or_default();
    let a = |n: &str, d: f32| adj.get(n).copied().unwrap_or(d);
    match preset_name(sp_pr) {
        "ellipse" | "flowChartConnector" => {
            let k = (1.0 - std::f32::consts::FRAC_1_SQRT_2) / 2.0;
            (w * k, h * k, w * (1.0 - k), h * (1.0 - k))
        }
        "roundRect" => {
            let r = ss * a("adj", 16667.0) / 100_000.0 * 0.29289;
            (r, r, w - r, h - r)
        }
        "triangle" => (w * a("adj", 50000.0) / 200_000.0, h / 2.0, w - w * (1.0 - a("adj", 50000.0) / 100_000.0) / 2.0, h),
        "diamond" | "flowChartDecision" => (w / 4.0, h / 4.0, w * 0.75, h * 0.75),
        "homePlate" => (0.0, 0.0, w - ss * a("adj", 50000.0) / 200_000.0, h),
        "chevron" => {
            let x = ss * a("adj", 50000.0) / 100_000.0;
            (x, 0.0, w - x, h)
        }
        "hexagon" => {
            let x = ss * a("adj", 25000.0) / 100_000.0 / 2.0;
            (x, 0.0, w - x, h)
        }
        "rightArrow" | "leftArrow" => {
            let t = h * a("adj1", 50000.0) / 100_000.0;
            (0.0, (h - t) / 2.0, w, (h + t) / 2.0)
        }
        _ => (0.0, 0.0, w, h),
    }
}

fn poly(pts: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (x0, y0) = *pts.first()?;
    pb.move_to(x0, y0);
    for &(x, y) in &pts[1..] {
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

fn rect_path(w: f32, h: f32) -> Option<Path> {
    // Une forme de taille nulle (trait horizontal dessiné en rectangle…) reste traçable.
    Rect::from_xywh(0.0, 0.0, w.max(1.0), h.max(1.0)).map(PathBuilder::from_rect)
}

fn round_rect(w: f32, h: f32, r: [f32; 4]) -> Option<Path> {
    // r = [haut gauche, haut droit, bas droit, bas gauche]
    let k = 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(r[0], 0.0);
    pb.line_to(w - r[1], 0.0);
    pb.cubic_to(w - r[1] + r[1] * k, 0.0, w, r[1] - r[1] * k, w, r[1]);
    pb.line_to(w, h - r[2]);
    pb.cubic_to(w, h - r[2] + r[2] * k, w - r[2] + r[2] * k, h, w - r[2], h);
    pb.line_to(r[3], h);
    pb.cubic_to(r[3] - r[3] * k, h, 0.0, h - r[3] + r[3] * k, 0.0, h - r[3]);
    pb.line_to(0.0, r[0]);
    pb.cubic_to(0.0, r[0] - r[0] * k, r[0] - r[0] * k, 0.0, r[0], 0.0);
    pb.close();
    pb.finish()
}

fn ellipse(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    Rect::from_xywh(x, y, w.max(1.0), h.max(1.0)).and_then(PathBuilder::from_oval)
}

/// Arc d'ellipse (centre, rayons, angles en radians) ajouté au tracé en cours, en courbes de Bézier.
pub fn arc_to(pb: &mut PathBuilder, cx: f32, cy: f32, rx: f32, ry: f32, start: f32, sweep: f32) {
    if sweep.abs() < 1e-6 {
        return;
    }
    let n = (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut a = start;
    for _ in 0..n {
        let (s0, c0) = a.sin_cos();
        let (s1, c1) = (a + step).sin_cos();
        pb.cubic_to(
            cx + rx * (c0 - k * s0),
            cy + ry * (s0 + k * c0),
            cx + rx * (c1 + k * s1),
            cy + ry * (s1 - k * c1),
            cx + rx * c1,
            cy + ry * s1,
        );
        a += step;
    }
}

fn star(w: f32, h: f32, points: usize, inner: f32) -> Option<Path> {
    let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let mut pts = Vec::with_capacity(points * 2);
    for i in 0..points * 2 {
        let ang = -FRAC_PI_2 + i as f32 * PI / points as f32;
        let r = if i % 2 == 0 { 1.0 } else { inner };
        pts.push((cx + rx * r * ang.cos(), cy + ry * r * ang.sin()));
    }
    poly(&pts)
}

fn filled(p: Option<Path>) -> Vec<GPath> {
    p.map(|path| vec![GPath { path, fill: true, stroke: true }]).unwrap_or_default()
}

fn stroked(p: Option<Path>) -> Vec<GPath> {
    p.map(|path| vec![GPath { path, fill: false, stroke: true }]).unwrap_or_default()
}

fn open(pts: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for &(x, y) in &pts[1..] {
        pb.line_to(x, y);
    }
    pb.finish()
}

pub fn preset_paths(name: &str, adj: &HashMap<String, f32>, w: f32, h: f32) -> Vec<GPath> {
    let a = |n: &str, d: f32| adj.get(n).copied().unwrap_or(d);
    let ss = w.min(h);
    match name {
        "rect" | "flowChartProcess" | "flowChartPredefinedProcess" | "flowChartInternalStorage" | "textBox" => filled(rect_path(w, h)),
        "roundRect" | "flowChartAlternateProcess" => {
            let r = (ss * a("adj", 16667.0) / 100_000.0).clamp(0.0, ss / 2.0);
            filled(round_rect(w, h, [r; 4]))
        }
        "round1Rect" => {
            let r = (ss * a("adj", 16667.0) / 100_000.0).clamp(0.0, ss / 2.0);
            filled(round_rect(w, h, [0.0, r, 0.0, 0.0]))
        }
        "round2SameRect" => {
            let r1 = (ss * a("adj1", 16667.0) / 100_000.0).clamp(0.0, ss / 2.0);
            let r2 = (ss * a("adj2", 0.0) / 100_000.0).clamp(0.0, ss / 2.0);
            filled(round_rect(w, h, [r1, r1, r2, r2]))
        }
        "round2DiagRect" => {
            let r1 = (ss * a("adj1", 16667.0) / 100_000.0).clamp(0.0, ss / 2.0);
            let r2 = (ss * a("adj2", 0.0) / 100_000.0).clamp(0.0, ss / 2.0);
            filled(round_rect(w, h, [r1, r2, r1, r2]))
        }
        "flowChartTerminator" => filled(round_rect(w, h, [ss / 2.0; 4])),
        "snip1Rect" => {
            let x = ss * a("adj", 16667.0) / 100_000.0;
            filled(poly(&[(0.0, 0.0), (w - x, 0.0), (w, x), (w, h), (0.0, h)]))
        }
        "snip2SameRect" => {
            let x = ss * a("adj1", 16667.0) / 100_000.0;
            filled(poly(&[(x, 0.0), (w - x, 0.0), (w, x), (w, h), (0.0, h), (0.0, x)]))
        }
        "ellipse" | "flowChartConnector" => filled(ellipse(0.0, 0.0, w, h)),
        "line" | "straightConnector1" => stroked(open(&[(0.0, 0.0), (w, h)])),
        "lineInv" => stroked(open(&[(0.0, h), (w, 0.0)])),
        "bentConnector2" => stroked(open(&[(0.0, 0.0), (w, 0.0), (w, h)])),
        "bentConnector3" | "bentConnector4" | "bentConnector5" => {
            let x = w * a("adj1", 50000.0) / 100_000.0;
            stroked(open(&[(0.0, 0.0), (x, 0.0), (x, h), (w, h)]))
        }
        "curvedConnector2" | "curvedConnector3" | "curvedConnector4" | "curvedConnector5" => {
            let x = w * a("adj1", 50000.0) / 100_000.0;
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, 0.0);
            pb.cubic_to(x, 0.0, x, h, w, h);
            stroked(pb.finish())
        }
        "triangle" | "flowChartExtract" => {
            let x = w * a("adj", 50000.0) / 100_000.0;
            filled(poly(&[(x, 0.0), (w, h), (0.0, h)]))
        }
        "flowChartMerge" => filled(poly(&[(0.0, 0.0), (w, 0.0), (w / 2.0, h)])),
        "rtTriangle" => filled(poly(&[(0.0, 0.0), (w, h), (0.0, h)])),
        "diamond" | "flowChartDecision" => filled(poly(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)])),
        "parallelogram" | "flowChartInputOutput" => {
            let x = if name == "parallelogram" { (ss * a("adj", 25000.0) / 100_000.0).min(w) } else { w / 5.0 };
            filled(poly(&[(x, 0.0), (w, 0.0), (w - x, h), (0.0, h)]))
        }
        "trapezoid" | "flowChartManualOperation" => {
            let x = if name == "trapezoid" { (ss * a("adj", 25000.0) / 100_000.0).min(w / 2.0) } else { w / 5.0 };
            if name == "trapezoid" {
                filled(poly(&[(0.0, h), (x, 0.0), (w - x, 0.0), (w, h)]))
            } else {
                filled(poly(&[(0.0, 0.0), (w, 0.0), (w - x, h), (x, h)]))
            }
        }
        "pentagon" => filled(poly(&[(w / 2.0, 0.0), (w, 0.382 * h), (0.809 * w, h), (0.191 * w, h), (0.0, 0.382 * h)])),
        "hexagon" => {
            let x = (ss * a("adj", 25000.0) / 100_000.0).min(w / 2.0);
            filled(poly(&[(x, 0.0), (w - x, 0.0), (w, h / 2.0), (w - x, h), (x, h), (0.0, h / 2.0)]))
        }
        "octagon" => {
            let x = (ss * a("adj", 29289.0) / 100_000.0).min(ss / 2.0);
            filled(poly(&[(x, 0.0), (w - x, 0.0), (w, x), (w, h - x), (w - x, h), (x, h), (0.0, h - x), (0.0, x)]))
        }
        "homePlate" => {
            let x = (w - ss * a("adj", 50000.0) / 100_000.0).max(0.0);
            filled(poly(&[(0.0, 0.0), (x, 0.0), (w, h / 2.0), (x, h), (0.0, h)]))
        }
        "chevron" => {
            let x = (ss * a("adj", 50000.0) / 100_000.0).min(w);
            filled(poly(&[(0.0, 0.0), (w - x, 0.0), (w, h / 2.0), (w - x, h), (0.0, h), (x, h / 2.0)]))
        }
        "rightArrow" | "leftArrow" => {
            let t = h * a("adj1", 50000.0) / 100_000.0;
            let hl = (ss * a("adj2", 50000.0) / 100_000.0).min(w);
            let (y0, y1) = ((h - t) / 2.0, (h + t) / 2.0);
            let pts = [(0.0, y0), (w - hl, y0), (w - hl, 0.0), (w, h / 2.0), (w - hl, h), (w - hl, y1), (0.0, y1)];
            let pts: Vec<(f32, f32)> = if name == "leftArrow" { pts.iter().map(|&(x, y)| (w - x, y)).collect() } else { pts.to_vec() };
            filled(poly(&pts))
        }
        "upArrow" | "downArrow" => {
            let t = w * a("adj1", 50000.0) / 100_000.0;
            let hl = (ss * a("adj2", 50000.0) / 100_000.0).min(h);
            let (x0, x1) = ((w - t) / 2.0, (w + t) / 2.0);
            let pts = [(x0, h), (x0, hl), (0.0, hl), (w / 2.0, 0.0), (w, hl), (x1, hl), (x1, h)];
            let pts: Vec<(f32, f32)> = if name == "downArrow" { pts.iter().map(|&(x, y)| (x, h - y)).collect() } else { pts.to_vec() };
            filled(poly(&pts))
        }
        "leftRightArrow" => {
            let t = h * a("adj1", 50000.0) / 100_000.0;
            let hl = (ss * a("adj2", 50000.0) / 100_000.0).min(w / 2.0);
            let (y0, y1) = ((h - t) / 2.0, (h + t) / 2.0);
            filled(poly(&[(0.0, h / 2.0), (hl, 0.0), (hl, y0), (w - hl, y0), (w - hl, 0.0), (w, h / 2.0), (w - hl, h), (w - hl, y1), (hl, y1), (hl, h)]))
        }
        "plus" | "flowChartSummingJunction" => {
            let x = ss * a("adj", 25000.0) / 100_000.0;
            filled(poly(&[(x, 0.0), (w - x, 0.0), (w - x, x), (w, x), (w, h - x), (w - x, h - x), (w - x, h), (x, h), (x, h - x), (0.0, h - x), (0.0, x), (x, x)]))
        }
        "star4" => filled(star(w, h, 4, a("adj", 12500.0) / 50_000.0)),
        "star5" => filled(star(w, h, 5, a("adj", 19098.0) / 50_000.0)),
        "star6" => filled(star(w, h, 6, a("adj", 28868.0) / 50_000.0)),
        "star7" => filled(star(w, h, 7, a("adj", 34601.0) / 50_000.0)),
        "star8" => filled(star(w, h, 8, a("adj", 38250.0) / 50_000.0)),
        "star10" => filled(star(w, h, 10, a("adj", 42533.0) / 50_000.0)),
        "star12" => filled(star(w, h, 12, a("adj", 37500.0) / 50_000.0)),
        "star16" | "star24" | "star32" => filled(star(w, h, name[4..].parse().unwrap_or(16), a("adj", 37500.0) / 50_000.0)),
        "frame" => {
            let t = ss * a("adj1", 12500.0) / 100_000.0;
            let mut pb = PathBuilder::new();
            for (x0, y0, x1, y1) in [(0.0, 0.0, w, h), (t, t, w - t, h - t)] {
                pb.move_to(x0, y0);
                pb.line_to(x1, y0);
                pb.line_to(x1, y1);
                pb.line_to(x0, y1);
                pb.close();
            }
            filled(pb.finish())
        }
        "donut" => {
            let t = ss * a("adj", 25000.0) / 100_000.0;
            let mut pb = PathBuilder::new();
            if let Some(r) = Rect::from_xywh(0.0, 0.0, w.max(1.0), h.max(1.0)) {
                pb.push_oval(r);
            }
            if let Some(r) = Rect::from_xywh(t, t, (w - 2.0 * t).max(1.0), (h - 2.0 * t).max(1.0)) {
                pb.push_oval(r);
            }
            filled(pb.finish())
        }
        "pie" | "arc" | "chord" | "blockArc" => {
            // Angles en 60 000es de degré, sens horaire depuis l'axe des x.
            let st = a("adj1", if name == "pie" { 0.0 } else { 16_200_000.0 }) / 60_000.0;
            let en = a("adj2", if name == "pie" { 16_200_000.0 } else { 0.0 }) / 60_000.0;
            let mut sweep = en - st;
            if sweep <= 0.0 {
                sweep += 360.0;
            }
            let (cx, cy, rx, ry) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
            let s = st.to_radians();
            let mut pb = PathBuilder::new();
            if name == "pie" {
                pb.move_to(cx, cy);
                pb.line_to(cx + rx * s.cos(), cy + ry * s.sin());
            } else {
                pb.move_to(cx + rx * s.cos(), cy + ry * s.sin());
            }
            arc_to(&mut pb, cx, cy, rx, ry, s, sweep.to_radians());
            if name == "arc" {
                stroked(pb.finish())
            } else {
                pb.close();
                filled(pb.finish())
            }
        }
        "flowChartDocument" => {
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, 0.0);
            pb.line_to(w, 0.0);
            pb.line_to(w, h * 0.8);
            pb.cubic_to(w * 0.75, h * 0.66, w * 0.5, h * 1.08, 0.0, h * 0.92);
            pb.close();
            filled(pb.finish())
        }
        "can" | "flowChartMagneticDisk" => {
            let ry = (ss * a("adj", 25000.0) / 100_000.0) / 2.0;
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, ry);
            arc_to(&mut pb, w / 2.0, ry, w / 2.0, ry, PI, -PI);
            pb.line_to(w, h - ry);
            arc_to(&mut pb, w / 2.0, h - ry, w / 2.0, ry, 0.0, PI);
            pb.close();
            let mut v = filled(pb.finish());
            v.extend(stroked(ellipse(0.0, 0.0, w, 2.0 * ry)));
            v
        }
        // Bulles : on dessine le corps sans la pointe.
        "wedgeRectCallout" => filled(rect_path(w, h)),
        "wedgeRoundRectCallout" => filled(round_rect(w, h, [ss * 0.16667; 4])),
        "wedgeEllipseCallout" | "cloud" | "cloudCallout" => filled(ellipse(0.0, 0.0, w, h)),
        _ => filled(rect_path(w, h)),
    }
}

/// `a:custGeom` : chemins libres (moveTo, lnTo, cubicBezTo, quadBezTo, arcTo, close).
fn custom_paths(cg: &Elem, w: f32, h: f32) -> Vec<GPath> {
    let guides = adjusts(cg);
    let val = |s: Option<&str>| -> f32 {
        let Some(s) = s else { return 0.0 };
        s.trim().parse::<f32>().ok().or_else(|| guides.get(s).copied()).unwrap_or(0.0)
    };
    let mut out = Vec::new();
    let Some(list) = cg.child("pathLst") else { return out };
    for p in list.children("path") {
        let pw = p.attr_f64("w").unwrap_or(0.0) as f32;
        let ph = p.attr_f64("h").unwrap_or(0.0) as f32;
        let sx = if pw > 0.0 { w / pw } else { 1.0 };
        let sy = if ph > 0.0 { h / ph } else { 1.0 };
        let pt = |e: &Elem| (val(e.attr("x")) * sx, val(e.attr("y")) * sy);
        let mut pb = PathBuilder::new();
        let mut cur = (0.0f32, 0.0f32);
        for c in &p.children {
            let pts: Vec<(f32, f32)> = c.children("pt").map(pt).collect();
            match c.name.as_str() {
                "moveTo" => {
                    if let Some(&q) = pts.first() {
                        pb.move_to(q.0, q.1);
                        cur = q;
                    }
                }
                "lnTo" => {
                    if let Some(&q) = pts.first() {
                        pb.line_to(q.0, q.1);
                        cur = q;
                    }
                }
                "cubicBezTo" if pts.len() >= 3 => {
                    pb.cubic_to(pts[0].0, pts[0].1, pts[1].0, pts[1].1, pts[2].0, pts[2].1);
                    cur = pts[2];
                }
                "quadBezTo" if pts.len() >= 2 => {
                    pb.quad_to(pts[0].0, pts[0].1, pts[1].0, pts[1].1);
                    cur = pts[1];
                }
                "arcTo" => {
                    let rx = val(c.attr("wR")) * sx;
                    let ry = val(c.attr("hR")) * sy;
                    let st = (val(c.attr("stAng")) / 60_000.0).to_radians();
                    let sw = (val(c.attr("swAng")) / 60_000.0).to_radians();
                    let (cx, cy) = (cur.0 - rx * st.cos(), cur.1 - ry * st.sin());
                    if pb.is_empty() {
                        pb.move_to(cur.0, cur.1);
                    }
                    arc_to(&mut pb, cx, cy, rx, ry, st, sw);
                    cur = (cx + rx * (st + sw).cos(), cy + ry * (st + sw).sin());
                }
                "close" => pb.close(),
                _ => {}
            }
        }
        if let Some(path) = pb.finish() {
            let fill = p.attr("fill").map(|f| f != "none").unwrap_or(true);
            let stroke = p.attr_bool("stroke").unwrap_or(true);
            out.push(GPath { path, fill, stroke });
        }
    }
    out
}
