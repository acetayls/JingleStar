//! Module GO : la conduite, le cue en attente et le gros bouton GO.

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Rect, RichText, Sense, Stroke, StrokeKind};

use super::theme::*;
use super::widgets::*;
use super::{DropTarget, JingleStarApp, TrackPayload};
use crate::i18n::tr;

/// Cue en cours de glisser pour changer l'ordre de la conduite.
pub(super) struct CuePayload {
    pub module: usize,
    pub index: usize,
    pub name: String,
}

const ROW_H: f32 = 26.0;

/// Gros bouton GO, éteint quand la conduite est terminée.
fn go_button(ui: &mut egui::Ui, rect: Rect, id: Id, enabled: bool) -> egui::Response {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter_at(rect.expand(1.0));
    let r = CornerRadius::same(6);
    let fill = if !enabled {
        PANEL_HI
    } else if resp.is_pointer_button_down_on() {
        GO.gamma_multiply(0.8)
    } else if resp.hovered() {
        GO.gamma_multiply(1.15)
    } else {
        GO
    };
    p.rect_filled(rect, r, fill);
    p.rect_stroke(rect, r, Stroke::new(1.0, Color32::from_black_alpha(110)), StrokeKind::Inside);
    let fg = if enabled { Color32::from_rgb(0x0c, 0x1a, 0x0e) } else { TEXT_DIM };
    let size = (rect.height() * 0.5).clamp(16.0, 34.0);
    p.text(rect.center(), Align2::CENTER_CENTER, "GO", FontId::proportional(size), fg);
    p.text(rect.right_bottom() + vec2(-8.0, -5.0), Align2::RIGHT_BOTTOM, tr("Espace", "Space"), FontId::proportional(10.5), fg.gamma_multiply(0.7));
    resp
}

impl JingleStarApp {
    pub(super) fn go_view(&mut self, ui: &mut egui::Ui, m: usize, area: Rect) {
        self.drop_targets.push((area, DropTarget::Go(m)));
        let editing = self.edit_mode[m];
        let cues = self.ctrl.cues(m).to_vec();
        let len = cues.len();
        let standby = self.ctrl.gos[m].standby.min(len);
        let rate = self.ctrl.device.as_ref().map(|d| d.sample_rate).unwrap_or(48_000) as f64;

        ui.horizontal(|ui| {
            if ui.button(tr("＋ Ajouter des sons…", "＋ Add sounds…")).clicked() {
                if let Some(files) = rfd::FileDialog::new().add_filter("Audio", crate::audio::decode::AUDIO_EXTENSIONS).pick_files() {
                    self.ctrl.add_cues(m, files, None);
                }
            }
            if editing && len > 0 && ui.button("🗑").on_hover_text(tr("Vider la conduite", "Clear the cue list")).clicked() {
                self.ctrl.clear_cues(m);
            }
            let count = if len == 1 { "1 cue".to_string() } else { format!("{len} cues") };
            ui.label(RichText::new(count).color(TEXT_DIM));
        });

        // Cue en attente : ce que le prochain GO va lancer.
        egui::Frame::new()
            .fill(Color32::from_rgb(0x1c, 0x1c, 0x20))
            .stroke(Stroke::new(1.0, if standby < len { ACCENT.gamma_multiply(0.7) } else { BORDER }))
            .corner_radius(4)
            .inner_margin(6)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr("EN ATTENTE", "STANDBY")).small().strong().color(if standby < len { ACCENT } else { TEXT_DIM }));
                match cues.get(standby) {
                    Some(c) => {
                        ui.add(egui::Label::new(RichText::new(format!("{}  {}", standby + 1, c.name)).strong().size(18.0).color(ACCENT)).truncate());
                        let next = match cues.get(standby + 1) {
                            Some(n) => format!("{} {}  {}", tr("Ensuite :", "Then:"), standby + 2, n.name),
                            None => tr("Dernier cue de la conduite", "Last cue of the list").into(),
                        };
                        ui.add(egui::Label::new(RichText::new(next).small().color(TEXT_DIM)).truncate());
                    }
                    None => {
                        let txt = if len == 0 {
                            tr("Ajoutez des sons à la conduite", "Add sounds to the cue list")
                        } else {
                            tr("Fin de la conduite", "End of the cue list")
                        };
                        ui.label(RichText::new(txt).size(18.0).color(TEXT_DIM));
                        ui.label(RichText::new(tr("Cliquez sur un cue pour le mettre en attente", "Click a cue to put it on standby")).small().color(TEXT_DIM));
                    }
                }
            });

        let go_h = (ui.available_height() * 0.22).clamp(44.0, 84.0);
        let (go_r, _) = ui.allocate_exact_size(vec2(ui.available_width(), go_h), Sense::hover());
        if go_button(ui, go_r, ui.make_persistent_id(("go", m)), standby < len)
            .on_hover_text(tr("Lance le cue en attente et passe au suivant (barre d'espace)", "Plays the standby cue and moves to the next one (space bar)"))
            .clicked()
        {
            self.ctrl.cue_go(m);
        }

        // Précédent, tout arrêter, suivant.
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let gap = 4.0;
        let w = (row.width() - 2.0 * gap) / 3.0;
        let cell = |k: f32| Rect::from_min_size(pos2(row.left() + k * (w + gap), row.top()), vec2(w, row.height()));
        if transport_button(ui, cell(0.0), ("go-prev", m), TransportIcon::Prev, Some(tr("Précédent", "Previous")), PANEL_HI, TEXT)
            .on_hover_text(tr("Met le cue précédent en attente", "Puts the previous cue on standby"))
            .clicked()
        {
            self.ctrl.cue_step(m, -1);
        }
        if transport_button(ui, cell(1.0), ("go-stop", m), TransportIcon::Stop, Some(tr("Arrêter", "Stop")), STOP.gamma_multiply(0.75), Color32::WHITE)
            .on_hover_text(tr("Arrête tous les cues de la conduite", "Stops every cue of the list"))
            .clicked()
        {
            self.ctrl.stop_cues(m);
        }
        if transport_button(ui, cell(2.0), ("go-next", m), TransportIcon::Next, Some(tr("Suivant", "Next")), PANEL_HI, TEXT)
            .on_hover_text(tr("Met le cue suivant en attente", "Puts the next cue on standby"))
            .clicked()
        {
            self.ctrl.cue_step(m, 1);
        }
        ui.add_space(2.0);

        // La conduite : clic = mettre en attente, glisser = changer l'ordre.
        let mut moved: Option<(usize, usize)> = None;
        let mut track_drop: Option<(std::path::PathBuf, usize)> = None;
        let mut action: Option<(usize, CueAction)> = None;
        egui::ScrollArea::vertical().id_salt(("go-list", m)).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let pointer = ui.ctx().pointer_hover_pos();
            let mut insert_line: Option<f32> = None;
            for (i, c) in cues.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
                let p = ui.painter_at(rect);
                let is_standby = i == standby;
                let pos = self.ctrl.cue_position(m, i);
                if is_standby {
                    p.rect_filled(rect, CornerRadius::same(3), ACCENT.gamma_multiply(0.18));
                    p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), CornerRadius::same(1), ACCENT);
                } else if resp.hovered() {
                    p.rect_filled(rect, CornerRadius::same(3), PANEL_HI);
                }
                // Barre de progression derrière le nom quand le cue sonne.
                if let Some((pp, l)) = pos {
                    let frac = if l > 0 { (pp as f32 / l as f32).clamp(0.0, 1.0) } else { 0.0 };
                    p.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width() * frac, rect.height())), CornerRadius::same(3), GO.gamma_multiply(0.22));
                }
                let col = if pos.is_some() { GO } else if is_standby { ACCENT } else { TEXT };
                let marker = if is_standby { "▶" } else { " " };
                p.text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, marker, FontId::proportional(11.0), ACCENT);
                p.text(pos2(rect.left() + 34.0, rect.center().y), Align2::RIGHT_CENTER, format!("{}", i + 1), FontId::monospace(12.0), TEXT_DIM);

                // À droite : arrêt et temps restant s'il sonne, puis le volume.
                let mut right = rect.right() - 4.0;
                if let Some((pp, l)) = pos {
                    let stop_r = Rect::from_center_size(pos2(right - 11.0, rect.center().y), vec2(22.0, 20.0));
                    right = stop_r.left() - 6.0;
                    let sresp = ui.interact(stop_r, ui.make_persistent_id(("cue-stop", m, i)), Sense::click());
                    let fill = if sresp.hovered() { STOP } else { STOP.gamma_multiply(0.7) };
                    ui.painter().rect_filled(stop_r, CornerRadius::same(3), fill);
                    ui.painter().rect_filled(Rect::from_center_size(stop_r.center(), vec2(8.0, 8.0)), CornerRadius::same(1), Color32::WHITE);
                    if sresp.on_hover_text(tr("Arrêter ce cue", "Stop this cue")).clicked() {
                        action = Some((i, CueAction::Stop));
                    }
                    let remaining = fmt_tenths(((l.saturating_sub(pp)) as f64 / rate).max(0.0));
                    let g = p.layout_no_wrap(format!("-{remaining}"), FontId::monospace(11.0), GO);
                    p.galley(pos2(right - g.size().x, rect.center().y - g.size().y / 2.0), g.clone(), GO);
                    right -= g.size().x + 8.0;
                } else if self.ctrl.cue_loading(m, i) {
                    p.text(pos2(right, rect.center().y), Align2::RIGHT_CENTER, "…", FontId::monospace(11.0), TEXT_DIM);
                    right -= 16.0;
                }
                let vol_col = if c.volume_db.abs() < 0.05 { TEXT_DIM } else { ACCENT };
                let vol_w = ui.painter().layout_no_wrap("-40.0 dB".into(), FontId::monospace(11.0), vol_col).size().x;
                if let Some(db) = db_value(
                    ui,
                    ui.make_persistent_id(("cue-vol", m, i)),
                    pos2(right, rect.center().y),
                    Align2::RIGHT_CENTER,
                    c.volume_db,
                    -40.0,
                    -40.0..=10.0,
                    FontId::monospace(11.0),
                    vol_col,
                ) {
                    action = Some((i, CueAction::Volume(db)));
                }
                right -= vol_w + 8.0;

                let name_r = Rect::from_min_max(pos2(rect.left() + 42.0, rect.top() + 2.0), pos2(right, rect.bottom() - 2.0));
                if editing {
                    let mut name = c.name.clone();
                    if ui.put(name_r, egui::TextEdit::singleline(&mut name).font(FontId::proportional(13.0))).changed() {
                        action = Some((i, CueAction::Rename(name)));
                    }
                } else {
                    let mut job = egui::text::LayoutJob::simple_singleline(c.name.clone(), FontId::proportional(13.5), col);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(name_r.width().max(10.0));
                    let g = p.layout_job(job);
                    p.galley(pos2(name_r.left(), rect.center().y - g.size().y / 2.0), g, col);
                }

                if resp.clicked() {
                    action = Some((i, CueAction::Standby));
                }
                resp.dnd_set_drag_payload(CuePayload { module: m, index: i, name: c.name.clone() });
                let after = pointer.is_some_and(|pt| pt.y > rect.center().y);
                let at = if after { i + 1 } else { i };
                let dragging_here = resp.dnd_hover_payload::<CuePayload>().is_some_and(|pl| pl.module == m)
                    || resp.dnd_hover_payload::<TrackPayload>().is_some();
                if dragging_here {
                    insert_line = Some(if after { rect.bottom() } else { rect.top() });
                }
                if let Some(pl) = resp.dnd_release_payload::<CuePayload>() {
                    if pl.module == m {
                        moved = Some((pl.index, at));
                    }
                }
                if let Some(track) = resp.dnd_release_payload::<TrackPayload>() {
                    track_drop = Some((track.0.clone(), at));
                }
                resp.context_menu(|ui| {
                    if ui.button(tr("Lancer maintenant", "Play now")).clicked() {
                        action = Some((i, CueAction::Play));
                        ui.close();
                    }
                    if ui.button(tr("Mettre en attente", "Put on standby")).clicked() {
                        action = Some((i, CueAction::Standby));
                        ui.close();
                    }
                    if ui.button(tr("Retirer de la conduite", "Remove from the list")).clicked() {
                        action = Some((i, CueAction::Remove));
                        ui.close();
                    }
                });
            }
            // Sous la liste : dépôt à la fin.
            let rest = ui.available_rect_before_wrap();
            let rest = Rect::from_min_size(rest.min, vec2(rest.width(), rest.height().max(ROW_H)));
            let resp = ui.allocate_rect(rest, Sense::hover());
            if resp.dnd_hover_payload::<CuePayload>().is_some_and(|pl| pl.module == m) || resp.dnd_hover_payload::<TrackPayload>().is_some() {
                insert_line = Some(rest.top());
            }
            if let Some(pl) = resp.dnd_release_payload::<CuePayload>() {
                if pl.module == m {
                    moved = Some((pl.index, len));
                }
            }
            if let Some(track) = resp.dnd_release_payload::<TrackPayload>() {
                track_drop = Some((track.0.clone(), len));
            }
            if len == 0 {
                ui.painter().text(
                    rest.left_top() + vec2(6.0, 6.0),
                    Align2::LEFT_TOP,
                    tr("Glissez ici des fichiers, des dossiers ou des morceaux de playlist.", "Drop files, folders or playlist tracks here."),
                    FontId::proportional(13.0),
                    TEXT_DIM,
                );
            }
            if let Some(y) = insert_line {
                let x = ui.max_rect().x_range();
                ui.painter().hline(x, y, Stroke::new(2.0, EDIT));
            }
        });

        if let Some((from, to)) = moved {
            self.ctrl.move_cue(m, from, to);
        }
        if let Some((path, at)) = track_drop {
            self.ctrl.add_cues(m, vec![path], Some(at));
        }
        match action {
            Some((i, CueAction::Standby)) => self.ctrl.set_standby(m, i),
            Some((i, CueAction::Play)) => self.ctrl.play_cue(m, i),
            Some((i, CueAction::Stop)) => self.ctrl.stop_cue(m, i),
            Some((i, CueAction::Remove)) => self.ctrl.remove_cue(m, i),
            Some((i, CueAction::Volume(db))) => self.ctrl.set_cue_volume(m, i, db),
            Some((i, CueAction::Rename(name))) => self.ctrl.rename_cue(m, i, name),
            None => {}
        }
    }
}

enum CueAction {
    Standby,
    Play,
    Stop,
    Remove,
    Volume(f32),
    Rename(String),
}
