//! Volume et égaliseur d'un élément traduits en filtres son de mpv (filtres ffmpeg).
//!
//! Mêmes bandes que l'égaliseur de JingleStar (`star_core::eq`) : coupe-bas 12 ou 24 dB/oct,
//! shelf grave, deux cloches, shelf aigu. Les filtres ffmpeg suivent les mêmes formules (biquads
//! RBJ), la courbe affichée dans l'éditeur reste donc juste. Chaque filtre porte une étiquette :
//! un changement de fréquence, de gain ou de Q part par `af-command`, sans reconstruire la chaîne.

use star_core::eq::{BandKind, CutSlope, EqSettings, GAIN_RANGE, MIN_FREQ};
use star_core::sound::SoundSettings;

/// Fréquence d'échantillonnage de la sortie son (fixée dans `new_deck`).
pub const SAMPLE_RATE: f64 = 48_000.0;

/// Un filtre de la chaîne : étiquette, nom du filtre ffmpeg et paramètres.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub label: &'static str,
    pub filter: &'static str,
    pub params: Vec<(&'static str, String)>,
}

impl Stage {
    /// Entrée de la propriété mpv `af` : « @étiquette:lavfi-filtre=k=v:k=v ».
    fn entry(&self) -> String {
        let params: Vec<String> = self.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("@{}:lavfi-{}={}", self.label, self.filter, params.join(":"))
    }
}

fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// Valeur sûre : un NaN ou un infini retombe sur `d` (fichier abîmé).
fn finite(v: f32, d: f32) -> f64 {
    if v.is_finite() {
        v as f64
    } else {
        d as f64
    }
}

/// Filtres de l'égaliseur seul (vide s'il est à plat).
pub fn eq_stages(eq: &EqSettings) -> Vec<Stage> {
    let mut v = Vec::new();
    for k in BandKind::ALL {
        if !eq.band_active(k) {
            continue;
        }
        let b = eq.band(k);
        let f = finite(b.freq, 1000.0).min(SAMPLE_RATE * 0.45).max(MIN_FREQ as f64);
        let q = finite(b.q, 1.0).clamp(0.1, 18.0);
        let g = finite(b.gain_db, 0.0).clamp(-GAIN_RANGE as f64, GAIN_RANGE as f64);
        let shaped = |label, filter, q: f64, gain: Option<f64>| {
            let mut params = vec![("f", num(f)), ("t", "q".to_string()), ("w", num(q))];
            if let Some(g) = gain {
                params.push(("g", num(g)));
            } else {
                params.push(("p", "2".to_string()));
            }
            Stage { label, filter, params }
        };
        match k {
            BandKind::LowCut => match eq.low_cut_slope {
                CutSlope::Db12 => v.push(shaped("cs_lc1", "highpass", std::f64::consts::FRAC_1_SQRT_2, None)),
                // Butterworth d'ordre 4 : deux cellules aux Q de 0,54 et 1,31 (comme JingleStar).
                CutSlope::Db24 => {
                    v.push(shaped("cs_lc1", "highpass", 0.541_196, None));
                    v.push(shaped("cs_lc2", "highpass", 1.306_563, None));
                }
            },
            BandKind::LowShelf => v.push(shaped("cs_ls", "lowshelf", q, Some(g))),
            BandKind::Peak1 => v.push(shaped("cs_p1", "equalizer", q, Some(g))),
            BandKind::Peak2 => v.push(shaped("cs_p2", "equalizer", q, Some(g))),
            BandKind::HighShelf => v.push(shaped("cs_hs", "highshelf", q, Some(g))),
        }
    }
    v
}

/// Chaîne complète d'un élément : volume puis égaliseur.
pub fn stages(s: &SoundSettings) -> Vec<Stage> {
    let mut v = Vec::new();
    let db = finite(s.volume_db, 0.0).clamp(-60.0, 10.0);
    if db != 0.0 {
        v.push(Stage { label: "cs_vol", filter: "volume", params: vec![("volume", format!("{}dB", num(db)))] });
    }
    v.extend(eq_stages(&s.eq));
    v
}

/// Valeur de la propriété mpv `af` (vide : aucun filtre).
pub fn af_value(stages: &[Stage]) -> String {
    stages.iter().map(Stage::entry).collect::<Vec<_>>().join(",")
}

/// Commandes `af-command` (étiquette, paramètre, valeur, filtre ffmpeg visé) qui font passer la chaîne `old` à `new` sans la reconstruire,
/// ou None si les filtres ne sont plus les mêmes (bande allumée ou éteinte, pente changée).
pub fn live_commands(old: &[Stage], new: &[Stage]) -> Option<Vec<[String; 4]>> {
    if old.len() != new.len() || old.iter().zip(new).any(|(a, b)| a.label != b.label || a.filter != b.filter) {
        return None;
    }
    let mut cmds = Vec::new();
    for (a, b) in old.iter().zip(new) {
        for (k, v) in &b.params {
            let before = a.params.iter().find(|(ka, _)| ka == k).map(|(_, va)| va);
            if before != Some(v) {
                // La cible est nommée : envoyée à « all », la commande échoue sur les filtres
                // internes du graphe qui ne la connaissent pas.
                cmds.push([b.label.to_string(), k.to_string(), v.clone(), b.filter.to_string()]);
            }
        }
    }
    Some(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_settings_give_no_filter() {
        assert_eq!(af_value(&stages(&SoundSettings::default())), "");
    }

    #[test]
    fn bands_become_labelled_ffmpeg_filters() {
        let mut s = SoundSettings { volume_db: -6.0, ..Default::default() };
        s.eq.enabled = true;
        s.eq.low_cut.on = true;
        s.eq.low_cut.freq = 80.0;
        s.eq.low_cut_slope = CutSlope::Db24;
        s.eq.peak1.on = true;
        s.eq.peak1.freq = 2500.0;
        s.eq.peak1.gain_db = -3.5;
        s.eq.peak1.q = 2.0;
        let af = af_value(&stages(&s));
        assert_eq!(
            af,
            "@cs_vol:lavfi-volume=volume=-6dB,\
             @cs_lc1:lavfi-highpass=f=80:t=q:w=0.5412:p=2,\
             @cs_lc2:lavfi-highpass=f=80:t=q:w=1.3066:p=2,\
             @cs_p1:lavfi-equalizer=f=2500:t=q:w=2:g=-3.5"
        );
    }

    #[test]
    fn disabled_eq_keeps_only_the_volume() {
        let mut s = SoundSettings { volume_db: 3.0, ..Default::default() };
        s.eq.enabled = false;
        s.eq.high_shelf.on = true;
        s.eq.high_shelf.gain_db = 6.0;
        assert_eq!(af_value(&stages(&s)), "@cs_vol:lavfi-volume=volume=3dB");
    }

    #[test]
    fn insane_values_are_bounded() {
        let mut s = SoundSettings::default();
        s.eq.enabled = true;
        s.eq.peak2.on = true;
        s.eq.peak2.freq = f32::NAN;
        s.eq.peak2.gain_db = 99.0;
        s.eq.peak2.q = 0.0;
        let st = stages(&s);
        assert_eq!(st[0].params, vec![("f", "1000".to_string()), ("t", "q".into()), ("w", "0.1".into()), ("g", GAIN_RANGE.to_string())]);
    }

    #[test]
    fn same_filters_change_live() {
        let mut s = SoundSettings::default();
        s.eq.enabled = true;
        s.eq.peak1.on = true;
        s.eq.peak1.gain_db = 2.0;
        let a = stages(&s);
        s.eq.peak1.gain_db = 4.0;
        let b = stages(&s);
        assert_eq!(live_commands(&a, &b), Some(vec![["cs_p1".to_string(), "g".to_string(), "4".to_string(), "equalizer".to_string()]]));
        s.eq.peak2.on = true;
        s.eq.peak2.gain_db = 1.0;
        assert_eq!(live_commands(&b, &stages(&s)), None);
    }
}
