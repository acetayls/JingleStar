//! Réglages d'un son, communs à l'éditeur de jingle et à celui des cues de la Cuelist :
//! volume, lecture une fois ou en boucle, fondus, forme d'onde avec ses points, égaliseur.

use std::fmt::Debug;
use std::hash::Hash;

use egui::{vec2, Rect, RichText, Ui};

use super::eq_view;
use super::theme::*;
use super::widgets::*;
use crate::audio::buffer::AudioBuffer;
use crate::i18n::tr;
use crate::project::SoundSettings;

/// Rangées Volume, Lecture et Fondus, à placer dans une grille à deux colonnes.
/// Sans `allow_loop` (cues de la Cuelist), le choix une fois / en boucle n'est pas proposé.
pub fn sound_rows(ui: &mut Ui, s: &mut SoundSettings, allow_loop: bool) {
    ui.label("Volume");
    let r = ui.add(egui::Slider::new(&mut s.volume_db, -40.0..=10.0).suffix(" dB").step_by(0.5));
    let rail = Rect::from_min_size(r.rect.min, vec2(ui.spacing().slider_width, r.rect.height()));
    db_marks(ui, rail, false, &(-40.0..=10.0), false);
    ui.end_row();

    if allow_loop {
        ui.label(tr("Lecture", "Playback"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut s.looping, false, tr("Une fois", "Once"));
            ui.radio_value(&mut s.looping, true, tr("En boucle", "Loop"));
        });
        ui.end_row();
    }

    ui.label(tr("Fondus", "Fades"));
    ui.horizontal(|ui| {
        ui.label(tr("entrée", "in"));
        ui.add(egui::DragValue::new(&mut s.fade_in).range(0.0..=30.0).speed(0.05).suffix(" s").max_decimals(2));
        ui.label(tr("sortie", "out"));
        ui.add(egui::DragValue::new(&mut s.fade_out).range(0.0..=30.0).speed(0.05).suffix(" s").max_decimals(2));
    });
    ui.end_row();
}

/// Forme d'onde avec ses poignées, saisie précise des points, puis égaliseur.
/// `status` remplace la forme d'onde tant que le son n'est pas chargé.
/// Renvoie l'image où déplacer la lecture si l'on a cliqué dans la forme d'onde pendant la lecture.
pub fn sound_waveform_and_eq(
    ui: &mut Ui,
    id_salt: impl Hash + Debug + Copy,
    s: &mut SoundSettings,
    buf: Option<&AudioBuffer>,
    status: Option<RichText>,
    playhead: Option<u64>,
    eq_rate: u32,
) -> Option<usize> {
    let mut seek_to = None;
    if let Some(buf) = buf {
        let dur = buf.duration_secs();
        let (st, ls, le, end) = s.markers(dur);
        let mut mk = Markers { start: st, loop_start: ls, loop_end: le, end };
        let rate = buf.sample_rate as f64;
        let playhead = playhead.map(|p| p as f64 / rate);
        let mut seek = None;
        let edited = waveform_editor(ui, |a, b, n| buf.peaks(a, b, n), dur, &mut mk, s.looping, &mut s.fade_in, &mut s.fade_out, playhead, &mut seek);
        seek_to = seek.map(|t| (t * rate) as usize);
        if edited {
            set_markers(s, &mk);
        }
        // Saisie précise des points, en secondes.
        let before = mk;
        ui.horizontal(|ui| {
            let field = |ui: &mut Ui, label: &str, v: &mut f64| {
                ui.label(RichText::new(label).color(TEXT_DIM));
                ui.add(egui::DragValue::new(v).range(0.0..=dur).speed(0.01).max_decimals(2).suffix(" s"));
            };
            field(ui, tr("Début", "Start"), &mut mk.start);
            if s.looping {
                field(ui, tr("Boucle", "Loop"), &mut mk.loop_start);
                field(ui, "→", &mut mk.loop_end);
            }
            field(ui, tr("Fin", "End"), &mut mk.end);
        });
        if mk != before {
            mk.end = mk.end.max(mk.start);
            mk.loop_start = mk.loop_start.clamp(mk.start, mk.end);
            mk.loop_end = mk.loop_end.clamp(mk.loop_start, mk.end);
            set_markers(s, &mk);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!(
                "{} {}  ·  {} {}  ·  {} {}",
                tr("Début", "Start"),
                fmt_secs(mk.start),
                tr("Fin", "End"),
                fmt_secs(mk.end),
                tr("Durée", "Duration"),
                fmt_secs(mk.end - mk.start)
            )).monospace().color(TEXT_DIM));
            if s.looping {
                ui.label(RichText::new(format!("{} {} → {}", tr("Boucle", "Loop"), fmt_secs(mk.loop_start), fmt_secs(mk.loop_end))).monospace().color(ACCENT));
            }
            if ui.small_button(tr("Réinitialiser les points", "Reset points")).clicked() {
                s.start = 0.0;
                s.end = None;
                s.loop_start = None;
                s.loop_end = None;
            }
        });
    } else if let Some(st) = status {
        ui.label(st);
    }

    ui.add_space(6.0);
    let eq_title = if s.eq.is_flat() {
        RichText::new(tr("Égaliseur", "Equalizer"))
    } else {
        RichText::new(tr("Égaliseur (actif)", "Equalizer (on)")).color(ACCENT)
    };
    let open_by_default = !s.eq.is_flat();
    egui::CollapsingHeader::new(eq_title).id_salt(("sound-eq", id_salt)).default_open(open_by_default).show(ui, |ui| {
        eq_view::eq_editor(ui, ("sound", id_salt), &mut s.eq, eq_rate);
    });
    seek_to
}

fn set_markers(s: &mut SoundSettings, mk: &Markers) {
    s.start = mk.start;
    s.end = Some(mk.end);
    s.loop_start = Some(mk.loop_start);
    s.loop_end = Some(mk.loop_end);
}

fn fmt_secs(t: f64) -> String {
    let t = t.max(0.0);
    format!("{}:{:05.2}", (t / 60.0) as u64, t % 60.0)
}
