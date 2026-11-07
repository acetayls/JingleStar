//! Module Texte : la conduite écrite, sauvegardée avec le projet. Le panneau vient de
//! `star_ui::notes` ; ici, le lien avec le module du projet.

use egui::Rect;

use super::dialogs::{Kind, Pick};
use super::JingleStarApp;

impl JingleStarApp {
    /// Module Texte : une barre de mise en forme et un grand champ libre pour la conduite,
    /// sauvegardé avec le projet.
    pub(super) fn notes_view(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        let module = &self.ctrl.project.modules[m];
        let out = star_ui::notes::notes_editor(ui, m, &module.notes, module.notes_zoom, area);
        if let Some(text) = out.text {
            self.ctrl.set_module_notes(m, text);
        }
        if let Some(zoom) = out.zoom {
            self.ctrl.set_notes_zoom(m, zoom);
        }
        if out.import {
            self.dialogs.show(ui.ctx(), Pick::NotesImport(m), Kind::Text);
        }
        self.notes_focused |= out.focused;
    }

    /// Importe un fichier texte (.txt, .md) à la suite de la conduite.
    pub(super) fn import_notes(&mut self, m: usize, path: &std::path::Path) {
        match star_ui::notes::import(&self.ctrl.project.modules[m].notes, path) {
            Ok((notes, status)) => {
                self.ctrl.set_module_notes(m, notes);
                self.ctrl.status = Some(status);
            }
            Err(msg) => self.ctrl.status = Some(msg),
        }
    }
}
