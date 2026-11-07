//! Identité d'une appli Star et ses dossiers sur cette machine (config, cache), toujours
//! « com.acetayls.<Nom> » : les chemins existants ne changent pas.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct App {
    /// Nom affiché et nom de dossier : « JingleStar », « CueStar ».
    pub name: &'static str,
}

impl App {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    fn dirs(&self) -> Option<directories::ProjectDirs> {
        directories::ProjectDirs::from("com", "acetayls", self.name)
    }

    /// Fichier `file` du dossier de configuration (préférences, session, journal).
    pub fn config_file(&self, file: &str) -> Option<PathBuf> {
        self.dirs().map(|d| d.config_dir().join(file))
    }

    /// Fichier ou dossier `file` du dossier de cache.
    pub fn cache_file(&self, file: &str) -> Option<PathBuf> {
        self.dirs().map(|d| d.cache_dir().join(file))
    }

    /// Fichier de session restauré automatiquement au lancement.
    pub fn session_path(&self) -> Option<PathBuf> {
        self.config_file("session.json")
    }

    /// Journal de l'appli : « jinglestar.log », « cuestar.log ».
    pub fn log_path(&self) -> Option<PathBuf> {
        self.config_file(&format!("{}.log", self.name.to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_follow_the_app_name() {
        let app = App::new("JingleStar");
        let d = directories::ProjectDirs::from("com", "acetayls", "JingleStar").unwrap();
        assert_eq!(app.session_path().unwrap(), d.config_dir().join("session.json"));
        assert_eq!(app.log_path().unwrap(), d.config_dir().join("jinglestar.log"));
        assert_eq!(app.cache_file("loudness.json").unwrap(), d.cache_dir().join("loudness.json"));
    }
}
