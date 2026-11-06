//! Arbre XML simplifié et autonome (sans durée de vie), construit avec roxmltree.
//!
//! Les éléments sont désignés par leur nom local (`sp`, `txBody`…) : les préfixes `p:`, `a:`,
//! `dsp:` désignent les mêmes notions dans les différentes parties. Les attributs de l'espace de
//! noms des relations gardent le préfixe `r:` (`r:id`, `r:embed`), car `p:sldId` porte à la fois
//! `id` et `r:id`.

use crate::error::{Error, Result};

const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[derive(Debug, Clone, Default)]
pub struct Elem {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Elem>,
    /// Texte direct de l'élément (utile pour `a:t`).
    pub text: String,
}

impl Elem {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn attr_i64(&self, name: &str) -> Option<i64> {
        let v = self.attr(name)?.trim();
        v.parse::<i64>().ok().or_else(|| v.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| f as i64))
    }

    pub fn attr_f64(&self, name: &str) -> Option<f64> {
        let v = self.attr(name)?.trim();
        // Certaines valeurs portent un suffixe « % » (fichiers non Microsoft).
        let v = v.trim_end_matches('%');
        v.parse::<f64>().ok().filter(|f| f.is_finite())
    }

    /// Attribut booléen XML (« 1 », « true », « on »).
    pub fn attr_bool(&self, name: &str) -> Option<bool> {
        self.attr(name).map(|v| matches!(v.trim(), "1" | "true" | "on"))
    }

    pub fn child(&self, name: &str) -> Option<&Elem> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Elem> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Suit un chemin d'enfants : `el.path(&["spPr", "xfrm"])`.
    pub fn path(&self, names: &[&str]) -> Option<&Elem> {
        let mut cur = self;
        for n in names {
            cur = cur.child(n)?;
        }
        Some(cur)
    }

    /// Premier descendant (en profondeur) portant ce nom.
    pub fn find(&self, name: &str) -> Option<&Elem> {
        for c in &self.children {
            if c.name == name {
                return Some(c);
            }
            if let Some(f) = c.find(name) {
                return Some(f);
            }
        }
        None
    }

    /// Tous les descendants portant ce nom (ordre du document).
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Elem>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.find_all(name, out);
        }
    }
}

/// Décode une partie XML (UTF-8 avec ou sans BOM, ou UTF-16).
pub fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    let utf16 = |le: bool| {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16(true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return utf16(false);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn parse(part: &str, bytes: &[u8]) -> Result<Elem> {
    let text = decode(bytes);
    // La déclaration XML peut annoncer UTF-16 alors que le texte est déjà décodé : on la retire.
    let body = if text.trim_start().starts_with("<?xml") {
        match text.find("?>") {
            Some(end) => &text[end + 2..],
            None => &text[..],
        }
    } else {
        &text[..]
    };
    let opts = roxmltree::ParsingOptions { allow_dtd: false, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(body, opts).map_err(|e| Error::Xml { part: part.to_string(), message: e.to_string() })?;
    Ok(convert(doc.root_element()))
}

fn convert(node: roxmltree::Node) -> Elem {
    let mut el = Elem { name: node.tag_name().name().to_string(), ..Default::default() };
    for a in node.attributes() {
        let key = if a.namespace() == Some(REL_NS) { format!("r:{}", a.name()) } else { a.name().to_string() };
        el.attrs.push((key, a.value().to_string()));
    }
    for c in node.children() {
        if c.is_element() {
            el.children.push(convert(c));
        } else if c.is_text() {
            if let Some(t) = c.text() {
                el.text.push_str(t);
            }
        }
    }
    el
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaces_and_rel_attrs() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldIdLst xmlns:p="urn:p" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:sldId id="256" r:id="rId2"/><a:t xmlns:a="urn:a"> x </a:t>
</p:sldIdLst>"#;
        let el = parse("t", xml).unwrap();
        assert_eq!(el.name, "sldIdLst");
        let s = el.child("sldId").unwrap();
        assert_eq!(s.attr("id"), Some("256"));
        assert_eq!(s.attr("r:id"), Some("rId2"));
        assert_eq!(el.child("t").unwrap().text, " x ");
    }

    #[test]
    fn bad_xml_is_an_error() {
        assert!(parse("t", b"<a><b></a>").is_err());
    }
}
