//! Projet CueStar (fichier `.cuestar`, JSON), même mécanisme que JingleStar :
//! enregistrement sûr (fichier temporaire forcé sur le disque puis renommé), session
//! restaurée au lancement avec copie de secours, copies datées, projet déplacé retrouvé.

use crate::protocol::Fit;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "cuestar";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    /// Nom donné par l'utilisateur (sinon le nom du fichier).
    pub name: String,
    pub lecteur: LecteurData,
    pub diaporama: DiaporamaData,
    pub cuelist: CuelistData,
}

/// Suite à donner à la fin d'un cue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CueFollow {
    /// On attend le prochain GO.
    #[default]
    Stop,
    /// Le cue suivant part tout seul à la fin de celui-ci.
    Continue,
}

/// Un cue de la conduite : un média (ou du noir si `path` est vide).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CueData {
    pub name: String,
    pub path: Option<PathBuf>,
    pub looping: bool,
    pub transition: crate::protocol::Transition,
    pub follow: CueFollow,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CuelistData {
    pub cues: Vec<CueData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LecteurItem {
    pub path: PathBuf,
    pub looping: bool,
}

impl Default for LecteurItem {
    fn default() -> Self {
        Self { path: PathBuf::new(), looping: false }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LecteurData {
    pub items: Vec<LecteurItem>,
    pub loop_all: bool,
    pub shuffle: bool,
    pub transition: crate::protocol::Transition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaporamaData {
    pub folder: Option<PathBuf>,
    pub recursive: bool,
    pub shuffle: bool,
    pub looping: bool,
    pub photo_seconds: f64,
    pub fit: Fit,
    pub video_sound: bool,
}

impl Default for DiaporamaData {
    fn default() -> Self {
        Self { folder: None, recursive: true, shuffle: false, looping: true, photo_seconds: 5.0, fit: Fit::Auto, video_sound: false }
    }
}

impl Project {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Self::load_checked(path).map(|(p, _)| p)
    }

    /// Charge le projet. Un `null` égaré (projet modifié à la main) est retiré et la valeur
    /// reprend son réglage par défaut ; `repaired` le signale.
    pub fn load_checked(path: &Path) -> anyhow::Result<(Self, bool)> {
        let text = std::fs::read_to_string(path)?;
        let (mut p, repaired) = match serde_json::from_str::<Project>(&text) {
            Ok(p) => (p, false),
            Err(e) => {
                let mut v: serde_json::Value = serde_json::from_str(&text)?;
                if !strip_nulls(&mut v) {
                    return Err(e.into());
                }
                (serde_json::from_value(v).map_err(|_| e)?, true)
            }
        };
        p.sanitize();
        Ok((p, repaired))
    }

    /// Remet dans leurs bornes les valeurs aberrantes.
    pub fn sanitize(&mut self) {
        let d = &mut self.diaporama;
        d.photo_seconds = if d.photo_seconds.is_finite() { d.photo_seconds.clamp(0.5, 3600.0) } else { 5.0 };
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        self.write_to(path, false)
    }

    pub fn save_with_backup(&self, path: &Path) -> anyhow::Result<()> {
        self.write_to(path, true)
    }

    /// Fichier temporaire propre à ce nom, écrit et forcé sur le disque (une coupure de courant
    /// ne laisse pas un fichier vide), puis renommé. Avec `backup`, l'ancien devient `.bak`.
    fn write_to(&self, path: &Path, backup: bool) -> anyhow::Result<()> {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut clean = self.clone();
        clean.sanitize();
        let json = serde_json::to_string_pretty(&clean)?;
        let tmp = with_suffix(path, ".tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        if backup && path.exists() {
            std::fs::rename(path, with_suffix(path, ".bak"))?;
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Chemins de tous les médias du projet.
    fn paths_mut(&mut self) -> impl Iterator<Item = &mut PathBuf> {
        self.lecteur
            .items
            .iter_mut()
            .map(|i| &mut i.path)
            .chain(self.diaporama.folder.iter_mut())
            .chain(self.cuelist.cues.iter_mut().filter_map(|c| c.path.as_mut()))
    }

    /// Médias introuvables (sans doublon).
    pub fn missing_files(&mut self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.paths_mut().filter(|p| !p.exists()).map(|p| p.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Projet copié ailleurs : un média introuvable est cherché à la même place par rapport
    /// au fichier projet (« …/Spectacle/videos/a.mp4 » → « <dossier du projet>/videos/a.mp4 »).
    pub fn resolve_moved(&mut self, project_file: &Path) -> usize {
        let Some(base) = project_file.parent() else { return 0 };
        let mut fixed = 0;
        for p in self.paths_mut() {
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

    /// Relocalisation : les médias introuvables sont cherchés par nom dans `folder`.
    pub fn relocate(&mut self, folder: &Path) -> usize {
        let mut index: std::collections::HashMap<std::ffi::OsString, PathBuf> = Default::default();
        index_files(folder, &mut index, 0);
        let mut fixed = 0;
        for p in self.paths_mut() {
            if p.exists() {
                continue;
            }
            let key = p.file_name().map(|n| n.to_ascii_lowercase());
            if let Some(found) = key.as_ref().and_then(|k| index.get(k)) {
                *p = found.clone();
                fixed += 1;
            } else if p.extension().is_none() {
                if let Some(d) = key.and_then(|k| find_dir(folder, &k, 0)) {
                    *p = d;
                    fixed += 1;
                }
            }
        }
        fixed
    }
}

fn index_files(dir: &Path, index: &mut std::collections::HashMap<std::ffi::OsString, PathBuf>, depth: usize) {
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

fn find_dir(dir: &Path, name: &std::ffi::OsStr, depth: usize) -> Option<PathBuf> {
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

/// « session.json » → « session.json.tmp ».
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn strip_nulls(v: &mut serde_json::Value) -> bool {
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

fn now_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (d, s) = (secs / 86_400, secs % 86_400);
    // Date civile (UTC), algorithme de H. Hinnant.
    let z = d as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{day:02}_{:02}h{:02}m{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Résultat du chargement de la session au lancement.
pub struct SessionLoad {
    pub project: Project,
    /// Message à afficher bien en vue (session réparée, illisible, copie de secours utilisée).
    pub alert: Option<String>,
}

/// Charge la session sans jamais perdre le fichier : une session illisible est mise de côté
/// (« session-illisible-<date>.json ») et la copie `.bak` prend le relais.
pub fn load_session(path: &Path) -> SessionLoad {
    let bak = with_suffix(path, ".bak");
    if !path.exists() {
        if let Ok(p) = Project::load(&bak) {
            return SessionLoad { project: p, alert: None };
        }
        return SessionLoad { project: Project::default(), alert: None };
    }
    let aside = || {
        let dest = path.with_file_name(format!("session-illisible-{}.json", now_stamp()));
        std::fs::copy(path, &dest).ok().map(|_| dest)
    };
    match Project::load_checked(path) {
        Ok((p, false)) => SessionLoad { project: p, alert: None },
        Ok((p, true)) => {
            let kept = aside().map(|k| format!(" Original gardé : {}", k.display())).unwrap_or_default();
            SessionLoad { project: p, alert: Some(format!("La session contenait des valeurs invalides : elles ont repris leur réglage par défaut.{kept}")) }
        }
        Err(e) => {
            let kept = aside().map(|k| format!(" Fichier gardé : {}", k.display())).unwrap_or_default();
            let _ = std::fs::remove_file(path);
            match Project::load(&bak) {
                Ok(p) => SessionLoad { project: p, alert: Some(format!("Session illisible ({e}). La copie de secours précédente a été chargée.{kept}")) },
                Err(_) => SessionLoad { project: Project::default(), alert: Some(format!("Session illisible ({e}). Aucune copie de secours lisible : projet vierge.{kept}")) },
            }
        }
    }
}

/// Au lancement, garde une copie datée de la session dans « sauvegardes » (les 5 dernières).
pub fn snapshot_session(path: &Path) {
    let Some(dir) = path.parent().map(|d| d.join("sauvegardes")) else { return };
    if !path.exists() || std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::copy(path, dir.join(format!("session-{}.json", now_stamp())));
    let mut old: Vec<PathBuf> =
        std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect()).unwrap_or_default();
    old.sort();
    let n = old.len().saturating_sub(5);
    for p in &old[..n] {
        let _ = std::fs::remove_file(p);
    }
}

/// Fichier de session restauré automatiquement au lancement.
pub fn autosave_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "CueStar").map(|d| d.config_dir().join("session.json"))
}

/// Dernier projet enregistré (pour « Enregistrer » au lancement suivant).
pub fn last_project_path() -> Option<PathBuf> {
    let f = autosave_path()?.with_file_name("dernier-projet.txt");
    let s = std::fs::read_to_string(f).ok()?;
    let p = PathBuf::from(s.trim());
    (!p.as_os_str().is_empty()).then_some(p)
}

pub fn remember_project_path(p: Option<&Path>) {
    if let Some(f) = autosave_path().map(|a| a.with_file_name("dernier-projet.txt")) {
        let _ = std::fs::write(f, p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("cuestar-test-{}", std::process::id()));
        let f = dir.join("p.cuestar");
        let mut p = Project::default();
        p.name = "Spectacle".into();
        p.lecteur.items.push(LecteurItem { path: "/v/a.mp4".into(), looping: true });
        p.lecteur.shuffle = true;
        p.save_with_backup(&f).unwrap();
        p.save_with_backup(&f).unwrap();
        assert!(with_suffix(&f, ".bak").exists());
        assert_eq!(Project::load(&f).unwrap(), p);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nulls_are_repaired() {
        let dir = std::env::temp_dir().join(format!("cuestar-null-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("p.cuestar");
        std::fs::write(&f, r#"{"name":"x","diaporama":{"photo_seconds":null}}"#).unwrap();
        let (p, repaired) = Project::load_checked(&f).unwrap();
        assert!(repaired);
        assert_eq!(p.diaporama.photo_seconds, 5.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
