//! Session restaurée au lancement : jamais perdue (copie de secours, fichier illisible mis de
//! côté) et copiée chaque jour de lancement dans « sauvegardes ». Repris de JingleStar.

use std::path::{Path, PathBuf};

use crate::document::{load, load_checked, with_suffix, Document};
use crate::i18n::tr;
use crate::log;

/// Résultat du chargement de la session au lancement.
pub struct SessionLoad<D> {
    pub project: D,
    /// Message à afficher bien en vue : session réparée, illisible, copie de secours utilisée.
    pub alert: Option<String>,
}

/// Charge la session sans jamais perdre le fichier : une session illisible est mise de côté
/// (« session-illisible-<date>.json ») au lieu d'être écrasée, et la copie `.bak` prend le relais.
pub fn load_session<D: Document>(path: &Path) -> SessionLoad<D> {
    let bak = with_suffix(path, ".bak");
    if !path.exists() {
        // Arrêt brutal entre les deux renommages de la sauvegarde : la copie est complète.
        if let Ok(p) = load::<D>(&bak) {
            return SessionLoad { project: p, alert: None };
        }
        return SessionLoad { project: D::default(), alert: None };
    }
    let aside = || {
        let name = format!("session-illisible-{}.json", log::now(true));
        let dest = path.with_file_name(name);
        std::fs::copy(path, &dest).ok().map(|_| dest)
    };
    match load_checked::<D>(path) {
        Ok((p, false)) => SessionLoad { project: p, alert: None },
        Ok((p, true)) => {
            let kept = aside();
            let msg = format!(
                "{}{}",
                tr(
                    "La session contenait des valeurs invalides : elles ont repris leur réglage par défaut.",
                    "The session contained invalid values: they were reset to their defaults."
                ),
                kept.map(|k| format!(" {} {}", tr("Original gardé :", "Original kept:"), k.display())).unwrap_or_default()
            );
            log::write(&msg);
            SessionLoad { project: p, alert: Some(msg) }
        }
        Err(e) => {
            let kept = aside();
            let _ = std::fs::remove_file(path);
            let kept = kept.map(|k| format!(" {} {}", tr("Fichier gardé :", "File kept:"), k.display())).unwrap_or_default();
            let (project, msg) = match load::<D>(&bak) {
                Ok(p) => (p, format!("{} ({e}). {}{kept}", tr("Session illisible", "Unreadable session"), tr("La copie de secours précédente a été chargée.", "The previous backup copy was loaded."))),
                Err(_) => (D::default(), format!("{} ({e}). {}{kept}", tr("Session illisible", "Unreadable session"), tr("Aucune copie de secours lisible : projet vierge.", "No readable backup: blank project."))),
            };
            log::write(&msg);
            SessionLoad { project, alert: Some(msg) }
        }
    }
}

/// Au lancement, garde une copie datée de la session dans « sauvegardes » (les 5 dernières).
pub fn snapshot_session(path: &Path) {
    let Some(dir) = path.parent().map(|d| d.join("sauvegardes")) else { return };
    if !path.exists() || std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::copy(path, dir.join(format!("session-{}.json", log::now(true))));
    let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect())
        .unwrap_or_default();
    old.sort();
    let n = old.len().saturating_sub(5);
    for p in &old[..n] {
        let _ = std::fs::remove_file(p);
    }
}
