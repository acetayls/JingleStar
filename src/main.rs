// Évite l'ouverture d'une console sous Windows en mode release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod controller;
mod i18n;
mod osc;
mod project;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("JingleStar")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "JingleStar",
        options,
        Box::new(|cc| Ok(Box::new(ui::JingleStarApp::new(cc)))),
    )
}
