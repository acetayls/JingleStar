//! Éditeur d'un son de JingleStar : celui de star-ui, nourri par le son chargé en mémoire
//! (crêtes de la forme d'onde, position de lecture en images).

use std::fmt::Debug;
use std::hash::Hash;

use egui::{RichText, Ui};

pub use star_ui::sound_editor::{listen_button, sound_rows};
use star_ui::sound_editor::WaveSource;
use star_ui::widgets::peaks_background;

use crate::audio::buffer::AudioBuffer;
use crate::project::SoundSettings;

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
    let rate = buf.map(|b| b.sample_rate as f64).unwrap_or(1.0);
    let wave = buf.map(|buf| WaveSource {
        duration: buf.duration_secs(),
        background: Box::new(peaks_background(|a, b, n| buf.peaks(a, b, n))),
        playhead: playhead.map(|p| p as f64 / rate),
    });
    star_ui::sound_editor::sound_waveform_and_eq(ui, id_salt, s, wave, status, eq_rate).map(|t| (t * rate) as usize)
}
