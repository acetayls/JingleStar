//! Thème : palette, polices du thème, styles de remplissage et de trait.

use crate::color::{child_color, ClrMap, ColorCtx, Rgba};
use crate::xml::Elem;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Theme {
    pub colors: HashMap<String, Rgba>,
    pub major_latin: Option<String>,
    pub minor_latin: Option<String>,
    pub major_ea: Option<String>,
    pub minor_ea: Option<String>,
    pub fill_styles: Vec<Elem>,
    pub line_styles: Vec<Elem>,
    pub bg_fill_styles: Vec<Elem>,
    pub effect_styles: Vec<Elem>,
}

impl Theme {
    pub fn from_elem(root: &Elem) -> Theme {
        let mut t = Theme::default();
        let Some(elems) = root.child("themeElements") else { return t };
        if let Some(cs) = elems.child("clrScheme") {
            let empty_map = ClrMap::default();
            let empty = HashMap::new();
            let ctx = ColorCtx { scheme: &empty, map: &empty_map, ph_clr: None };
            for c in &cs.children {
                if let Some(col) = child_color(c, &ctx) {
                    t.colors.insert(c.name.clone(), col);
                }
            }
        }
        if let Some(fs) = elems.child("fontScheme") {
            let face = |font: &str, script: &str| {
                fs.path(&[font, script]).and_then(|l| l.attr("typeface")).filter(|s| !s.is_empty()).map(str::to_string)
            };
            t.major_latin = face("majorFont", "latin");
            t.minor_latin = face("minorFont", "latin");
            t.major_ea = face("majorFont", "ea");
            t.minor_ea = face("minorFont", "ea");
        }
        if let Some(fmt) = elems.child("fmtScheme") {
            let list = |n: &str| fmt.child(n).map(|l| l.children.clone()).unwrap_or_default();
            t.fill_styles = list("fillStyleLst");
            t.line_styles = list("lnStyleLst");
            t.bg_fill_styles = list("bgFillStyleLst");
            t.effect_styles = list("effectStyleLst");
        }
        t
    }

    /// Remplace `+mj-lt`, `+mn-lt`… par la police du thème.
    pub fn resolve_font(&self, face: &str) -> Option<String> {
        match face {
            "+mj-lt" => self.major_latin.clone(),
            "+mn-lt" => self.minor_latin.clone(),
            "+mj-ea" => self.major_ea.clone().or_else(|| self.major_latin.clone()),
            "+mn-ea" => self.minor_ea.clone().or_else(|| self.minor_latin.clone()),
            "+mj-cs" => self.major_latin.clone(),
            "+mn-cs" => self.minor_latin.clone(),
            "" => None,
            f => Some(f.to_string()),
        }
    }
}
