//! Paquet OPC : l'archive zip, ses parties et leurs relations (`_rels/*.rels`).

use crate::error::{Error, Result};
use crate::xml::{self, Elem};
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::sync::{Arc, Mutex};

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// Taille maximale d'une partie lue en mémoire (garde-fou contre les archives piégées).
const MAX_PART: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Rel {
    pub id: String,
    /// Dernier segment du type (`slide`, `slideLayout`, `image`…).
    pub kind: String,
    /// Nom de partie absolu dans l'archive (sans « / » initial), ou URL si externe.
    pub target: String,
    pub external: bool,
}

pub struct Package {
    zip: Mutex<zip::ZipArchive<Box<dyn ReadSeek>>>,
    /// Nom en minuscules → nom réel (les noms de parties OPC ne tiennent pas compte de la casse).
    names: HashMap<String, String>,
    xml_cache: Mutex<HashMap<String, Arc<Elem>>>,
    rels_cache: Mutex<HashMap<String, Arc<Vec<Rel>>>>,
}

impl Package {
    pub fn new(reader: Box<dyn ReadSeek>) -> Result<Self> {
        let zip = zip::ZipArchive::new(reader)?;
        let names = zip.file_names().filter_map(|n| n.ok()).map(|n| (n.to_lowercase(), n.to_string())).collect();
        Ok(Self { zip: Mutex::new(zip), names, xml_cache: Mutex::default(), rels_cache: Mutex::default() })
    }

    pub fn exists(&self, part: &str) -> bool {
        self.names.contains_key(&part.to_lowercase())
    }

    pub fn read(&self, part: &str) -> Result<Vec<u8>> {
        let real = self.names.get(&part.to_lowercase()).ok_or_else(|| Error::MissingPart(part.to_string()))?;
        let mut zip = self.zip.lock().unwrap_or_else(|e| e.into_inner());
        let file = zip.by_name(real)?;
        let mut out = Vec::with_capacity(file.size().min(MAX_PART) as usize);
        file.take(MAX_PART).read_to_end(&mut out)?;
        Ok(out)
    }

    /// Partie XML analysée (gardée en cache : masques et dispositions servent à plusieurs diapositives).
    pub fn xml(&self, part: &str) -> Result<Arc<Elem>> {
        if let Some(e) = self.xml_cache.lock().unwrap_or_else(|e| e.into_inner()).get(part) {
            return Ok(e.clone());
        }
        let el = Arc::new(xml::parse(part, &self.read(part)?)?);
        self.xml_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(part.to_string(), el.clone());
        Ok(el)
    }

    /// Relations d'une partie (vide si elle n'en a pas).
    pub fn rels(&self, part: &str) -> Arc<Vec<Rel>> {
        if let Some(r) = self.rels_cache.lock().unwrap_or_else(|e| e.into_inner()).get(part) {
            return r.clone();
        }
        let rels = Arc::new(self.load_rels(part));
        self.rels_cache.lock().unwrap_or_else(|e| e.into_inner()).insert(part.to_string(), rels.clone());
        rels
    }

    fn load_rels(&self, part: &str) -> Vec<Rel> {
        let (dir, file) = match part.rfind('/') {
            Some(i) => (&part[..i + 1], &part[i + 1..]),
            None => ("", part),
        };
        let rels_part = format!("{dir}_rels/{file}.rels");
        let Ok(bytes) = self.read(&rels_part) else { return Vec::new() };
        let Ok(root) = xml::parse(&rels_part, &bytes) else { return Vec::new() };
        root.children("Relationship")
            .filter_map(|r| {
                let id = r.attr("Id")?.to_string();
                let kind = r.attr("Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
                let target = r.attr("Target")?;
                let external = r.attr("TargetMode").map(|m| m.eq_ignore_ascii_case("External")).unwrap_or(false);
                let target = if external { target.to_string() } else { resolve(part, target) };
                Some(Rel { id, kind, target, external })
            })
            .collect()
    }
}

/// Résout une cible de relation relative à la partie source.
pub fn resolve(source: &str, target: &str) -> String {
    let target = percent_decode(target);
    let target = target.split('#').next().unwrap_or("");
    let mut parts: Vec<&str> = if let Some(abs) = target.strip_prefix('/') {
        return normalize(abs.split('/').collect());
    } else {
        source.split('/').collect()
    };
    parts.pop(); // nom de la partie source
    parts.extend(target.split('/'));
    normalize(parts)
}

fn normalize(parts: Vec<&str>) -> String {
    let mut out: Vec<&str> = Vec::new();
    for p in parts {
        match p {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve("ppt/slides/slide1.xml", "../slideLayouts/slideLayout2.xml"), "ppt/slideLayouts/slideLayout2.xml");
        assert_eq!(resolve("ppt/presentation.xml", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides/slide1.xml", "/ppt/media/image%201.png"), "ppt/media/image 1.png");
        assert_eq!(resolve("ppt/slides/slide1.xml", "../media/a.png#x"), "ppt/media/a.png");
        assert_eq!(percent_decode("a%2"), "a%2");
    }
}
