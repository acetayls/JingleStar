//! Couleurs DrawingML : sRGB, couleurs du thème (via `p:clrMap`), couleurs système, et leurs
//! modificateurs (`lumMod`, `lumOff`, `tint`, `shade`, `alpha`…).

use crate::xml::Elem;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    pub fn from_hex(s: &str) -> Option<Rgba> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgba::from_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn from_u8(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    pub fn with_alpha(self, a: f32) -> Rgba {
        Rgba { a, ..self }
    }

    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba(self.r.clamp(0.0, 1.0), self.g.clamp(0.0, 1.0), self.b.clamp(0.0, 1.0), self.a.clamp(0.0, 1.0))
            .unwrap_or(tiny_skia::Color::BLACK)
    }

    fn to_hsl(self) -> (f32, f32, f32) {
        let (r, g, b) = (self.r, self.g, self.b);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        if (max - min).abs() < 1e-6 {
            return (0.0, 0.0, l);
        }
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0, s, l)
    }

    fn from_hsl(h: f32, s: f32, l: f32, a: f32) -> Rgba {
        let (h, s, l) = (h.rem_euclid(360.0) / 360.0, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
        if s == 0.0 {
            return Rgba { r: l, g: l, b: l, a };
        }
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let f = |mut t: f32| {
            if t < 0.0 {
                t += 1.0;
            }
            if t > 1.0 {
                t -= 1.0;
            }
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        Rgba { r: f(h + 1.0 / 3.0), g: f(h), b: f(h - 1.0 / 3.0), a }
    }
}

// Composantes « linéaires » pour tint / shade, comme les autres lecteurs OOXML.
const GAMMA: f32 = 2.3;
fn to_lin(c: f32) -> f32 {
    c.clamp(0.0, 1.0).powf(GAMMA)
}
fn from_lin(c: f32) -> f32 {
    c.clamp(0.0, 1.0).powf(1.0 / GAMMA)
}

/// Correspondance des couleurs logiques (`tx1`, `bg1`…) vers celles du thème (`dk1`, `lt1`…).
#[derive(Debug, Clone)]
pub struct ClrMap(pub HashMap<String, String>);

impl Default for ClrMap {
    fn default() -> Self {
        let mut m = HashMap::new();
        for (k, v) in [("bg1", "lt1"), ("tx1", "dk1"), ("bg2", "lt2"), ("tx2", "dk2")] {
            m.insert(k.to_string(), v.to_string());
        }
        ClrMap(m)
    }
}

impl ClrMap {
    pub fn from_elem(el: &Elem) -> ClrMap {
        let mut m = ClrMap::default();
        for (k, v) in &el.attrs {
            m.0.insert(k.clone(), v.clone());
        }
        m
    }

    /// Superpose une correspondance (`p:clrMapOvr/a:overrideClrMapping`).
    pub fn overridden(&self, ovr: Option<&Elem>) -> ClrMap {
        match ovr.and_then(|o| o.child("overrideClrMapping")) {
            Some(o) => {
                let mut m = self.clone();
                for (k, v) in &o.attrs {
                    m.0.insert(k.clone(), v.clone());
                }
                m
            }
            None => self.clone(),
        }
    }
}

/// De quoi résoudre une couleur : thème, correspondance, couleur de substitution (`phClr`).
#[derive(Clone, Copy)]
pub struct ColorCtx<'a> {
    pub scheme: &'a HashMap<String, Rgba>,
    pub map: &'a ClrMap,
    pub ph_clr: Option<Rgba>,
}

impl ColorCtx<'_> {
    pub fn scheme_color(&self, name: &str) -> Option<Rgba> {
        if name == "phClr" {
            return Some(self.ph_clr.unwrap_or(Rgba::BLACK));
        }
        let mapped = self.map.0.get(name).map(String::as_str).unwrap_or(name);
        self.scheme
            .get(mapped)
            .or(self.scheme.get(name))
            .copied()
            .or(match mapped {
                "dk1" | "tx1" => Some(Rgba::BLACK),
                "lt1" | "bg1" => Some(Rgba::WHITE),
                _ => None,
            })
    }
}

pub const COLOR_ELEMS: &[&str] = &["srgbClr", "schemeClr", "sysClr", "prstClr", "scrgbClr", "hslClr"];

/// Première couleur enfant de `parent` (ex. `a:solidFill`, `a:buClr`, `a:gs`).
pub fn child_color(parent: &Elem, ctx: &ColorCtx) -> Option<Rgba> {
    parent.children.iter().find(|c| COLOR_ELEMS.contains(&c.name.as_str())).and_then(|c| parse_color(c, ctx))
}

/// Couleur décrite par un élément `a:srgbClr`, `a:schemeClr`… modificateurs compris.
pub fn parse_color(el: &Elem, ctx: &ColorCtx) -> Option<Rgba> {
    let base = match el.name.as_str() {
        "srgbClr" => Rgba::from_hex(el.attr("val")?)?,
        "schemeClr" => ctx.scheme_color(el.attr("val")?)?,
        "sysClr" => el.attr("lastClr").and_then(Rgba::from_hex).or_else(|| system_color(el.attr("val")?))?,
        "prstClr" => preset_color(el.attr("val")?)?,
        "scrgbClr" => {
            let c = |n: &str| from_lin(el.attr_f64(n).unwrap_or(0.0) as f32 / 100_000.0);
            Rgba { r: c("r"), g: c("g"), b: c("b"), a: 1.0 }
        }
        "hslClr" => {
            let h = el.attr_f64("hue").unwrap_or(0.0) as f32 / 60_000.0;
            let s = el.attr_f64("sat").unwrap_or(0.0) as f32 / 100_000.0;
            let l = el.attr_f64("lum").unwrap_or(0.0) as f32 / 100_000.0;
            Rgba::from_hsl(h, s, l, 1.0)
        }
        _ => return None,
    };
    Some(apply_mods(base, el))
}

fn apply_mods(mut c: Rgba, el: &Elem) -> Rgba {
    for m in &el.children {
        let v = m.attr_f64("val").unwrap_or(0.0) as f32 / 100_000.0;
        match m.name.as_str() {
            "alpha" => c.a = v.clamp(0.0, 1.0),
            "alphaMod" => c.a = (c.a * v).clamp(0.0, 1.0),
            "alphaOff" => c.a = (c.a + v).clamp(0.0, 1.0),
            "lumMod" | "lumOff" | "satMod" | "satOff" | "hueMod" | "hueOff" | "sat" | "lum" | "hue" => {
                let (mut h, mut s, mut l) = c.to_hsl();
                match m.name.as_str() {
                    "lumMod" => l *= v,
                    "lumOff" => l += v,
                    "lum" => l = v,
                    "satMod" => s *= v,
                    "satOff" => s += v,
                    "sat" => s = v,
                    // Teinte en 60 000es de degré.
                    "hueMod" => h *= v,
                    "hueOff" => h += m.attr_f64("val").unwrap_or(0.0) as f32 / 60_000.0,
                    _ => h = m.attr_f64("val").unwrap_or(0.0) as f32 / 60_000.0,
                }
                c = Rgba::from_hsl(h, s, l.clamp(0.0, 1.0), c.a);
            }
            "tint" => {
                let t = |x: f32| from_lin(1.0 - (1.0 - to_lin(x)) * v);
                c = Rgba { r: t(c.r), g: t(c.g), b: t(c.b), a: c.a };
            }
            "shade" => {
                let t = |x: f32| from_lin(to_lin(x) * v);
                c = Rgba { r: t(c.r), g: t(c.g), b: t(c.b), a: c.a };
            }
            "comp" => {
                let (h, s, l) = c.to_hsl();
                c = Rgba::from_hsl(h + 180.0, s, l, c.a);
            }
            "inv" => c = Rgba { r: 1.0 - c.r, g: 1.0 - c.g, b: 1.0 - c.b, a: c.a },
            "gray" => {
                let y = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
                c = Rgba { r: y, g: y, b: y, a: c.a };
            }
            "redMod" => c.r *= v,
            "greenMod" => c.g *= v,
            "blueMod" => c.b *= v,
            "redOff" => c.r += v,
            "greenOff" => c.g += v,
            "blueOff" => c.b += v,
            "red" => c.r = v,
            "green" => c.g = v,
            "blue" => c.b = v,
            _ => {}
        }
    }
    Rgba { r: c.r.clamp(0.0, 1.0), g: c.g.clamp(0.0, 1.0), b: c.b.clamp(0.0, 1.0), a: c.a.clamp(0.0, 1.0) }
}

fn system_color(name: &str) -> Option<Rgba> {
    Some(match name {
        "window" | "highlightText" | "btnHighlight" | "3dLight" => Rgba::WHITE,
        "windowText" | "btnText" | "menuText" | "captionText" | "infoText" | "3dDkShadow" => Rgba::BLACK,
        "btnFace" | "menu" | "scrollBar" => Rgba::from_u8(0xF0, 0xF0, 0xF0),
        "btnShadow" | "grayText" => Rgba::from_u8(0xA0, 0xA0, 0xA0),
        "highlight" | "hotLight" => Rgba::from_u8(0x00, 0x78, 0xD7),
        _ => return None,
    })
}

fn preset_color(name: &str) -> Option<Rgba> {
    let hex = match name {
        "black" => "000000",
        "white" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "lime" => "00FF00",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "cyan" | "aqua" => "00FFFF",
        "magenta" | "fuchsia" => "FF00FF",
        "gray" | "grey" => "808080",
        "silver" => "C0C0C0",
        "ltGray" | "lightGray" => "D3D3D3",
        "dkGray" | "darkGray" => "A9A9A9",
        "maroon" => "800000",
        "navy" => "000080",
        "olive" => "808000",
        "purple" => "800080",
        "teal" => "008080",
        "orange" => "FFA500",
        "pink" => "FFC0CB",
        "brown" => "A52A2A",
        "gold" => "FFD700",
        "darkBlue" | "dkBlue" => "00008B",
        "darkRed" | "dkRed" => "8B0000",
        "darkGreen" | "dkGreen" => "006400",
        "lightBlue" | "ltBlue" => "ADD8E6",
        "skyBlue" => "87CEEB",
        "orangeRed" => "FF4500",
        "violet" => "EE82EE",
        "indigo" => "4B0082",
        _ => return None,
    };
    Rgba::from_hex(hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    fn ctx_color(xml: &str) -> Rgba {
        let mut scheme = HashMap::new();
        scheme.insert("accent1".to_string(), Rgba::from_hex("4472C4").unwrap());
        scheme.insert("dk1".to_string(), Rgba::BLACK);
        scheme.insert("lt1".to_string(), Rgba::WHITE);
        let map = ClrMap::default();
        let ctx = ColorCtx { scheme: &scheme, map: &map, ph_clr: None };
        let el = parse("t", xml.as_bytes()).unwrap();
        parse_color(&el, &ctx).unwrap()
    }

    fn hex(c: Rgba) -> String {
        format!("{:02X}{:02X}{:02X}", (c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8)
    }

    #[test]
    fn scheme_and_mods() {
        assert_eq!(hex(ctx_color(r#"<schemeClr val="tx1"/>"#)), "000000");
        assert_eq!(hex(ctx_color(r#"<schemeClr val="bg1"/>"#)), "FFFFFF");
        // « Blanc, arrière-plan 1, plus sombre 15 % ».
        assert_eq!(hex(ctx_color(r#"<schemeClr val="bg1"><lumMod val="85000"/></schemeClr>"#)), "D9D9D9");
        // « Accent 1, plus clair 40 % ».
        assert_eq!(hex(ctx_color(r#"<schemeClr val="accent1"><lumMod val="60000"/><lumOff val="40000"/></schemeClr>"#)), "8FAADC");
        let c = ctx_color(r#"<srgbClr val="FF0000"><alpha val="50000"/></srgbClr>"#);
        assert!((c.a - 0.5).abs() < 1e-3);
        assert_eq!(hex(ctx_color(r#"<sysClr val="windowText" lastClr="000000"/>"#)), "000000");
    }
}
