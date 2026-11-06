//! Ouverture d'une présentation : ordre et taille des diapositives, titres, notes, rendu.

use crate::error::{Error, Result};
use crate::fonts::{self, Fonts};
use crate::package::{Package, ReadSeek};
use crate::render;
use crate::xml::Elem;
use image::RgbaImage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Taille par défaut d'une diapositive 4:3 (10 × 7,5 pouces).
const DEFAULT_SIZE: (i64, i64) = (9_144_000, 6_858_000);
/// Plus grand côté accepté pour un rendu.
const MAX_SIDE: u32 = 16_384;
/// Mémoire maximale des images décodées gardées en cache.
const IMAGE_CACHE_BYTES: usize = 384 * 1024 * 1024;

/// Images décodées par partie, et leur taille totale en octets.
type ImageCache = (HashMap<String, Option<Arc<RgbaImage>>>, usize);

pub(crate) struct SlideRef {
    pub part: String,
    pub hidden: bool,
}

/// Une présentation PowerPoint ouverte.
///
/// Le rendu peut se faire depuis n'importe quel fil d'exécution (`Presentation` est `Send + Sync`).
pub struct Presentation {
    pub(crate) pkg: Package,
    pub(crate) pres_part: String,
    pub(crate) slides: Vec<SlideRef>,
    size: (i64, i64),
    images: Mutex<ImageCache>,
    fonts: Mutex<Option<Arc<Mutex<Fonts>>>>,
}

impl Presentation {
    /// Ouvre un fichier .pptx (ou .ppsx, .potx…).
    pub fn open(path: impl AsRef<Path>) -> Result<Presentation> {
        let file = std::fs::File::open(path.as_ref())?;
        Self::from_reader(Box::new(std::io::BufReader::new(file)))
    }

    /// Ouvre une présentation déjà en mémoire.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Presentation> {
        Self::from_reader(Box::new(std::io::Cursor::new(bytes)))
    }

    fn from_reader(reader: Box<dyn ReadSeek>) -> Result<Presentation> {
        let pkg = Package::new(reader)?;
        // Partie principale : relation officeDocument de /_rels/.rels, sinon l'emplacement habituel.
        let pres_part = pkg
            .rels("")
            .iter()
            .find(|r| r.kind == "officeDocument")
            .map(|r| r.target.clone())
            .filter(|p| pkg.exists(p))
            .unwrap_or_else(|| "ppt/presentation.xml".into());
        if !pkg.exists(&pres_part) {
            return Err(Error::NotAPresentation("ppt/presentation.xml introuvable".into()));
        }
        let root = pkg.xml(&pres_part)?;
        if root.name != "presentation" {
            return Err(Error::NotAPresentation(format!("élément racine « {} »", root.name)));
        }
        let size = root
            .child("sldSz")
            .and_then(|s| Some((s.attr_i64("cx")?, s.attr_i64("cy")?)))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or(DEFAULT_SIZE);
        let rels = pkg.rels(&pres_part);
        let mut slides = Vec::new();
        if let Some(list) = root.child("sldIdLst") {
            for id in list.children("sldId") {
                let Some(rid) = id.attr("r:id") else { continue };
                let Some(rel) = rels.iter().find(|r| r.id == rid && !r.external) else { continue };
                if !pkg.exists(&rel.target) {
                    continue;
                }
                let hidden = pkg.xml(&rel.target).map(|s| s.attr("show").map(|v| v == "0" || v == "false").unwrap_or(false)).unwrap_or(false);
                slides.push(SlideRef { part: rel.target.clone(), hidden });
            }
        }
        Ok(Presentation { pkg, pres_part, slides, size, images: Mutex::default(), fonts: Mutex::new(None) })
    }

    /// Nombre de diapositives (masquées comprises).
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// Taille des diapositives en EMU (914 400 EMU par pouce).
    pub fn slide_size_emu(&self) -> (i64, i64) {
        self.size
    }

    /// Largeur / hauteur des diapositives.
    pub fn aspect_ratio(&self) -> f64 {
        self.size.0 as f64 / self.size.1 as f64
    }

    /// Plus grande taille en pixels aux proportions de la diapositive qui tient dans `max_w × max_h`.
    pub fn fit_size(&self, max_w: u32, max_h: u32) -> (u32, u32) {
        let ar = self.aspect_ratio();
        let (mw, mh) = (max_w.max(1) as f64, max_h.max(1) as f64);
        if mw / mh > ar {
            (((mh * ar).round() as u32).max(1), mh as u32)
        } else {
            (mw as u32, ((mw / ar).round() as u32).max(1))
        }
    }

    /// Diapositive masquée dans PowerPoint (`show="0"`) : le diaporama la saute.
    pub fn is_hidden(&self, index: usize) -> bool {
        self.slides.get(index).map(|s| s.hidden).unwrap_or(false)
    }

    /// Première diapositive non masquée après `from`.
    pub fn next_visible(&self, from: usize) -> Option<usize> {
        (from + 1..self.slides.len()).find(|&i| !self.slides[i].hidden)
    }

    /// Dernière diapositive non masquée avant `from`.
    pub fn prev_visible(&self, from: usize) -> Option<usize> {
        (0..from.min(self.slides.len())).rev().find(|&i| !self.slides[i].hidden)
    }

    /// Titre de la diapositive (texte de l'espace réservé titre), s'il y en a un.
    pub fn slide_title(&self, index: usize) -> Option<String> {
        let slide = self.pkg.xml(&self.slides.get(index)?.part).ok()?;
        let tree = slide.path(&["cSld", "spTree"])?;
        let mut shapes = Vec::new();
        tree.find_all("sp", &mut shapes);
        let title = shapes.into_iter().find(|sp| {
            let ty = sp.path(&["nvSpPr", "nvPr", "ph"]).map(|ph| ph.attr("type").unwrap_or("obj"));
            matches!(ty, Some("title" | "ctrTitle"))
        })?;
        let text = body_text(title.child("txBody")?, " ");
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then_some(text)
    }

    /// Notes du présentateur de la diapositive.
    pub fn notes(&self, index: usize) -> Option<String> {
        let part = &self.slides.get(index)?.part;
        let rel = self.pkg.rels(part).iter().find(|r| r.kind == "notesSlide")?.clone();
        let notes = self.pkg.xml(&rel.target).ok()?;
        let tree = notes.path(&["cSld", "spTree"])?;
        let mut shapes = Vec::new();
        tree.find_all("sp", &mut shapes);
        let body = shapes.into_iter().find(|sp| sp.path(&["nvSpPr", "nvPr", "ph"]).and_then(|ph| ph.attr("type")) == Some("body"))?;
        let text = body_text(body.child("txBody")?, "\n");
        let text = text.trim().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// Dessine la diapositive `index` (0 = première) à la plus grande taille qui tient dans
    /// `max_w × max_h` en gardant ses proportions (voir [`fit_size`](Self::fit_size)).
    pub fn render_slide(&self, index: usize, max_w: u32, max_h: u32) -> Result<RgbaImage> {
        if max_w == 0 || max_h == 0 || max_w > MAX_SIDE || max_h > MAX_SIDE {
            return Err(Error::InvalidSize { width: max_w, height: max_h });
        }
        if index >= self.slides.len() {
            return Err(Error::NoSuchSlide { index, count: self.slides.len() });
        }
        let (w, h) = self.fit_size(max_w, max_h);
        let fonts = self.fonts.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(fonts::shared).clone();
        let mut fonts = fonts.lock().unwrap_or_else(|e| e.into_inner());
        render::render_slide(self, index, w, h, &mut fonts)
    }

    /// Image de l'archive décodée (None : format non pris en charge, EMF/WMF…). Gardée en cache.
    pub(crate) fn image(&self, part: &str) -> Option<Arc<RgbaImage>> {
        if let Some(i) = self.images.lock().unwrap_or_else(|e| e.into_inner()).0.get(part) {
            return i.clone();
        }
        let img = self.pkg.read(part).ok().and_then(decode_image).map(|i| Arc::new(i.into_rgba8()));
        let mut cache = self.images.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = img.as_ref().map(|i| i.as_raw().len()).unwrap_or(0);
        if cache.1 + bytes > IMAGE_CACHE_BYTES {
            cache.0.clear();
            cache.1 = 0;
        }
        cache.1 += bytes;
        cache.0.insert(part.to_string(), img.clone());
        img
    }
}

/// Décode une image ; un PNG tronqué (sans bloc IEND, ce qui arrive) est complété.
fn decode_image(mut bytes: Vec<u8>) -> Option<image::DynamicImage> {
    match image::load_from_memory(&bytes) {
        Ok(i) => Some(i),
        Err(_) if bytes.starts_with(b"\x89PNG") && !bytes.ends_with(b"IEND\xAE\x42\x60\x82") => {
            bytes.extend_from_slice(b"\0\0\0\0IEND\xAE\x42\x60\x82");
            image::load_from_memory(&bytes).ok()
        }
        Err(_) => None,
    }
}

/// Texte brut d'un `txBody` (paragraphes séparés par `sep`).
fn body_text(body: &Elem, sep: &str) -> String {
    let mut paras = Vec::new();
    for p in body.children("p") {
        let mut s = String::new();
        for c in &p.children {
            match c.name.as_str() {
                "r" | "fld" => s.push_str(c.child("t").map(|t| t.text.as_str()).unwrap_or("")),
                "br" => s.push(' '),
                _ => {}
            }
        }
        paras.push(s);
    }
    paras.join(sep)
}
