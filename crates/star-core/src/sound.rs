//! Réglages d'un son (jingle, cue, élément de playlist) : volume, boucle, points d'édition,
//! fondus et égaliseur. Repris de JingleStar ; le format JSON ne change pas.

use serde::{Deserialize, Serialize};

use crate::eq::EqSettings;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SoundSettings {
    pub volume_db: f32,
    /// Lecture en boucle entre `loop_start` et `loop_end`.
    pub looping: bool,
    /// Points d'édition en secondes (poignées de début et de fin, traits de boucle).
    pub start: f64,
    pub end: Option<f64>,
    pub loop_start: Option<f64>,
    pub loop_end: Option<f64>,
    /// Fondus d'entrée et de sortie, en secondes.
    pub fade_in: f64,
    pub fade_out: f64,
    pub eq: EqSettings,
}

impl SoundSettings {
    /// Points effectifs pour un son de `duration` secondes : (début, début de boucle, fin de boucle, fin).
    pub fn markers(&self, duration: f64) -> (f64, f64, f64, f64) {
        let end = self.end.unwrap_or(duration).clamp(0.0, duration);
        let start = self.start.clamp(0.0, end);
        let ls = self.loop_start.unwrap_or(start).clamp(start, end);
        let le = self.loop_end.unwrap_or(end).clamp(ls, end);
        (start, ls, le, end)
    }

    /// Remet dans leurs bornes les valeurs aberrantes (NaN, infini, hors limites) : un projet
    /// modifié à la main ou abîmé ne doit ni rendre un son muet ni le faire hurler.
    /// `max_db` : volume le plus fort accepté.
    pub fn sanitize(&mut self, max_db: f32) {
        crate::num::clamp_f32(&mut self.volume_db, 0.0, -60.0, max_db);
        crate::num::secs(&mut self.start);
        crate::num::opt_secs(&mut self.end);
        crate::num::opt_secs(&mut self.loop_start);
        crate::num::opt_secs(&mut self.loop_end);
        crate::num::secs(&mut self.fade_in);
        crate::num::secs(&mut self.fade_out);
        self.eq.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_values_in_range() {
        let mut s = SoundSettings { volume_db: f32::NAN, start: -3.0, end: Some(f64::INFINITY), fade_in: 1e9, ..Default::default() };
        s.eq.peak1.gain_db = 99.0;
        s.sanitize(10.0);
        assert_eq!(s.volume_db, 0.0);
        assert_eq!(s.start, 0.0);
        assert_eq!(s.end, None);
        assert_eq!(s.fade_in, 86_400.0);
        assert_eq!(s.eq.peak1.gain_db, crate::eq::GAIN_RANGE);
    }
}
