//! Graphiques simples (`c:chart`), dessinés à partir des valeurs en cache du fichier :
//! histogrammes et barres (groupés, empilés, 100 %), courbes, aires, secteurs et anneaux,
//! avec titre, légende, axes et quadrillage. Pas d'étiquettes de données ni de 3D.

use crate::color::{ColorCtx, Rgba};
use crate::fill::{parse_fill, Fill};
use crate::fonts::Fonts;
use crate::geom::arc_to;
use crate::style::{Align, ListStyle, RunProps};
use crate::text::{self, LayoutOpts, EMU_PER_PT};
use crate::theme::Theme;
use crate::xml::Elem;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub struct ChartEnv<'a, 'b> {
    pub pix: &'a mut Pixmap,
    pub fonts: &'a mut Fonts,
    pub theme: &'b Theme,
    pub cc: ColorCtx<'b>,
    /// Pixels par EMU.
    pub scale: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Stack {
    None,
    Stacked,
    Percent,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Bar { horizontal: bool },
    Line { markers: bool },
    Area,
    Pie { hole: f32 },
}

struct Series {
    name: String,
    values: Vec<Option<f64>>,
    color: Rgba,
    line: Option<(Rgba, f32)>,
    points: HashMap<usize, Rgba>,
    marker: bool,
}

struct Group {
    kind: Kind,
    stack: Stack,
    gap: f32,
    overlap: f32,
    vary: bool,
    series: Vec<Series>,
}

const GRID: Rgba = Rgba { r: 0.851, g: 0.851, b: 0.851, a: 1.0 };
const TEXT: Rgba = Rgba { r: 0.349, g: 0.349, b: 0.349, a: 1.0 };

fn cache_pts(el: Option<&Elem>) -> Vec<(usize, String)> {
    let Some(el) = el else { return Vec::new() };
    let cache = ["strRef", "numRef", "multiLvlStrRef"]
        .iter()
        .find_map(|r| el.child(r).and_then(|r| r.child("strCache").or_else(|| r.child("numCache")).or_else(|| r.child("multiLvlStrCache"))))
        .or_else(|| el.child("strLit"))
        .or_else(|| el.child("numLit"));
    let Some(cache) = cache else { return Vec::new() };
    // Catégories à plusieurs niveaux : le premier niveau suffit.
    let cache = cache.child("lvl").unwrap_or(cache);
    cache.children("pt").filter_map(|p| Some((p.attr_i64("idx")?.max(0) as usize, p.child("v").map(|v| v.text.clone()).unwrap_or_default()))).collect()
}

fn pt_count(el: Option<&Elem>) -> usize {
    let mut v = Vec::new();
    if let Some(e) = el {
        e.find_all("ptCount", &mut v);
    }
    v.first().and_then(|p| p.attr_i64("val")).unwrap_or(0).clamp(0, 100_000) as usize
}

fn val_attr(el: &Elem, name: &str) -> Option<String> {
    el.child(name).and_then(|c| c.attr("val")).map(str::to_string)
}

fn accent(cc: &ColorCtx, i: usize) -> Rgba {
    let base = cc.scheme_color(&format!("accent{}", i % 6 + 1)).unwrap_or(Rgba::from_u8(0x44, 0x72, 0xC4));
    // Au-delà de six séries, PowerPoint reprend les couleurs plus sombres puis plus claires.
    let k = match (i / 6) % 3 {
        0 => 1.0,
        1 => 0.6,
        _ => 1.4,
    };
    let f = |c: f32| if k < 1.0 { c * k } else { c + (1.0 - c) * (k - 1.0) };
    Rgba { r: f(base.r), g: f(base.g), b: f(base.b), a: 1.0 }
}

fn parse_group(el: &Elem, env: &ChartEnv, first_index: usize) -> Option<Group> {
    let kind = match el.name.as_str() {
        "barChart" | "bar3DChart" => Kind::Bar { horizontal: val_attr(el, "barDir").as_deref() == Some("bar") },
        "lineChart" | "line3DChart" | "stockChart" => Kind::Line { markers: val_attr(el, "marker").map(|v| v != "0").unwrap_or(true) },
        "areaChart" | "area3DChart" => Kind::Area,
        "pieChart" | "pie3DChart" | "ofPieChart" => Kind::Pie { hole: 0.0 },
        "doughnutChart" => Kind::Pie { hole: el.child("holeSize").and_then(|h| h.attr_f64("val")).unwrap_or(50.0).clamp(10.0, 90.0) as f32 / 100.0 },
        _ => return None,
    };
    let stack = match val_attr(el, "grouping").as_deref() {
        Some("stacked") => Stack::Stacked,
        Some("percentStacked") => Stack::Percent,
        _ => Stack::None,
    };
    let gap = el.child("gapWidth").and_then(|g| g.attr_f64("val")).unwrap_or(150.0).clamp(0.0, 500.0) as f32 / 100.0;
    let overlap = el.child("overlap").and_then(|g| g.attr_f64("val")).unwrap_or(if stack == Stack::None { 0.0 } else { 100.0 }).clamp(-100.0, 100.0) as f32 / 100.0;
    let vary = el.child("varyColors").and_then(|v| v.attr_bool("val")).unwrap_or(matches!(kind, Kind::Pie { .. }));
    let mut series = Vec::new();
    for (k, s) in el.children("ser").enumerate() {
        let idx = s.child("idx").and_then(|i| i.attr_i64("val")).map(|v| v.max(0) as usize).unwrap_or(first_index + k);
        let name = s.child("tx").map(|tx| cache_pts(Some(tx)).first().map(|p| p.1.clone()).or_else(|| tx.child("v").map(|v| v.text.clone())).unwrap_or_default()).unwrap_or_else(|| format!("Série {}", idx + 1));
        let val = s.child("val").or_else(|| s.child("yVal"));
        let mut values = vec![None; pt_count(val)];
        for (i, v) in cache_pts(val) {
            if i < 100_000 {
                if values.len() <= i {
                    values.resize(i + 1, None);
                }
                values[i] = v.trim().parse::<f64>().ok().filter(|f| f.is_finite());
            }
        }
        let sp = s.child("spPr");
        let fill = sp.and_then(|p| parse_fill(p, &env.cc, &[])).and_then(|f| match f {
            Fill::None => Some(Rgba::BLACK.with_alpha(0.0)),
            f => f.main_color(),
        });
        let ln = sp.and_then(|p| p.child("ln"));
        let line_color = ln.and_then(|l| parse_fill(l, &env.cc, &[])).map(|f| match f {
            Fill::None => Rgba::BLACK.with_alpha(0.0),
            f => f.main_color().unwrap_or(Rgba::BLACK),
        });
        let line_w = ln.and_then(|l| l.attr_f64("w")).unwrap_or(28_575.0) as f32;
        let color = fill.or(line_color.filter(|_| matches!(kind, Kind::Line { .. }))).unwrap_or_else(|| accent(&env.cc, idx));
        let mut points = HashMap::new();
        for d in s.children("dPt") {
            if let (Some(i), Some(c)) = (d.child("idx").and_then(|i| i.attr_i64("val")), d.child("spPr").and_then(|p| parse_fill(p, &env.cc, &[])).and_then(|f| f.main_color())) {
                points.insert(i.max(0) as usize, c);
            }
        }
        let marker = s.path(&["marker", "symbol"]).and_then(|m| m.attr("val")).map(|v| v != "none").unwrap_or(true);
        series.push(Series { name, values, color, line: line_color.map(|c| (c, line_w)), points, marker });
    }
    Some(Group { kind, stack, gap, overlap, vary, series })
}

/// Propriétés de texte d'un `c:txPr` superposées à `base`.
fn text_props(tx_pr: Option<&Elem>, base: &RunProps, cc: &ColorCtx) -> RunProps {
    let mut r = base.clone();
    if let Some(d) = tx_pr.and_then(|t| t.find("defRPr")) {
        r.merge(&RunProps::parse(d, cc));
    }
    r
}

impl ChartEnv<'_, '_> {
    fn paint(c: Rgba) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color(c.to_skia());
        p.anti_alias = true;
        p
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba, tf: Transform) {
        let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
        let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
        if let Some(r) = Rect::from_xywh(x, y, w.max(1.0), h.max(1.0)) {
            self.pix.fill_rect(r, &Self::paint(c), tf, None);
        }
    }

    fn line(&mut self, pts: &[(f32, f32)], c: Rgba, width: f32, tf: Transform) {
        if pts.len() < 2 || c.a <= 0.0 {
            return;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            pb.line_to(p.0, p.1);
        }
        if let Some(path) = pb.finish() {
            let width = width.max(1.0 / self.scale);
            let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
            self.pix.stroke_path(&path, &Self::paint(c), &stroke, tf, None);
        }
    }

    /// Texte d'une ligne ; renvoie (largeur, hauteur) en EMU. `ax`/`ay` : 0 début, 0.5 centre, 1 fin.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, s: &str, props: &RunProps, x: f32, y: f32, ax: f32, ay: f32, tf: Option<Transform>) -> (f32, f32) {
        let para = text::simple_para(s, props.clone(), Align::Left);
        let opts = LayoutOpts { width: 10_000.0, wrap: false, font_scale: 1.0, line_reduction: 0.0 };
        let laid = text::layout(&[para], &opts, self.fonts, self.theme);
        let (w, h) = (laid.width * EMU_PER_PT, laid.height * EMU_PER_PT);
        if let Some(tf) = tf {
            let t = tf.pre_translate(x - w * ax, y - h * ay).pre_scale(EMU_PER_PT, EMU_PER_PT);
            text::draw(&laid, self.pix, self.fonts, t);
        }
        (w, h)
    }
}

fn fmt_num(v: f64, decimals: usize, percent: bool) -> String {
    let v = if percent { v * 100.0 } else { v };
    // Format « Standard » : pas de zéros inutiles après la virgule.
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    let s = if s == "-0" { "0".to_string() } else { s };
    if percent {
        format!("{s} %")
    } else {
        s
    }
}

/// Graduation « ronde » de l'axe des valeurs : (min, max, pas).
fn nice_axis(lo: f64, hi: f64, fixed_min: Option<f64>, fixed_max: Option<f64>) -> (f64, f64, f64) {
    let lo = fixed_min.unwrap_or(if lo >= 0.0 { 0.0 } else { lo });
    let mut hi = fixed_max.unwrap_or(hi);
    if hi <= lo {
        hi = lo + 1.0;
    }
    let head = if fixed_max.is_none() { (hi - lo) / 20.0 } else { 0.0 };
    let raw = (hi + head - lo) / 8.0;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = mag * if norm <= 1.0 { 1.0 } else if norm <= 2.0 { 2.0 } else if norm <= 5.0 { 5.0 } else { 10.0 };
    let min = fixed_min.unwrap_or((lo / step).floor() * step);
    let max = fixed_max.unwrap_or(((hi + head) / step).ceil() * step);
    (min, max.max(min + step), step)
}

pub fn draw_chart(env: &mut ChartEnv, space: &Elem, w: f32, h: f32, tf: Transform) {
    let Some(chart) = space.child("chart") else { return };
    let Some(plot) = chart.child("plotArea") else { return };
    let cc = env.cc;
    // Fond du graphique.
    if let Some(Fill::Solid(c)) = space.child("spPr").and_then(|p| parse_fill(p, &cc, &[])) {
        env.rect(0.0, 0.0, w, h, c, tf);
    }
    let base = text_props(space.child("txPr"), &RunProps { size: Some(10.0), color: Some(TEXT), ..Default::default() }, &cc);
    let mut groups = Vec::new();
    for el in &plot.children {
        let first = groups.iter().map(|g: &Group| g.series.len()).sum();
        if let Some(g) = parse_group(el, env, first) {
            if !g.series.is_empty() {
                groups.push(g);
            }
        }
    }
    if groups.is_empty() {
        return;
    }
    let categories: Vec<String> = {
        let ser = plot.children.iter().find_map(|g| g.child("ser"));
        let cat = ser.and_then(|s| s.child("cat").or_else(|| s.child("xVal")));
        let n = groups.iter().flat_map(|g| g.series.iter().map(|s| s.values.len())).max().unwrap_or(0).max(pt_count(cat));
        let mut v: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
        for (i, s) in cache_pts(cat) {
            if i < v.len() {
                v[i] = s;
            }
        }
        v
    };
    let pad = w.min(h) * 0.03;
    let mut top = pad;

    // Titre : texte explicite, ou nom de la série unique.
    let all_series: Vec<&Series> = groups.iter().flat_map(|g| g.series.iter()).collect();
    let auto_deleted = chart.child("autoTitleDeleted").and_then(|a| a.attr_bool("val")).unwrap_or(false);
    let title_el = chart.child("title");
    let title_text = title_el.and_then(|t| t.path(&["tx", "rich"]));
    let title_props = text_props(title_el.and_then(|t| t.child("txPr")), &RunProps { size: Some(base.size.unwrap_or(10.0) * 1.4), bold: Some(true), ..base.clone() }, &cc);
    if let Some(rich) = title_text {
        let mut levels = ListStyle::default();
        for l in levels.0.iter_mut() {
            l.run = title_props.clone();
            l.align = Some(Align::Center);
        }
        let paras = text::collect_paragraphs(rich, &levels, &cc, 0);
        let opts = LayoutOpts { width: (w - 2.0 * pad) / EMU_PER_PT, wrap: true, font_scale: 1.0, line_reduction: 0.0 };
        let laid = text::layout(&paras, &opts, env.fonts, env.theme);
        text::draw(&laid, env.pix, env.fonts, tf.pre_translate(pad, top).pre_scale(EMU_PER_PT, EMU_PER_PT));
        top += laid.height * EMU_PER_PT + pad / 2.0;
    } else if (title_el.is_some() || !auto_deleted) && all_series.len() == 1 && !all_series[0].name.is_empty() && !(title_el.is_none() && matches!(groups[0].kind, Kind::Bar { .. } | Kind::Line { .. } | Kind::Area) && auto_deleted) {
        let (_, th) = env.text(&all_series[0].name.clone(), &title_props, w / 2.0, top, 0.5, 0.0, Some(tf));
        top += th + pad / 2.0;
    }

    // Légende.
    let pie = groups.iter().find(|g| matches!(g.kind, Kind::Pie { .. }));
    let legend = chart.child("legend").filter(|l| !l.child("delete").and_then(|d| d.attr_bool("val")).unwrap_or(false));
    let mut area = (pad, top, w - pad, h - pad);
    if let Some(lg) = legend {
        let props = text_props(lg.child("txPr"), &base, &cc);
        let entries: Vec<(String, Rgba)> = match pie {
            Some(g) => {
                let s = &g.series[0];
                categories.iter().enumerate().map(|(i, c)| (c.clone(), s.points.get(&i).copied().unwrap_or_else(|| accent(&cc, i)))).collect()
            }
            None => groups.iter().flat_map(|g| g.series.iter().map(|s| (s.name.clone(), s.color))).collect(),
        };
        let pos = val_attr(lg, "legendPos").unwrap_or_else(|| "r".into());
        let sizes: Vec<(f32, f32)> = entries.iter().map(|(t, _)| env.text(t, &props, 0.0, 0.0, 0.0, 0.0, None)).collect();
        let line_h = sizes.iter().map(|s| s.1).fold(0.0, f32::max).max(1.0);
        let sw = line_h * 0.55;
        let entry_w: Vec<f32> = sizes.iter().map(|s| sw * 1.6 + s.0).collect();
        let draw_entry = |env: &mut ChartEnv, i: usize, x: f32, y: f32| {
            let (t, c) = &entries[i];
            env.rect(x, y + (line_h - sw) / 2.0, sw, sw, *c, tf);
            env.text(t, &props, x + sw * 1.6, y, 0.0, 0.0, Some(tf));
        };
        match pos.as_str() {
            "b" | "t" => {
                let total: f32 = entry_w.iter().sum::<f32>() + sw * 1.5 * entries.len().saturating_sub(1) as f32;
                let y = if pos == "b" { area.3 - line_h } else { area.1 };
                let mut x = (w - total) / 2.0;
                for (i, ew) in entry_w.iter().enumerate() {
                    draw_entry(env, i, x, y);
                    x += ew + sw * 1.5;
                }
                if pos == "b" {
                    area.3 -= line_h + pad / 2.0;
                } else {
                    area.1 += line_h + pad / 2.0;
                }
            }
            _ => {
                let lw = entry_w.iter().fold(0.0f32, |a, b| a.max(*b));
                let total = line_h * 1.2 * entries.len() as f32;
                let x = if pos == "l" { area.0 } else { area.2 - lw };
                let mut y = area.1 + ((area.3 - area.1) - total).max(0.0) / 2.0;
                for i in 0..entries.len() {
                    draw_entry(env, i, x, y);
                    y += line_h * 1.2;
                }
                if pos == "l" {
                    area.0 += lw + pad;
                } else {
                    area.2 -= lw + pad;
                }
            }
        }
    }
    if area.2 - area.0 < 1.0 || area.3 - area.1 < 1.0 {
        return;
    }

    if let Some(g) = pie {
        draw_pie(env, g, area, tf);
        return;
    }

    // Axes.
    let horizontal = groups.iter().any(|g| matches!(g.kind, Kind::Bar { horizontal: true }));
    let val_ax = plot.child("valAx");
    let cat_ax = plot.child("catAx").or_else(|| plot.child("dateAx"));
    let deleted = |a: Option<&Elem>| a.and_then(|a| a.child("delete")).and_then(|d| d.attr_bool("val")).unwrap_or(false);
    let percent = groups.iter().any(|g| g.stack == Stack::Percent);
    let n = categories.len().max(1);
    let mut lo = 0.0f64;
    let mut hi = 0.0f64;
    for g in &groups {
        for i in 0..n {
            let vals = g.series.iter().map(|s| s.values.get(i).copied().flatten().unwrap_or(0.0));
            match g.stack {
                Stack::None => {
                    for v in vals {
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
                Stack::Stacked => {
                    let (p, m): (Vec<f64>, Vec<f64>) = vals.partition(|v| *v >= 0.0);
                    hi = hi.max(p.iter().sum());
                    lo = lo.min(m.iter().sum());
                }
                Stack::Percent => hi = hi.max(1.0),
            }
        }
    }
    let scaling = val_ax.and_then(|a| a.child("scaling"));
    let fixed = |n: &str| scaling.and_then(|s| s.child(n)).and_then(|m| m.attr_f64("val"));
    // Empilé 100 % : l'axe s'arrête à 100 %.
    let fixed_max = fixed("max").or(if percent && lo >= 0.0 { Some(1.0) } else { None });
    let (vmin, vmax, step) = nice_axis(lo, hi, fixed("min"), fixed_max);
    let decimals = if percent { 0 } else { (-(step.log10().floor())).max(0.0) as usize + usize::from(((step / 10f64.powf(step.log10().floor())) - 2.5).abs() < 1e-9) };
    let fmt_pct = percent || val_ax.and_then(|a| a.child("numFmt")).and_then(|f| f.attr("formatCode")).map(|f| f.contains('%')).unwrap_or(false);
    let val_props = text_props(val_ax.and_then(|a| a.child("txPr")), &base, &cc);
    let cat_props = text_props(cat_ax.and_then(|a| a.child("txPr")), &base, &cc);
    let ticks: Vec<f64> = (0..=((vmax - vmin) / step).round() as usize).map(|k| vmin + k as f64 * step).collect();
    let labels: Vec<String> = ticks.iter().map(|t| fmt_num(*t, decimals, fmt_pct)).collect();
    let show_val = !deleted(val_ax);
    let show_cat = !deleted(cat_ax);
    let val_w = if show_val { labels.iter().map(|l| env.text(l, &val_props, 0.0, 0.0, 0.0, 0.0, None).0).fold(0.0, f32::max) } else { 0.0 };
    let cat_h = if show_cat { env.text("Ag", &cat_props, 0.0, 0.0, 0.0, 0.0, None).1 } else { 0.0 };
    let cat_w = if show_cat && horizontal { categories.iter().map(|c| env.text(c, &cat_props, 0.0, 0.0, 0.0, 0.0, None).0).fold(0.0, f32::max) } else { 0.0 };
    let val_h = env.text("0", &val_props, 0.0, 0.0, 0.0, 0.0, None).1;
    let (px0, py0, px1, py1) = if horizontal {
        (area.0 + cat_w + pad / 2.0, area.1 + val_h / 2.0, area.2 - pad, area.3 - if show_val { val_h + pad / 3.0 } else { 0.0 })
    } else {
        (area.0 + val_w + pad / 2.0, area.1 + val_h / 2.0, area.2 - pad / 2.0, area.3 - cat_h - pad / 3.0)
    };
    if px1 - px0 < 1.0 || py1 - py0 < 1.0 {
        return;
    }
    let to_val = |v: f64| -> f32 {
        let t = ((v - vmin) / (vmax - vmin)) as f32;
        if horizontal {
            px0 + t * (px1 - px0)
        } else {
            py1 - t * (py1 - py0)
        }
    };
    let cat_len = if horizontal { py1 - py0 } else { px1 - px0 };
    let slot = cat_len / n as f32;
    // Position le long de l'axe des catégories (en barres horizontales, la première est en bas).
    let to_cat = |c: f32| -> f32 {
        if horizontal {
            py1 - c
        } else {
            px0 + c
        }
    };
    let grid_w = 9525.0;
    let grid = val_ax.and_then(|a| a.child("majorGridlines")).is_some();
    for (k, t) in ticks.iter().enumerate() {
        let p = to_val(*t);
        if grid {
            if horizontal {
                env.line(&[(p, py0), (p, py1)], GRID, grid_w, tf);
            } else {
                env.line(&[(px0, p), (px1, p)], GRID, grid_w, tf);
            }
        }
        if show_val {
            if horizontal {
                env.text(&labels[k], &val_props, p, py1 + pad / 3.0, 0.5, 0.0, Some(tf));
            } else {
                env.text(&labels[k], &val_props, px0 - pad / 3.0, p, 1.0, 0.5, Some(tf));
            }
        }
    }

    // Séries.
    for g in &groups {
        let ns = g.series.len();
        match g.kind {
            Kind::Bar { .. } => {
                let group_w = slot / (1.0 + g.gap);
                let per = if g.stack == Stack::None { ns as f32 - (ns as f32 - 1.0) * g.overlap } else { 1.0 };
                let bw = group_w / per.max(1.0);
                for i in 0..n {
                    let total: f64 = g.series.iter().map(|s| s.values.get(i).copied().flatten().unwrap_or(0.0).abs()).sum();
                    let (mut pos, mut neg) = (0.0f64, 0.0f64);
                    for (j, s) in g.series.iter().enumerate() {
                        let Some(mut v) = s.values.get(i).copied().flatten() else { continue };
                        if g.stack == Stack::Percent {
                            v = if total > 0.0 { v / total } else { 0.0 };
                        }
                        let (a, b) = match g.stack {
                            Stack::None => (0.0f64.clamp(vmin, vmax), v),
                            _ => {
                                if v >= 0.0 {
                                    pos += v;
                                    (pos - v, pos)
                                } else {
                                    neg += v;
                                    (neg - v, neg)
                                }
                            }
                        };
                        let off = if g.stack == Stack::None { j as f32 * bw * (1.0 - g.overlap) } else { 0.0 };
                        let c0 = slot * i as f32 + (slot - group_w) / 2.0 + off;
                        let color = if g.vary && ns == 1 { s.points.get(&i).copied().unwrap_or_else(|| accent(&cc, i)) } else { s.points.get(&i).copied().unwrap_or(s.color) };
                        let (va, vb) = (to_val(a.clamp(vmin, vmax)), to_val(b.clamp(vmin, vmax)));
                        if horizontal {
                            let y = to_cat(c0 + bw);
                            env.rect(va, y, vb - va, bw, color, tf);
                        } else {
                            env.rect(to_cat(c0), vb, bw, va - vb, color, tf);
                        }
                    }
                }
            }
            Kind::Line { markers } => {
                let mut acc = vec![0.0f64; n];
                for s in &g.series {
                    let mut pts = Vec::new();
                    let mut segs: Vec<Vec<(f32, f32)>> = Vec::new();
                    for (i, a) in acc.iter_mut().enumerate().take(n) {
                        match s.values.get(i).copied().flatten() {
                            Some(v) => {
                                let v = if g.stack == Stack::None { v } else { *a + v };
                                *a = v;
                                let c = to_cat(slot * (i as f32 + 0.5));
                                let p = to_val(v.clamp(vmin, vmax));
                                pts.push(if horizontal { (p, c) } else { (c, p) });
                            }
                            None => {
                                if !pts.is_empty() {
                                    segs.push(std::mem::take(&mut pts));
                                }
                            }
                        }
                    }
                    segs.push(pts);
                    let (lc, lw) = s.line.unwrap_or((s.color, 28_575.0));
                    for seg in &segs {
                        env.line(seg, lc, lw, tf);
                        if markers && s.marker {
                            for p in seg {
                                let r = 2.5 * EMU_PER_PT;
                                if let Some(path) = Rect::from_xywh(p.0 - r, p.1 - r, 2.0 * r, 2.0 * r).and_then(PathBuilder::from_oval) {
                                    env.pix.fill_path(&path, &ChartEnv::paint(s.color), FillRule::Winding, tf, None);
                                }
                            }
                        }
                    }
                }
            }
            Kind::Area => {
                let mut acc = vec![0.0f64; n];
                for s in &g.series {
                    let base_line: Vec<f64> = acc.clone();
                    let mut pb = PathBuilder::new();
                    for (i, a) in acc.iter_mut().enumerate() {
                        let v = s.values.get(i).copied().flatten().unwrap_or(0.0);
                        *a = if g.stack == Stack::None { v } else { *a + v };
                        let c = to_cat(if n > 1 { cat_len * i as f32 / (n - 1) as f32 } else { cat_len / 2.0 });
                        let p = to_val(a.clamp(vmin, vmax));
                        let (x, y) = if horizontal { (p, c) } else { (c, p) };
                        if i == 0 {
                            pb.move_to(x, y);
                        } else {
                            pb.line_to(x, y);
                        }
                    }
                    for i in (0..n).rev() {
                        let b = if g.stack == Stack::None { 0.0f64.clamp(vmin, vmax) } else { base_line[i].clamp(vmin, vmax) };
                        let c = to_cat(if n > 1 { cat_len * i as f32 / (n - 1) as f32 } else { cat_len / 2.0 });
                        let p = to_val(b);
                        let (x, y) = if horizontal { (p, c) } else { (c, p) };
                        pb.line_to(x, y);
                    }
                    pb.close();
                    if let Some(path) = pb.finish() {
                        env.pix.fill_path(&path, &ChartEnv::paint(s.color), FillRule::Winding, tf, None);
                    }
                }
            }
            Kind::Pie { .. } => {}
        }
    }

    // Axe des catégories et ses étiquettes.
    let axis_at = to_val(0.0f64.clamp(vmin, vmax));
    if horizontal {
        env.line(&[(axis_at, py0), (axis_at, py1)], GRID, grid_w, tf);
    } else {
        env.line(&[(px0, axis_at), (px1, axis_at)], GRID, grid_w, tf);
    }
    if show_cat {
        // Une étiquette sur deux, trois… si elles ne tiennent pas.
        let widest = if horizontal { 0.0 } else { categories.iter().map(|c| env.text(c, &cat_props, 0.0, 0.0, 0.0, 0.0, None).0).fold(0.0, f32::max) };
        let every = if horizontal { ((cat_h * 1.1) / slot).ceil().max(1.0) as usize } else { ((widest * 1.1) / slot).ceil().max(1.0) as usize };
        let area_like = groups.iter().all(|g| g.kind == Kind::Area);
        for (i, c) in categories.iter().enumerate().step_by(every) {
            let pos = if area_like && n > 1 { cat_len * i as f32 / (n - 1) as f32 } else { slot * (i as f32 + 0.5) };
            if horizontal {
                env.text(c, &cat_props, px0 - pad / 2.0, to_cat(pos), 1.0, 0.5, Some(tf));
            } else {
                env.text(c, &cat_props, to_cat(pos), py1 + pad / 3.0, 0.5, 0.0, Some(tf));
            }
        }
    }
}

fn draw_pie(env: &mut ChartEnv, g: &Group, area: (f32, f32, f32, f32), tf: Transform) {
    let s = &g.series[0];
    let total: f64 = s.values.iter().flatten().filter(|v| **v > 0.0).sum();
    if total <= 0.0 {
        return;
    }
    let (cx, cy) = ((area.0 + area.2) / 2.0, (area.1 + area.3) / 2.0);
    let r = ((area.2 - area.0).min(area.3 - area.1) / 2.0) * 0.95;
    let hole = match g.kind {
        Kind::Pie { hole } => hole,
        _ => 0.0,
    };
    let mut a = -FRAC_PI_2;
    let cc = env.cc;
    for (i, v) in s.values.iter().enumerate() {
        let Some(v) = v.filter(|v| *v > 0.0) else { continue };
        let sweep = (v / total) as f32 * 2.0 * PI;
        let color = s.points.get(&i).copied().unwrap_or_else(|| if g.vary { accent(&cc, i) } else { s.color });
        let mut pb = PathBuilder::new();
        if hole > 0.0 {
            pb.move_to(cx + r * a.cos(), cy + r * a.sin());
            arc_to(&mut pb, cx, cy, r, r, a, sweep);
            let ri = r * hole;
            pb.line_to(cx + ri * (a + sweep).cos(), cy + ri * (a + sweep).sin());
            arc_to(&mut pb, cx, cy, ri, ri, a + sweep, -sweep);
        } else {
            pb.move_to(cx, cy);
            pb.line_to(cx + r * a.cos(), cy + r * a.sin());
            arc_to(&mut pb, cx, cy, r, r, a, sweep);
        }
        pb.close();
        if let Some(path) = pb.finish() {
            env.pix.fill_path(&path, &ChartEnv::paint(color), FillRule::Winding, tf, None);
            let stroke = Stroke { width: 9525.0f32.max(1.0 / env.scale), line_join: LineJoin::Round, ..Default::default() };
            env.pix.stroke_path(&path, &ChartEnv::paint(Rgba::WHITE), &stroke, tf, None);
        }
        a += sweep;
    }
}
