//! Onglet Video output : écran et moteur de la sortie, mires de test, mapping 4 coins.

use crate::link::OutputLink;
use crate::output::warp::map_point;
use crate::prefs::{Cadence, Decode, MachinePrefs, RenderApi, Warp};
use crate::protocol::{Command, PatternKind};
use egui::{pos2, vec2, Color32, Key, Pos2, Rect, RichText, Sense, Stroke};
use star_commun::theme::*;
use std::time::{Duration, Instant};

const CORNER_NAMES: [&str; 4] = ["Haut gauche", "Haut droit", "Bas droit", "Bas gauche"];

pub struct OutputView {
    /// Fenêtre flottante ouverte.
    pub open: bool,
    /// Réglages moteur en cours d'édition (appliqués en relançant la sortie).
    draft: Option<MachinePrefs>,
    selected: usize,
    last_warp_send: Instant,
    warp_dirty: bool,
}

impl Default for OutputView {
    fn default() -> Self {
        Self { open: false, draft: None, selected: 0, last_warp_send: Instant::now(), warp_dirty: false }
    }
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL).corner_radius(4).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(16.0).strong());
        ui.add_space(6.0);
        add(ui);
    });
    ui.add_space(10.0);
}

/// Ouvre le panneau « Écrans » du système d'exploitation.
fn open_display_settings() -> Result<(), String> {
    use std::process::Command;
    #[cfg(windows)]
    let tries: &[(&str, &[&str])] = &[("explorer.exe", &["ms-settings:display"])];
    #[cfg(target_os = "macos")]
    let tries: &[(&str, &[&str])] = &[
        ("open", &["x-apple.systempreferences:com.apple.Displays-Settings.extension"]),
        ("open", &["/System/Library/PreferencePanes/Displays.prefPane"]),
    ];
    #[cfg(all(unix, not(target_os = "macos")))]
    let tries: &[(&str, &[&str])] = &[
        ("gnome-control-center", &["display"]),
        ("systemsettings", &["kcm_kscreen"]),
        ("kcmshell6", &["kcm_kscreen"]),
        ("xfce4-display-settings", &[]),
        ("arandr", &[]),
    ];
    for (prog, args) in tries {
        if Command::new(prog).args(*args).spawn().is_ok() {
            return Ok(());
        }
    }
    Err("réglages d'affichage introuvables".into())
}

pub enum Action {
    None,
    /// Les réglages moteur ont changé : relancer la sortie.
    Restart,
}

impl OutputView {
    /// Fenêtre flottante du système (peut sortir de la régie).
    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut MachinePrefs, out: &mut OutputLink, pattern: &mut Option<PatternKind>) -> Action {
        if !self.open {
            return Action::None;
        }
        let vp = egui::ViewportBuilder::default().with_title("CueStar — Video output").with_inner_size([1040.0, 660.0]).with_min_inner_size([600.0, 400.0]);
        let mut close = false;
        let action = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("cuestar-video-output"), vp, |ui, _| {
            close = ui.ctx().input(|i| i.viewport().close_requested());
            let typing = ui.ctx().egui_wants_keyboard_input();
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(BG).inner_margin(10))
                .show(ui, |ui| self.ui(ui, prefs, out, pattern, typing))
                .inner
        });
        if close {
            self.open = false;
            self.draft = None;
        }
        action
    }

    fn ui(&mut self, ui: &mut egui::Ui, prefs: &mut MachinePrefs, out: &mut OutputLink, pattern: &mut Option<PatternKind>, typing: bool) -> Action {
        let mut action = Action::None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.columns(2, |cols| {
                if self.engine(&mut cols[0], prefs, out) {
                    action = Action::Restart;
                }
                self.patterns(&mut cols[0], out, pattern);
                self.mapping(&mut cols[1], prefs, out, pattern, typing);
            });
        });
        // Envoi du mapping limité pendant un glisser : mpv recompile le shader à chaque envoi.
        if self.warp_dirty && self.last_warp_send.elapsed() >= Duration::from_millis(40) {
            out.send(Command::Warp { corners: prefs.warp.active() });
            self.last_warp_send = Instant::now();
            self.warp_dirty = false;
        }
        action
    }

    /// Écran et moteur. Vrai quand il faut relancer la sortie.
    fn engine(&mut self, ui: &mut egui::Ui, prefs: &mut MachinePrefs, out: &OutputLink) -> bool {
        let mut restart = false;
        let d = self.draft.get_or_insert_with(|| prefs.clone());
        section(ui, "Écran et moteur", |ui| {
            let s = &out.state;
            if let Some(m) = &s.monitor {
                let mode = if s.fullscreen { "plein écran" } else { "dans une fenêtre" };
                ui.label(RichText::new(format!("Actuellement : {} — {}×{} @ {:.2} Hz, {mode}", m.name, m.width, m.height, m.refresh_hz)).color(TEXT_DIM));
            }
            egui::Grid::new("engine").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Écran de sortie");
                let auto = "Auto (premier écran secondaire)";
                egui::ComboBox::from_id_salt("screen").selected_text(d.screen.clone().unwrap_or_else(|| auto.into())).width(280.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut d.screen, None, auto);
                    for m in &s.monitors {
                        let txt = format!("{}  {}×{} @ {:.2} Hz{}", m.name, m.width, m.height, m.refresh_hz, if m.primary { " (principal)" } else { "" });
                        ui.selectable_value(&mut d.screen, Some(m.name.clone()), txt);
                    }
                });
                ui.end_row();

                ui.label("Un seul écran");
                ui.checkbox(&mut d.windowed_if_single, "Sortie dans une fenêtre");
                ui.end_row();

                ui.label("Décodage");
                egui::ComboBox::from_id_salt("decode").selected_text(d.decode.label()).width(280.0).show_ui(ui, |ui| {
                    for v in Decode::ALL {
                        ui.selectable_value(&mut d.decode, v, v.label());
                    }
                });
                ui.end_row();

                ui.label("Rendu de secours");
                egui::ComboBox::from_id_salt("render").selected_text(d.render.label()).width(280.0).show_ui(ui, |ui| {
                    for v in RenderApi::available() {
                        ui.selectable_value(&mut d.render, *v, v.label());
                    }
                });
                ui.end_row();

                ui.label("Petite machine");
                ui.checkbox(&mut d.light, "Rendu léger");
                ui.end_row();

                ui.label("Cadence");
                egui::ComboBox::from_id_salt("cadence").selected_text(d.cadence.label()).width(280.0).show_ui(ui, |ui| {
                    for v in Cadence::ALL {
                        ui.selectable_value(&mut d.cadence, v, v.label());
                    }
                })
                .response
                .on_hover_text("Automatique : recalage de vitesse (moins de 1 %) quand l'écran tombe presque juste pour la vidéo, synchro audio sinon");
                ui.end_row();

                ui.label("");
                ui.checkbox(&mut d.interpolation, "Interpolation (lisse le rythme, ex. 24 i/s sur 60 Hz)")
                    .on_hover_text("Fond deux images voisines quand la cadence ne tombe pas juste. Demande un peu de GPU.");
                ui.end_row();

                ui.label("");
                ui.checkbox(&mut d.adapt_refresh, "Adapter la fréquence de l'écran à la vidéo")
                    .on_hover_text("Passe l'écran de sortie en 50, 60… Hz selon la vidéo (Windows), et rétablit sa fréquence en quittant. Noir un instant au changement.");
                ui.end_row();
            });
            let dec = if s.hwdec.is_empty() || s.hwdec == "no" { "CPU".to_string() } else { format!("GPU ({})", s.hwdec) };
            ui.label(RichText::new(format!("Décodeur en cours : {dec}")).color(TEXT_DIM));
            if ui
                .button("Réglages d'affichage du système…")
                .on_hover_text("Ouvre les réglages d'écrans du système : résolution, fréquence, disposition, écran étendu")
                .clicked()
            {
                if let Err(e) = open_display_settings() {
                    ui.ctx().debug_text(e);
                }
            }
            let changed = !d.same_engine(prefs);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new("Appliquer").color(if changed { PLAYING_INK } else { TEXT_DIM })).fill(if changed { ACCENT } else { PANEL_HI });
                if ui.add_enabled(changed, b).clicked() {
                    let warp = prefs.warp;
                    *prefs = d.clone();
                    prefs.warp = warp;
                    prefs.save();
                    restart = true;
                }
                if changed {
                    if ui.button("Annuler").clicked() {
                        *d = prefs.clone();
                    }
                    ui.label(RichText::new("Relance la sortie : noir un instant").color(METER_MID));
                }
            });
        });
        restart
    }

    fn patterns(&mut self, ui: &mut egui::Ui, out: &mut OutputLink, pattern: &mut Option<PatternKind>) {
        section(ui, "Mires de test", |ui| {
            ui.horizontal_wrapped(|ui| {
                for k in PatternKind::ALL {
                    let sel = *pattern == Some(k);
                    let b = egui::Button::new(RichText::new(k.label()).color(if sel { PLAYING_INK } else { TEXT }))
                        .fill(if sel { ACCENT } else { PANEL_HI })
                        .min_size(vec2(110.0, 30.0));
                    if ui.add(b).clicked() {
                        *pattern = if sel { None } else { Some(k) };
                        out.send(Command::Pattern { kind: *pattern });
                    }
                }
            });
            ui.add_space(4.0);
            if ui.add_enabled(pattern.is_some(), egui::Button::new("Retirer la mire")).clicked() {
                *pattern = None;
                out.send(Command::Pattern { kind: None });
            }
        });
    }

    fn mapping(&mut self, ui: &mut egui::Ui, prefs: &mut MachinePrefs, out: &mut OutputLink, pattern: &mut Option<PatternKind>, typing: bool) {
        let before = prefs.warp;
        section(ui, "Mapping 4 coins", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut prefs.warp.enabled, "Activer");
                if ui.button("Réinitialiser").clicked() {
                    prefs.warp.corners = Warp::IDENTITY;
                }
                let grid = *pattern == Some(PatternKind::Grid);
                if ui.selectable_label(grid, "Grille de réglage").on_hover_text("Affiche la grille sur la sortie pendant le réglage").clicked() {
                    *pattern = if grid { None } else { Some(PatternKind::Grid) };
                    out.send(Command::Pattern { kind: *pattern });
                }
            });
            ui.label(RichText::new("Fais glisser les coins ; flèches = 1 pixel, Maj + flèches = 10 pixels sur le coin choisi.").color(TEXT_DIM));
            ui.add_space(6.0);

            let (mw, mh) = out.state.monitor.as_ref().map(|m| (m.width.max(1) as f32, m.height.max(1) as f32)).unwrap_or((1920.0, 1080.0));
            let width = ui.available_width().min(720.0);
            let (rect, _) = ui.allocate_exact_size(vec2(width, width * mh / mw), Sense::hover());
            self.editor(ui, rect, &mut prefs.warp);

            // Ajustement fin au clavier du coin choisi.
            if !typing {
                let (dx, dy, shift) = ui.input(|i| {
                    let mut d = (0.0f32, 0.0f32);
                    if i.key_pressed(Key::ArrowLeft) {
                        d.0 -= 1.0;
                    }
                    if i.key_pressed(Key::ArrowRight) {
                        d.0 += 1.0;
                    }
                    if i.key_pressed(Key::ArrowUp) {
                        d.1 -= 1.0;
                    }
                    if i.key_pressed(Key::ArrowDown) {
                        d.1 += 1.0;
                    }
                    (d.0, d.1, i.modifiers.shift)
                });
                if dx != 0.0 || dy != 0.0 {
                    let k = if shift { 10.0 } else { 1.0 };
                    let c = &mut prefs.warp.corners[self.selected];
                    c[0] = (c[0] + dx * k / mw).clamp(0.0, 1.0);
                    c[1] = (c[1] + dy * k / mh).clamp(0.0, 1.0);
                }
            }

            ui.add_space(6.0);
            egui::Grid::new("corners").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                for (i, name) in CORNER_NAMES.iter().enumerate() {
                    if ui.selectable_label(self.selected == i, *name).clicked() {
                        self.selected = i;
                    }
                    let c = &mut prefs.warp.corners[i];
                    let mut x = c[0] * mw;
                    let mut y = c[1] * mh;
                    if ui.add(egui::DragValue::new(&mut x).range(0.0..=mw).speed(1.0).prefix("x ").suffix(" px")).changed() {
                        c[0] = x / mw;
                    }
                    if ui.add(egui::DragValue::new(&mut y).range(0.0..=mh).speed(1.0).prefix("y ").suffix(" px")).changed() {
                        c[1] = y / mh;
                    }
                    ui.end_row();
                }
            });
        });
        if prefs.warp != before {
            // Déplacer un coin active le mapping.
            if prefs.warp.corners != before.corners {
                prefs.warp.enabled = true;
            }
            prefs.save();
            self.warp_dirty = true;
        }
    }

    /// Aperçu de l'écran de sortie : quadrilatère, grille déformée et poignées.
    fn editor(&mut self, ui: &mut egui::Ui, rect: Rect, warp: &mut Warp) {
        let p = ui.painter_at(rect.expand(8.0));
        p.rect_filled(rect, 2, Color32::from_rgb(0x10, 0x10, 0x12));
        p.rect_stroke(rect, 2, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
        let to_screen = |q: [f32; 2]| pos2(rect.left() + q[0] * rect.width(), rect.top() + q[1] * rect.height());
        let col = if warp.enabled { ACCENT } else { TEXT_DIM };

        // Grille 16 × 9 déformée par l'homographie.
        let line = |a: (f32, f32), b: (f32, f32)| -> Vec<Pos2> {
            (0..=24)
                .map(|t| {
                    let t = t as f32 / 24.0;
                    to_screen(map_point(warp.corners, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t))
                })
                .collect()
        };
        for i in 1..16 {
            let u = i as f32 / 16.0;
            p.add(egui::Shape::line(line((u, 0.0), (u, 1.0)), Stroke::new(1.0, col.gamma_multiply(0.3))));
        }
        for i in 1..9 {
            let v = i as f32 / 9.0;
            p.add(egui::Shape::line(line((0.0, v), (1.0, v)), Stroke::new(1.0, col.gamma_multiply(0.3))));
        }
        let quad: Vec<Pos2> = warp.corners.iter().map(|c| to_screen(*c)).collect();
        p.add(egui::Shape::closed_line(quad.clone(), Stroke::new(2.0, col)));

        for (i, pos) in quad.iter().enumerate() {
            let id = ui.scope_id().with(("corner", i));
            let resp = ui.interact(Rect::from_center_size(*pos, vec2(22.0, 22.0)), id, Sense::click_and_drag());
            if resp.clicked() || resp.drag_started() {
                self.selected = i;
            }
            if resp.dragged() {
                let d = resp.drag_delta();
                let c = &mut warp.corners[i];
                c[0] = (c[0] + d.x / rect.width()).clamp(0.0, 1.0);
                c[1] = (c[1] + d.y / rect.height()).clamp(0.0, 1.0);
            }
            let hot = resp.hovered() || resp.dragged() || self.selected == i;
            p.circle_filled(*pos, if hot { 8.0 } else { 6.0 }, if self.selected == i { ACCENT } else { TEXT });
            p.circle_stroke(*pos, 9.0, Stroke::new(1.0, PLAYING_INK));
        }
    }
}
