//! Réglages propres à une machine (JSON dans le dossier de configuration), gardés à part du
//! projet : ouvrir un projet venu d'un autre ordinateur ne change ni la carte son, ni l'écran
//! de sortie, ni les télécommandes.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Lit `path` ; `None` s'il manque ou ne se lit pas.
pub fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Enregistre `value` dans `path` de façon sûre (fichier temporaire forcé sur le disque, puis
/// renommé) : une coupure de courant ne laisse pas un fichier vide.
pub fn save<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    crate::document::write_atomic(path, serde_json::to_string_pretty(value)?.as_bytes(), false)
}
