// Évite l'ouverture d'une console sous Windows en mode release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod controller;
mod osc;
mod project;
mod remote;
mod ui;

fn main() -> eframe::Result {
    star_core::log::init(project::APP);
    star_core::log::install_panic_hook(&format!("Démarrage de JingleStar {}", env!("CARGO_PKG_VERSION")));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("JingleStar")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true)
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/jinglestar-fenetre-256.png"))
                    .expect("icône PNG valide"),
            ),
        ..Default::default()
    };
    let res = eframe::run_native(
        "JingleStar",
        options,
        Box::new(|cc| Ok(Box::new(ui::JingleStarApp::new(cc)))),
    );
    if let Err(e) = &res {
        star_core::log::write(&format!("Arrêt sur erreur : {e}"));
    }
    res
}
