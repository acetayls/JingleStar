//! Thème sombre inspiré de QLab.

use egui::{Color32, CornerRadius, Stroke};

pub const BG: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1d);
pub const PANEL: Color32 = Color32::from_rgb(0x24, 0x24, 0x29);
pub const PANEL_HI: Color32 = Color32::from_rgb(0x2e, 0x2e, 0x35);
pub const SLOT_EMPTY: Color32 = Color32::from_rgb(0x20, 0x20, 0x24);
pub const BORDER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x43);
pub const TEXT: Color32 = Color32::from_rgb(0xdd, 0xdd, 0xe2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x85, 0x85, 0x90);
pub const GO: Color32 = Color32::from_rgb(0x46, 0xc8, 0x55);
pub const STOP: Color32 = Color32::from_rgb(0xd9, 0x48, 0x48);
pub const ACCENT: Color32 = Color32::from_rgb(0xf0, 0xb0, 0x30);
pub const EDIT: Color32 = Color32::from_rgb(0x4a, 0x90, 0xe2);
pub const METER_LOW: Color32 = Color32::from_rgb(0x46, 0xc8, 0x55);
pub const METER_MID: Color32 = Color32::from_rgb(0xe8, 0xc5, 0x3a);
pub const METER_HIGH: Color32 = Color32::from_rgb(0xe0, 0x45, 0x45);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(6);
    v.extreme_bg_color = Color32::from_rgb(0x14, 0x14, 0x16);
    v.faint_bg_color = PANEL_HI;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = EDIT;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.bg_fill = PANEL_HI;
    w.inactive.weak_bg_fill = PANEL_HI;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.hovered.bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x44);
    w.hovered.weak_bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x44);
    w.active.bg_fill = Color32::from_rgb(0x45, 0x45, 0x50);
    w.active.weak_bg_fill = Color32::from_rgb(0x45, 0x45, 0x50);
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::same(4);
    }
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}

/// Assombrit une couleur de jingle pour le fond du bouton.
pub fn slot_fill(rgb: [u8; 3], playing: bool) -> Color32 {
    let k = if playing { 0.55 } else { 0.38 };
    Color32::from_rgb((rgb[0] as f32 * k) as u8, (rgb[1] as f32 * k) as u8, (rgb[2] as f32 * k) as u8)
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}
