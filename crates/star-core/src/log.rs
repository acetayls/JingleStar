//! Journal de l'application, à côté de la session (`jinglestar.log`, `cuestar.log`), et trace
//! des plantages : sans console sous Windows, c'est la seule trace qui reste.
//!
//! [`init`] au lancement choisit le fichier ; avant (et dans les tests), rien n'est écrit.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::app::App;
pub use crate::time::now;

/// Au-delà, le journal repart d'un fichier vide (l'ancien est gardé en `.old.log`).
const MAX_BYTES: u64 = 1_000_000;

static PATH: OnceLock<PathBuf> = OnceLock::new();

/// Journal de `app` pour tout le processus. Sans effet après le premier appel.
pub fn init(app: App) {
    if let Some(p) = app.log_path() {
        let _ = PATH.set(p);
    }
}

/// Fichier du journal, une fois [`init`] appelé.
pub fn log_path() -> Option<PathBuf> {
    PATH.get().cloned()
}

/// Ajoute une ligne datée au journal. Ne bloque jamais l'application si le disque refuse.
pub fn write(msg: &str) {
    let Some(path) = PATH.get() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {msg}", now(false));
    }
}

/// Inscrit chaque panique au journal (thread, message, emplacement), puis laisse faire
/// le comportement habituel. Les paniques rattrapées sont aussi notées. `started` : la ligne
/// de démarrage, par exemple « Démarrage de JingleStar 0.1.4 ».
pub fn install_panic_hook(started: &str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        write(&format!("PANIQUE dans le thread « {thread} » : {info}\n{bt}"));
        previous(info);
    }));
    write(started);
}
