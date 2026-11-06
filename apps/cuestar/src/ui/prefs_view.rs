//! Fenêtre Préférences : réglages propres à cette machine.

use crate::link::OutputState;
use crate::prefs::{Decode, MachinePrefs, RenderApi};
use egui::RichText;
use star_commun::theme::*;

#[derive(Default)]
pub struct PrefsView {
    draft: Option<MachinePrefs>,
}

impl PrefsView {
    pub fn open(&mut self, current: &MachinePrefs) {
        self.draft = Some(current.clone());
    }

    /// Renvoie les nouvelles préférences quand l'utilisateur clique « Appliquer ».
    pub fn show(&mut self, ctx: &egui::Context, out: &OutputState) -> Option<MachinePrefs> {
        let mut result = None;
        let mut close = false;
        let Some(d) = self.draft.as_mut() else { return None };
        egui::Window::new("Préférences de cette machine").collapsible(false).resizable(false).default_width(460.0).show(ctx, |ui| {
            ui.label(RichText::new("Ces réglages restent sur ce PC : le spectacle n'en dépend pas.").color(TEXT_DIM));
            ui.add_space(6.0);
            egui::Grid::new("prefs").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Écran de sortie");
                let label = d.screen.clone().unwrap_or_else(|| "Auto (premier écran secondaire)".into());
                egui::ComboBox::from_id_salt("screen").selected_text(label).width(260.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut d.screen, None, "Auto (premier écran secondaire)");
                    for m in &out.monitors {
                        let txt = format!(
                            "{}  {}×{} @ {:.2} Hz{}",
                            m.name,
                            m.width,
                            m.height,
                            m.refresh_hz,
                            if m.primary { " (principal)" } else { "" }
                        );
                        ui.selectable_value(&mut d.screen, Some(m.name.clone()), txt);
                    }
                });
                ui.end_row();

                ui.label("Un seul écran");
                ui.checkbox(&mut d.windowed_if_single, "Sortie dans une fenêtre (sinon plein écran)");
                ui.end_row();

                ui.label("Décodage");
                egui::ComboBox::from_id_salt("decode").selected_text(d.decode.label()).width(260.0).show_ui(ui, |ui| {
                    for v in Decode::ALL {
                        ui.selectable_value(&mut d.decode, v, v.label());
                    }
                });
                ui.end_row();

                ui.label("Rendu");
                egui::ComboBox::from_id_salt("render").selected_text(d.render.label()).width(260.0).show_ui(ui, |ui| {
                    for v in RenderApi::available() {
                        ui.selectable_value(&mut d.render, *v, v.label());
                    }
                });
                ui.end_row();

                ui.label("Petite machine");
                ui.checkbox(&mut d.light, "Rendu léger (moins de qualité de mise à l'échelle)");
                ui.end_row();

                ui.label("Fluidité");
                ui.checkbox(&mut d.display_sync, "Caler la lecture sur la fréquence de l'écran");
                ui.end_row();
            });
            ui.add_space(8.0);
            if !out.hwdec.is_empty() {
                ui.label(RichText::new(format!("Décodeur en cours : {}", out.hwdec)).color(TEXT_DIM));
            }
            ui.label(RichText::new("Appliquer relance la sortie : l'image passe au noir un instant.").color(METER_MID));
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("Appliquer").color(PLAYING_INK)).fill(ACCENT)).clicked() {
                    result = Some(d.clone());
                    close = true;
                }
                if ui.button("Annuler").clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.draft = None;
        }
        result
    }
}
