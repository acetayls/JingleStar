//! Langue de l'interface (français ou anglais), choisie dans les préférences.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Fr,
    En,
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn is_en() -> bool {
    LANG.load(Ordering::Relaxed) == Lang::En as u8
}

/// Renvoie le texte dans la langue de l'interface.
pub fn tr(fr: &'static str, en: &'static str) -> &'static str {
    if is_en() {
        en
    } else {
        fr
    }
}
