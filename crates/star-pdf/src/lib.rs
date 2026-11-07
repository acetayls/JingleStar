//! Rendu des pages d'un fichier PDF en images RGBA, pour le module Présentation de CueStar.
//!
//! Le rendu passe par hayro (Rust pur, sans DLL à livrer). Les polices standard du PDF
//! (Helvetica, Times…) sont incluses dans hayro. Les PDF chiffrés ne sont pas pris en charge.
//!
//! ```no_run
//! let doc = star_pdf::Document::open("diapos.pdf")?;
//! let img = doc.render_page(0, 1920, 1080)?;
//! img.save("page-1.png").ok();
//! # Ok::<(), star_pdf::Error>(())
//! ```

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use image::RgbaImage;
use std::path::Path;

/// Erreur d'ouverture ou de rendu, avec un message en français.
#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Un document PDF ouvert.
pub struct Document {
    pdf: Pdf,
}

/// Plus grand côté rendu, en pixels (limite de hayro : 65 535).
const MAX_SIDE: u32 = 8192;

impl Document {
    pub fn open(path: impl AsRef<Path>) -> Result<Document, Error> {
        let path = path.as_ref();
        let data = std::fs::read(path).map_err(|e| Error(format!("{} : {e}", path.display())))?;
        Self::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Document, Error> {
        let pdf = Pdf::new(data).map_err(|e| Error(format!("PDF illisible ({e:?})")))?;
        if pdf.pages().is_empty() {
            return Err(Error("ce PDF ne contient aucune page".into()));
        }
        Ok(Document { pdf })
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Taille de la page en points (1/72 de pouce), rotation comprise.
    pub fn page_size(&self, index: usize) -> Option<(f32, f32)> {
        self.pdf.pages().get(index).map(|p| p.render_dimensions())
    }

    /// Taille en pixels de la page `index` tenant dans une boîte `w × h`, proportions gardées.
    pub fn fit_size(&self, index: usize, w: u32, h: u32) -> Option<(u32, u32)> {
        let (pw, ph) = self.page_size(index)?;
        if !(pw > 0.0 && ph > 0.0) {
            return None;
        }
        let s = (w as f32 / pw).min(h as f32 / ph);
        let fw = ((pw * s).round() as u32).clamp(1, MAX_SIDE);
        let fh = ((ph * s).round() as u32).clamp(1, MAX_SIDE);
        Some((fw, fh))
    }

    /// Rend la page `index` (à partir de 0) dans une boîte `w × h`, sur fond blanc.
    pub fn render_page(&self, index: usize, w: u32, h: u32) -> Result<RgbaImage, Error> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or_else(|| Error(format!("page {} absente", index + 1)))?;
        let (fw, fh) = self.fit_size(index, w.max(1), h.max(1)).ok_or_else(|| Error(format!("page {} : taille invalide", index + 1)))?;
        let (pw, ph) = page.render_dimensions();
        let settings = PixmapSettings { x_scale: fw as f32 / pw, y_scale: fh as f32 / ph, bg_color: WHITE };
        let cache = RenderCache::new();
        let pixmap = hayro::render(page, &cache, &InterpreterSettings::default(), &RenderSettings::default(), &settings);
        let (pxw, pxh) = (pixmap.width() as u32, pixmap.height() as u32);
        // Fond blanc opaque : les données prémultipliées sont déjà les bonnes couleurs.
        RgbaImage::from_raw(pxw, pxh, pixmap.data_as_u8_slice().to_vec()).ok_or_else(|| Error(format!("page {} : image invalide", index + 1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Petit PDF d'une page A4 paysage : un rectangle rouge plein sur fond blanc.
    fn sample_pdf() -> Vec<u8> {
        let content = b"1 0 0 rg 100 100 200 150 re f";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 842 595] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    #[test]
    fn renders_a_page_in_its_box() {
        let doc = Document::from_bytes(sample_pdf()).unwrap();
        assert_eq!(doc.page_count(), 1);
        let img = doc.render_page(0, 1920, 1080).unwrap();
        // 842 × 595 dans 1920 × 1080 : la hauteur limite.
        assert_eq!(img.height(), 1080);
        assert!((img.width() as i32 - 1528).abs() <= 1, "largeur {}", img.width());
        // Coin haut gauche blanc ; le rectangle (100..300, 100..250 en points depuis le bas) est rouge.
        assert_eq!(img.get_pixel(5, 5).0, [255, 255, 255, 255]);
        let s = 1080.0 / 595.0;
        let (x, y) = ((200.0 * s) as u32, ((595.0 - 175.0) * s) as u32);
        assert_eq!(img.get_pixel(x, y).0, [255, 0, 0, 255]);
    }

    #[test]
    fn bad_files_are_errors() {
        assert!(Document::from_bytes(b"pas un pdf".to_vec()).is_err());
        assert!(Document::from_bytes(Vec::new()).is_err());
        assert!(Document::open("/fichier/absent.pdf").is_err());
    }
}
