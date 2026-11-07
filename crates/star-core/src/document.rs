//! Fichier de projet (JSON) d'une appli Star : lecture tolérante, enregistrement sûr, médias
//! retrouvés après un déplacement. Repris de JingleStar ; chaque appli décrit son projet en
//! implémentant [`Document`].

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Projet d'une appli.
pub trait Document: Serialize + DeserializeOwned + Default + Clone {
    /// Après lecture : remet la structure en forme (nombre de modules…).
    fn normalize(&mut self) {}
    /// Remet dans leurs bornes les valeurs aberrantes (NaN, infini, hors limites).
    fn sanitize(&mut self) {}
    /// Chemins de tous les médias du projet (fichiers et dossiers).
    fn media_paths(&mut self) -> Vec<&mut PathBuf>;
}

/// Lit un projet (réparé au besoin, voir [`load_checked`]).
pub fn load<D: Document>(path: &Path) -> anyhow::Result<D> {
    load_checked(path).map(|(p, _)| p)
}

/// Lit un projet. Un fichier où des valeurs manquent (`null` écrit par une version précédente
/// pour un nombre invalide, projet modifié à la main) est réparé : ces valeurs reprennent leur
/// réglage par défaut, et le booléen le signale.
pub fn load_checked<D: Document>(path: &Path) -> anyhow::Result<(D, bool)> {
    let text = std::fs::read_to_string(path)?;
    let (mut p, repaired) = match serde_json::from_str::<D>(&text) {
        Ok(p) => (p, false),
        Err(e) => {
            let mut v: serde_json::Value = serde_json::from_str(&text)?;
            if !strip_nulls(&mut v) {
                return Err(e.into());
            }
            (serde_json::from_value(v).map_err(|_| e)?, true)
        }
    };
    p.normalize();
    p.sanitize();
    Ok((p, repaired))
}

/// Enregistre sur le disque de façon sûre : fichier temporaire propre à ce nom, écrit et
/// forcé sur le disque (une coupure de courant ne laisse pas un fichier vide), puis renommé.
/// Avec `backup`, la version précédente est gardée en `.bak`.
pub fn save<D: Document>(doc: &D, path: &Path, backup: bool) -> anyhow::Result<()> {
    // Jamais de `null` dans le fichier : il le rendrait illisible.
    let mut clean = doc.clone();
    clean.sanitize();
    write_atomic(path, serde_json::to_string_pretty(&clean)?.as_bytes(), backup)
}

/// Écrit `bytes` dans `path` par un fichier temporaire forcé sur le disque puis renommé ;
/// avec `backup`, l'ancien fichier devient `.bak`.
pub fn write_atomic(path: &Path, bytes: &[u8], backup: bool) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = with_suffix(path, ".tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if backup && path.exists() {
        std::fs::rename(path, with_suffix(path, ".bak"))?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Médias introuvables (sans doublon).
pub fn missing_files<D: Document>(doc: &mut D) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = doc.media_paths().into_iter().filter(|p| !p.exists()).map(|p| p.clone()).collect();
    v.sort();
    v.dedup();
    v
}

/// Projet copié ailleurs (autre ordinateur, autre disque) : un média introuvable à son
/// ancien chemin est cherché à la même place par rapport au fichier projet
/// (« …/Spectacle/sons/a.wav » → « <dossier du projet>/sons/a.wav »). Renvoie le nombre corrigé.
pub fn resolve_moved<D: Document>(doc: &mut D, project_file: &Path) -> usize {
    let Some(base) = project_file.parent() else { return 0 };
    let mut fixed = 0;
    for p in doc.media_paths() {
        if p.exists() {
            continue;
        }
        let parts: Vec<_> = p
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_os_string()),
                _ => None,
            })
            .collect();
        // Le plus long morceau de chemin d'abord : le plus précis.
        let found = (1..=parts.len().min(4))
            .rev()
            .map(|k| {
                let mut c = base.to_path_buf();
                c.extend(&parts[parts.len() - k..]);
                c
            })
            .find(|c| c.exists());
        if let Some(c) = found {
            *p = c;
            fixed += 1;
        }
    }
    fixed
}

/// Relocalisation : les médias introuvables sont cherchés par leur nom de fichier dans
/// `folder` et ses sous-dossiers. Renvoie le nombre de chemins corrigés.
pub fn relocate<D: Document>(doc: &mut D, folder: &Path) -> usize {
    let mut index: HashMap<OsString, PathBuf> = Default::default();
    index_files(folder, &mut index, 0);
    let mut fixed = 0;
    for p in doc.media_paths() {
        if p.exists() {
            continue;
        }
        let key = p.file_name().map(|n| n.to_ascii_lowercase());
        if let Some(found) = key.as_ref().and_then(|k| index.get(k)) {
            *p = found.clone();
            fixed += 1;
        } else if p.extension().is_none() {
            // Dossier (playlist, diaporama) : même nom de dossier.
            if let Some(d) = key.and_then(|k| find_dir(folder, &k, 0)) {
                *p = d;
                fixed += 1;
            }
        }
    }
    fixed
}

/// Fichiers d'un dossier et de ses sous-dossiers (8 niveaux au plus), par nom en minuscules.
fn index_files(dir: &Path, index: &mut HashMap<OsString, PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 8 {
                index_files(&p, index, depth + 1);
            }
        } else if let Some(n) = p.file_name() {
            index.entry(n.to_ascii_lowercase()).or_insert(p);
        }
    }
}

fn find_dir(dir: &Path, name: &OsStr, depth: usize) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().map(|n| n.to_ascii_lowercase()).as_deref() == Some(name) {
                return Some(p);
            }
            if depth < 8 {
                if let Some(f) = find_dir(&p, name, depth + 1) {
                    return Some(f);
                }
            }
        }
    }
    None
}

/// « session.json » → « session.json.tmp » : le suffixe s'ajoute au nom complet, deux projets
/// de même nom (a.json, a.jinglestar) n'ont donc pas le même fichier temporaire.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Retire les valeurs `null` des objets JSON (le réglage par défaut les remplace).
/// Les éléments `null` des tableaux (emplacements vides) restent. Renvoie vrai si
/// quelque chose a été retiré.
pub fn strip_nulls(v: &mut serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(map) => {
            let before = map.len();
            map.retain(|_, x| !x.is_null());
            let mut changed = map.len() != before;
            for x in map.values_mut() {
                changed |= strip_nulls(x);
            }
            changed
        }
        serde_json::Value::Array(a) => a.iter_mut().fold(false, |c, x| strip_nulls(x) | c),
        _ => false,
    }
}
