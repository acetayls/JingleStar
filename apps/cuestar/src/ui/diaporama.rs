//! Module Diaporama (à venir) : reprise du projet « Diaporama » (Electron) sur le moteur CueStar.
//!
//! Prévu : dossier (sous-dossiers, ordre aléatoire, boucle), durée des photos, ajustement
//! (contenir, remplir, auto, fond flou), Ken Burns, 9 transitions + aléatoire, vidéos lues
//! en entier, musique de fond avec baisse pendant les vidéos sonores, mêmes commandes OSC.

use egui::RichText;
use star_commun::theme::*;

#[derive(Default)]
pub struct Diaporama;

impl Diaporama {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("Diaporama").size(22.0).strong());
            ui.add_space(8.0);
            ui.label(RichText::new("Prochaine étape : photos et vidéos d'un dossier, transitions, Ken Burns, musique de fond.").color(TEXT_DIM));
        });
    }
}
