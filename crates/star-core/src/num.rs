//! Garde-fous des valeurs lues dans un fichier (projet modifié à la main ou abîmé).

/// `v` ramené dans `min..=max`, ou `default` s'il n'est pas un nombre fini.
pub fn clamp_f32(v: &mut f32, default: f32, min: f32, max: f32) {
    *v = if v.is_finite() { v.clamp(min, max) } else { default };
}

/// Durée en secondes : de 0 à 24 h, 0 si elle n'est pas un nombre fini.
pub fn secs(v: &mut f64) {
    *v = if v.is_finite() { v.clamp(0.0, 86_400.0) } else { 0.0 };
}

/// Instant facultatif : retiré s'il est négatif ou n'est pas un nombre fini.
pub fn opt_secs(v: &mut Option<f64>) {
    if v.is_some_and(|x| !x.is_finite() || x < 0.0) {
        *v = None;
    }
}
