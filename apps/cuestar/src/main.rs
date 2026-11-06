// Évite l'ouverture d'une console sous Windows en mode release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! CueStar : un seul exécutable, deux rôles.
//! - sans argument : la régie (interface egui) ;
//! - `--output <config JSON>` : le moteur de sortie, processus séparé lancé par la régie.
//!   Il possède la fenêtre plein écran et libmpv ; si la régie rame, l'image continue.

mod console;
mod link;
mod m32;
mod midi;
mod osc;
mod output;
mod prefs;
mod project;
mod protocol;
mod remote;
mod stats;
mod thumb;
mod ui;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Journal commun à la régie et à la sortie (cuestar.log, à côté de la session).
    star_core::log::init(project::APP);
    if args.get(1).map(String::as_str) == Some("--output") {
        star_core::log::install_panic_hook(&format!("Démarrage de la sortie de CueStar {}", env!("CARGO_PKG_VERSION")));
        let cfg = args.get(2).and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
        let monitor_port = args.get(3).and_then(|s| s.parse().ok());
        output::run(cfg, monitor_port);
        return;
    }
    if args.get(1).map(String::as_str) == Some("--thumb") {
        if let (Some(media), Some(dir)) = (args.get(2), args.get(3)) {
            thumb::extract(media, dir);
        }
        return;
    }
    star_core::log::install_panic_hook(&format!("Démarrage de CueStar {}", env!("CARGO_PKG_VERSION")));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CueStar")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/cuestar-256.png")).expect("icône PNG valide")),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("CueStar", options, Box::new(|cc| Ok(Box::new(ui::CueStarApp::new(cc))))) {
        star_core::log::write(&format!("Arrêt sur erreur : {e}"));
    }
}
