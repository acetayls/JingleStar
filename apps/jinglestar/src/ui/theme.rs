//! Thème : celui des applis Star (star-ui), plus les couleurs des boutons de jingle.

pub use star_ui::theme::*;

use egui::Color32;

/// Fond d'un bouton de jingle : couleur assombrie au repos, presque pleine en lecture.
pub fn slot_fill(rgb: [u8; 3], playing: bool) -> Color32 {
    let k = if playing { 0.92 } else { 0.32 };
    Color32::from_rgb((rgb[0] as f32 * k) as u8, (rgb[1] as f32 * k) as u8, (rgb[2] as f32 * k) as u8)
}
