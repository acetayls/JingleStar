//! Module Cuelist (à venir) : liste de cues vidéo avec GO, comme dans JingleStar.

use egui::RichText;
use star_commun::theme::*;

#[derive(Default)]
pub struct Cuelist;

impl Cuelist {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("Cuelist").size(22.0).strong());
            ui.add_space(8.0);
            ui.label(RichText::new("Prochaine étape : cues vidéo, image et noir, avec GO, enchaînements et pilotage OSC.").color(TEXT_DIM));
        });
    }
}
