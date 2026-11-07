//! Panneau Texte : la conduite écrite, enregistrée avec le projet. Le panneau vient de
//! `star_ui::notes`, comme le module Texte de JingleStar.

use egui::Rect;

pub struct NotesPanel {
    pub text: String,
    /// Zoom du texte, gardé le temps de la session (le format du projet ne le prévoit pas).
    pub zoom: f32,
    /// Fichier texte à importer, demandé par le bouton « Importer… ».
    pub import_requested: bool,
}

impl Default for NotesPanel {
    fn default() -> Self {
        Self { text: String::new(), zoom: 1.0, import_requested: false }
    }
}

impl NotesPanel {
    /// Barre de mise en forme et grand champ libre. Renvoie vrai si le champ a le clavier.
    pub fn ui(&mut self, ui: &mut egui::Ui, area: Rect) -> bool {
        let out = star_ui::notes::notes_editor(ui, "cuestar", &self.text, self.zoom, area);
        if let Some(text) = out.text {
            self.text = text;
        }
        if let Some(zoom) = out.zoom {
            self.zoom = zoom;
        }
        self.import_requested |= out.import;
        out.focused
    }

    /// Ajoute un fichier texte (.txt, .md) à la suite. Renvoie le message d'état.
    pub fn import(&mut self, path: &std::path::Path) -> Result<String, String> {
        let (text, status) = star_ui::notes::import(&self.text, path)?;
        self.text = text;
        Ok(status)
    }
}
