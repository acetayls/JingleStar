//! Polices : base des polices du système (chargée une fois), substitution des polices Office
//! absentes par des équivalents métriques, contours des glyphes.

use cosmic_text::{fontdb, FontSystem};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use swash::scale::ScaleContext;
use swash::zeno::{Angle, Command, Transform as ZTransform};
use tiny_skia::{Path, PathBuilder};

pub struct Fonts {
    pub system: FontSystem,
    /// Familles présentes : nom en minuscules → nom exact.
    families: HashMap<String, String>,
    resolved: HashMap<String, String>,
    line_ratio: HashMap<(String, bool), (f32, f32)>,
    scale: ScaleContext,
    outlines: HashMap<(fontdb::ID, u16, u32, u8), Option<Path>>,
}

static FONTS: OnceLock<Arc<Mutex<Fonts>>> = OnceLock::new();

/// Base de polices partagée (le chargement des polices du système prend un moment).
pub fn shared() -> Arc<Mutex<Fonts>> {
    FONTS.get_or_init(|| Arc::new(Mutex::new(Fonts::new()))).clone()
}

/// Polices métriquement compatibles, ou proches, pour les polices Office courantes.
const SUBSTITUTES: &[(&str, &[&str])] = &[
    ("calibri", &["Carlito"]),
    ("calibri light", &["Calibri", "Carlito"]),
    ("cambria", &["Caladea", "Liberation Serif"]),
    ("arial", &["Liberation Sans", "Arimo", "Helvetica"]),
    ("helvetica", &["Arial", "Liberation Sans"]),
    ("arial narrow", &["Liberation Sans Narrow", "Arial", "Liberation Sans"]),
    ("times new roman", &["Liberation Serif", "Tinos", "Times"]),
    ("times", &["Times New Roman", "Liberation Serif"]),
    ("courier new", &["Liberation Mono", "Cousine", "Courier"]),
    ("consolas", &["Liberation Mono", "DejaVu Sans Mono"]),
    ("segoe ui", &["Selawik", "Open Sans", "Noto Sans", "DejaVu Sans"]),
    ("segoe ui light", &["Segoe UI", "Open Sans", "Noto Sans"]),
    ("segoe ui semibold", &["Segoe UI", "Open Sans", "Noto Sans"]),
    ("aptos", &["Aptos Display", "Segoe UI", "Calibri", "Carlito"]),
    ("aptos display", &["Aptos", "Segoe UI", "Calibri", "Carlito"]),
    ("verdana", &["DejaVu Sans"]),
    ("tahoma", &["DejaVu Sans"]),
    ("georgia", &["Gelasio", "DejaVu Serif", "Liberation Serif"]),
    ("garamond", &["EB Garamond", "Liberation Serif"]),
    ("century gothic", &["URW Gothic", "TeX Gyre Adventor", "Liberation Sans"]),
    ("gill sans mt", &["Gillius ADF", "Liberation Sans"]),
    ("trebuchet ms", &["DejaVu Sans", "Liberation Sans"]),
    ("franklin gothic book", &["Libre Franklin", "Liberation Sans"]),
    ("comic sans ms", &["Comic Neue", "Liberation Sans"]),
    ("impact", &["Anton", "Liberation Sans"]),
    ("symbol", &["OpenSymbol", "DejaVu Sans"]),
    ("lato", &["Carlito"]),
    ("noto sans", &["Open Sans", "Liberation Sans", "DejaVu Sans"]),
    ("open sans", &["Noto Sans", "Liberation Sans", "DejaVu Sans"]),
    ("roboto", &["Noto Sans", "Liberation Sans", "DejaVu Sans"]),
    ("source sans pro", &["Noto Sans", "Liberation Sans"]),
    ("montserrat", &["Noto Sans", "Liberation Sans", "DejaVu Sans"]),
    ("dejavu sans", &["Verdana", "Liberation Sans"]),
];

/// Repli quand rien ne correspond.
const SANS: &[&str] = &["Calibri", "Carlito", "Arial", "Liberation Sans", "Segoe UI", "Helvetica", "DejaVu Sans", "Noto Sans"];
const SERIF: &[&str] = &["Cambria", "Caladea", "Times New Roman", "Liberation Serif", "DejaVu Serif", "Noto Serif"];

impl Fonts {
    fn new() -> Fonts {
        let system = FontSystem::new();
        let mut families = HashMap::new();
        for face in system.db().faces() {
            for (name, _) in &face.families {
                families.entry(name.to_lowercase()).or_insert_with(|| name.clone());
            }
        }
        Fonts { system, families, resolved: HashMap::new(), line_ratio: HashMap::new(), scale: ScaleContext::new(), outlines: HashMap::new() }
    }

    fn has(&self, name: &str) -> Option<&String> {
        self.families.get(&name.to_lowercase())
    }

    /// Famille installée à utiliser pour `name`.
    pub fn resolve(&mut self, name: &str) -> String {
        if let Some(r) = self.resolved.get(name) {
            return r.clone();
        }
        let r = self.find(name);
        self.resolved.insert(name.to_string(), r.clone());
        r
    }

    fn find(&self, name: &str) -> String {
        if let Some(f) = self.has(name) {
            return f.clone();
        }
        let low = name.to_lowercase();
        if let Some((_, subs)) = SUBSTITUTES.iter().find(|(k, _)| *k == low) {
            for s in *subs {
                if let Some(f) = self.has(s) {
                    return f.clone();
                }
            }
        }
        // « Montserrat Light » → « Montserrat » ; « Calibri Light » → « Calibri ».
        let mut words: Vec<&str> = name.split_whitespace().collect();
        while words.len() > 1 {
            words.pop();
            let shorter = words.join(" ");
            if let Some(f) = self.has(&shorter) {
                return f.clone();
            }
        }
        let serif = low.contains("serif") && !low.contains("sans") || ["times", "cambria", "georgia", "garamond", "roman", "book antiqua", "palatino"].iter().any(|k| low.contains(k));
        for s in if serif { SERIF } else { SANS }.iter().chain(SANS) {
            if let Some(f) = self.has(s) {
                return f.clone();
            }
        }
        self.families.values().min().cloned().unwrap_or_else(|| "sans-serif".into())
    }

    /// (interligne simple, hauteur au-dessus de la ligne de base) en fraction de la taille,
    /// d'après les métriques de la police.
    pub fn line_metrics(&mut self, family: &str, bold: bool) -> (f32, f32) {
        let key = (family.to_string(), bold);
        if let Some(r) = self.line_ratio.get(&key) {
            return *r;
        }
        let q = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
            ..Default::default()
        };
        let r = self
            .system
            .db()
            .query(&q)
            .and_then(|id| self.system.get_font(id, q.weight))
            .map(|f| {
                let m = f.as_swash().metrics(&[]);
                let upem = m.units_per_em.max(1) as f32;
                let line = (m.ascent + m.descent + m.leading) / upem;
                (line.clamp(0.8, 2.5), (m.ascent / upem).clamp(0.5, 2.0))
            })
            .unwrap_or((1.2, 0.9));
        self.line_ratio.insert(key, r);
        r
    }

    /// Contour d'un glyphe à la taille `size`, origine sur la ligne de base, y vers le bas.
    pub fn glyph_path(&mut self, id: fontdb::ID, weight: fontdb::Weight, glyph: u16, size: f32, italic: bool, bold: bool) -> Option<Path> {
        let flags = italic as u8 | (bold as u8) << 1;
        let key = (id, glyph, size.to_bits(), flags);
        if let Some(p) = self.outlines.get(&key) {
            return p.clone();
        }
        let p = self.make_glyph_path(id, weight, glyph, size, italic, bold);
        if self.outlines.len() > 20_000 {
            self.outlines.clear();
        }
        self.outlines.insert(key, p.clone());
        p
    }

    fn make_glyph_path(&mut self, id: fontdb::ID, weight: fontdb::Weight, glyph: u16, size: f32, italic: bool, bold: bool) -> Option<Path> {
        let font = self.system.get_font(id, weight)?;
        let face_weight = self.system.db().face(id).map(|f| f.weight.0).unwrap_or(400);
        let sw = font.as_swash();
        let wght = sw.variations().find_by_tag(swash::Tag::from_be_bytes(*b"wght"));
        let mut builder = self.scale.builder(sw).size(size).hint(false);
        let coords;
        if let Some(v) = wght {
            coords = sw.variations().normalized_coords([(swash::Tag::from_be_bytes(*b"wght"), (weight.0 as f32).clamp(v.min_value(), v.max_value()))]).collect::<Vec<_>>();
            builder = builder.normalized_coords(coords.iter());
        }
        let mut scaler = builder.build();
        let mut outline = scaler.scale_outline(glyph).or_else(|| scaler.scale_color_outline(glyph))?;
        if italic {
            outline.transform(&ZTransform::skew(Angle::from_degrees(12.0), Angle::from_degrees(0.0)));
        }
        // Gras simulé quand la famille n'a pas de graisse grasse.
        if bold && wght.is_none() && face_weight < 600 {
            let s = size * 0.025;
            outline.embolden(s, s);
        }
        let mut pb = PathBuilder::new();
        use swash::zeno::PathData;
        for c in outline.path().commands() {
            match c {
                Command::MoveTo(p) => pb.move_to(p.x, -p.y),
                Command::LineTo(p) => pb.line_to(p.x, -p.y),
                Command::CurveTo(a, b, p) => pb.cubic_to(a.x, -a.y, b.x, -b.y, p.x, -p.y),
                Command::QuadTo(a, p) => pb.quad_to(a.x, -a.y, p.x, -p.y),
                Command::Close => pb.close(),
            }
        }
        pb.finish()
    }
}

/// Caractères des polices de symboles (Wingdings, Symbol) remplacés par leur équivalent Unicode.
pub fn map_symbol_char(font: Option<&str>, s: &str) -> String {
    let font = font.unwrap_or("").to_lowercase();
    let wing = font.starts_with("wingdings");
    let symbol = font == "symbol";
    if !wing && !symbol {
        return s.to_string();
    }
    s.chars()
        .map(|c| {
            // Les polices de symboles utilisent parfois la zone privée F0xx.
            let c = if ('\u{F020}'..='\u{F0FF}').contains(&c) { char::from_u32(c as u32 - 0xF000).unwrap_or(c) } else { c };
            if wing {
                match c {
                    '§' => '▪',
                    'Ø' => '➢',
                    'ü' => '✓',
                    'û' => '✗',
                    'q' => '❑',
                    'v' => '❖',
                    'n' => '■',
                    'l' => '●',
                    'm' => '○',
                    'o' => '□',
                    'p' => '◻',
                    'Ÿ' => '•',
                    'à' | 'è' | 'ð' => '➔',
                    'Ü' => '➤',
                    'J' => '☺',
                    'w' => '◆',
                    'u' => '◆',
                    '¨' => '◻',
                    'þ' => '☑',
                    'ý' => '☒',
                    other => other,
                }
            } else {
                match c {
                    '·' => '•',
                    'Þ' => '⇒',
                    '®' => '→',
                    'ç' => '●',
                    other => other,
                }
            }
        })
        .collect()
}
