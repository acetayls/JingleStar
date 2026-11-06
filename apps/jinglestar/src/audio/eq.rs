//! Égaliseur : coupe-bas, shelf grave, deux bandes paramétriques, shelf aigu.
//!
//! Les réglages (`EqSettings`) sont sauvegardés dans le projet ; le contrôleur
//! les convertit en coefficients de biquads (`EqCoeffs`, formules RBJ) au taux
//! du périphérique et les envoie au mixeur, qui filtre sans allocation.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

/// Nombre maximal de cellules biquad : coupe-bas 24 dB/oct (2) + 4 bandes.
const MAX_STAGES: usize = 6;

pub const MIN_FREQ: f32 = 20.0;
pub const MAX_FREQ: f32 = 20_000.0;
pub const GAIN_RANGE: f32 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandKind {
    LowCut,
    LowShelf,
    Peak1,
    Peak2,
    HighShelf,
}

impl BandKind {
    pub const ALL: [BandKind; 5] = [BandKind::LowCut, BandKind::LowShelf, BandKind::Peak1, BandKind::Peak2, BandKind::HighShelf];

    /// Le coupe-bas n'a pas de gain : seule sa fréquence se règle.
    pub fn has_gain(self) -> bool {
        self != BandKind::LowCut
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Band {
    pub on: bool,
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Default for Band {
    fn default() -> Self {
        Self { on: false, freq: 1000.0, gain_db: 0.0, q: 1.0 }
    }
}

impl Band {
    const fn new(freq: f32, q: f32) -> Self {
        Self { on: false, freq, gain_db: 0.0, q }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CutSlope {
    #[default]
    #[serde(rename = "12")]
    Db12,
    #[serde(rename = "24")]
    Db24,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqSettings {
    /// Interrupteur général (bypass) : les bandes gardent leurs réglages.
    pub enabled: bool,
    pub low_cut: Band,
    pub low_cut_slope: CutSlope,
    pub low_shelf: Band,
    pub peak1: Band,
    pub peak2: Band,
    pub high_shelf: Band,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            low_cut: Band::new(80.0, std::f32::consts::FRAC_1_SQRT_2),
            low_cut_slope: CutSlope::Db12,
            low_shelf: Band::new(150.0, std::f32::consts::FRAC_1_SQRT_2),
            peak1: Band::new(400.0, 1.0),
            peak2: Band::new(2500.0, 1.0),
            high_shelf: Band::new(8000.0, std::f32::consts::FRAC_1_SQRT_2),
        }
    }
}

impl EqSettings {
    pub fn band(&self, k: BandKind) -> &Band {
        match k {
            BandKind::LowCut => &self.low_cut,
            BandKind::LowShelf => &self.low_shelf,
            BandKind::Peak1 => &self.peak1,
            BandKind::Peak2 => &self.peak2,
            BandKind::HighShelf => &self.high_shelf,
        }
    }

    pub fn band_mut(&mut self, k: BandKind) -> &mut Band {
        match k {
            BandKind::LowCut => &mut self.low_cut,
            BandKind::LowShelf => &mut self.low_shelf,
            BandKind::Peak1 => &mut self.peak1,
            BandKind::Peak2 => &mut self.peak2,
            BandKind::HighShelf => &mut self.high_shelf,
        }
    }

    /// Une bande modifie le son : coupe-bas allumé, ou gain non nul.
    pub fn band_active(&self, k: BandKind) -> bool {
        let b = self.band(k);
        self.enabled && b.on && (!k.has_gain() || b.gain_db.abs() > 0.01)
    }

    /// Vrai si l'EQ laisse passer le son sans le modifier.
    pub fn is_flat(&self) -> bool {
        !BandKind::ALL.iter().any(|&k| self.band_active(k))
    }

    /// Cellules biquad d'une bande au taux `sr` (vide si la bande est éteinte).
    pub fn band_stages(&self, k: BandKind, sr: f64) -> Vec<Biquad> {
        if !self.band_active(k) {
            return Vec::new();
        }
        let b = self.band(k);
        // Valeurs non finies (fichier abîmé, saisie) : on retombe sur des réglages sûrs,
        // sinon un NaN bloquerait le filtre pour toujours.
        let finite = |v: f32, d: f32| if v.is_finite() { v as f64 } else { d as f64 };
        let f = finite(b.freq, 1000.0).min(sr * 0.45).max(MIN_FREQ as f64);
        let q = finite(b.q, 1.0).clamp(0.1, 18.0);
        let g = finite(b.gain_db, 0.0).clamp(-GAIN_RANGE as f64, GAIN_RANGE as f64);
        match k {
            BandKind::LowCut => match self.low_cut_slope {
                CutSlope::Db12 => vec![Biquad::high_pass(f, std::f64::consts::FRAC_1_SQRT_2, sr)],
                // Butterworth d'ordre 4 : deux cellules aux Q de 0,54 et 1,31.
                CutSlope::Db24 => vec![Biquad::high_pass(f, 0.541_196, sr), Biquad::high_pass(f, 1.306_563, sr)],
            },
            BandKind::LowShelf => vec![Biquad::low_shelf(f, g, q, sr)],
            BandKind::Peak1 | BandKind::Peak2 => vec![Biquad::peak(f, g, q, sr)],
            BandKind::HighShelf => vec![Biquad::high_shelf(f, g, q, sr)],
        }
    }

    pub fn coeffs(&self, sample_rate: u32) -> EqCoeffs {
        let sr = sample_rate.max(1) as f64;
        let mut c = EqCoeffs::flat();
        for k in BandKind::ALL {
            for s in self.band_stages(k, sr) {
                if c.len < MAX_STAGES {
                    c.stages[c.len] = s;
                    c.len += 1;
                }
            }
        }
        c
    }
}

/// Cellule biquad normalisée (a0 = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    const IDENTITY: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    pub fn high_pass(f: f64, q: f64, sr: f64) -> Self {
        let w = 2.0 * PI * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        Self::normalized((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    pub fn peak(f: f64, gain_db: f64, q: f64, sr: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        Self::normalized(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
    }

    pub fn low_shelf(f: f64, gain_db: f64, q: f64, sr: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f / sr;
        let (s, c) = w.sin_cos();
        let k = 2.0 * a.sqrt() * s / (2.0 * q);
        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * c + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - k),
            (a + 1.0) + (a - 1.0) * c + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - k,
        )
    }

    pub fn high_shelf(f: f64, gain_db: f64, q: f64, sr: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f / sr;
        let (s, c) = w.sin_cos();
        let k = 2.0 * a.sqrt() * s / (2.0 * q);
        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * c + k),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - k),
            (a + 1.0) - (a - 1.0) * c + k,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - k,
        )
    }

    /// Gain (linéaire) de la cellule à la fréquence `f`.
    pub fn magnitude(&self, f: f64, sr: f64) -> f64 {
        let w = 2.0 * PI * f / sr;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let (nr, ni) = (self.b0 + self.b1 * c1 + self.b2 * c2, -(self.b1 * s1 + self.b2 * s2));
        let (dr, di) = (1.0 + self.a1 * c1 + self.a2 * c2, -(self.a1 * s1 + self.a2 * s2));
        ((nr * nr + ni * ni) / (dr * dr + di * di).max(1e-30)).sqrt()
    }
}

/// Coefficients prêts pour le thread audio (copiables, sans allocation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqCoeffs {
    stages: [Biquad; MAX_STAGES],
    len: usize,
}

impl EqCoeffs {
    pub const fn flat() -> Self {
        Self { stages: [Biquad::IDENTITY; MAX_STAGES], len: 0 }
    }

    pub fn is_flat(&self) -> bool {
        self.len == 0
    }

    /// Plus grand module des pôles : < 1 pour un filtre stable.
    #[cfg(test)]
    pub fn max_pole_radius(&self) -> f64 {
        self.stages[..self.len]
            .iter()
            .map(|s| {
                // Racines de z² + a1·z + a2.
                let disc = s.a1 * s.a1 - 4.0 * s.a2;
                if disc < 0.0 {
                    s.a2.sqrt()
                } else {
                    let r = disc.sqrt();
                    ((-s.a1 + r) / 2.0).abs().max(((-s.a1 - r) / 2.0).abs())
                }
            })
            .fold(0.0, f64::max)
    }

    /// Réponse totale en dB à la fréquence `f`.
    pub fn response_db(&self, f: f64, sr: f64) -> f64 {
        let g: f64 = self.stages[..self.len].iter().map(|s| s.magnitude(f, sr)).product();
        20.0 * g.max(1e-9).log10()
    }
}

/// Mémoire des filtres (forme transposée directe II), par cellule et par canal.
#[derive(Debug, Clone, Copy, Default)]
pub struct EqState {
    z: [[[f64; 2]; 2]; MAX_STAGES],
}

impl EqState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Filtre en place un bloc stéréo entrelacé.
    pub fn process(&mut self, c: &EqCoeffs, buf: &mut [f32]) {
        if c.len == 0 {
            return;
        }
        for frame in buf.chunks_exact_mut(2) {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample as f64;
                for (s, q) in c.stages[..c.len].iter().zip(self.z.iter_mut()) {
                    let z = &mut q[ch];
                    let y = s.b0 * x + z[0];
                    z[0] = s.b1 * x - s.a1 * y + z[1];
                    z[1] = s.b2 * x - s.a2 * y;
                    x = y;
                }
                *sample = x as f32;
            }
        }
        // Évite les nombres dénormalisés quand le signal s'éteint, et repart de zéro
        // si un échantillon non fini a empoisonné la mémoire du filtre.
        let mut finite = true;
        for q in self.z[..c.len].iter_mut().flatten().flatten() {
            finite &= q.is_finite();
            if q.abs() < 1e-20 {
                *q = 0.0;
            }
        }
        if !finite {
            self.reset();
        }
    }
}

/// Égaliseur temps réel : quand les coefficients changent, l'ancien et le nouveau
/// filtre tournent en parallèle le temps d'un court fondu enchaîné, pour qu'un
/// réglage pendant la lecture ne produise ni clic ni transitoire.
#[derive(Debug, Clone, Copy)]
pub struct EqProcessor {
    cur: EqCoeffs,
    cur_state: EqState,
    old: EqCoeffs,
    old_state: EqState,
    /// Réglage arrivé pendant un fondu, appliqué à la fin de celui-ci.
    pending: Option<EqCoeffs>,
    xfade_left: usize,
    xfade_len: usize,
}

impl EqProcessor {
    pub fn new(coeffs: EqCoeffs, xfade_len: usize) -> Self {
        Self {
            cur: coeffs,
            cur_state: EqState::default(),
            old: EqCoeffs::flat(),
            old_state: EqState::default(),
            pending: None,
            xfade_left: 0,
            xfade_len: xfade_len.max(1),
        }
    }

    /// Vrai si le signal passe sans traitement (EQ plat et aucun fondu en cours).
    pub fn is_bypassed(&self) -> bool {
        self.cur.is_flat() && self.xfade_left == 0
    }

    /// Nouveau réglage, appliqué en fondu.
    pub fn set(&mut self, c: EqCoeffs) {
        if self.xfade_left > 0 {
            self.pending = (c != self.cur).then_some(c);
        } else if c != self.cur {
            self.begin(c);
        }
    }

    /// Repart de zéro (nouveau son, module silencieux) sans fondu.
    pub fn reset(&mut self) {
        if let Some(c) = self.pending.take() {
            self.cur = c;
        }
        self.cur_state.reset();
        self.old_state.reset();
        self.xfade_left = 0;
    }

    fn begin(&mut self, c: EqCoeffs) {
        self.old = self.cur;
        self.old_state = self.cur_state;
        // Même structure de filtres : on garde la mémoire, le nouveau filtre démarre
        // presque en régime établi. Sinon il part de zéro et le fondu masque sa mise en route.
        if c.len != self.cur.len {
            self.cur_state.reset();
        }
        self.cur = c;
        self.xfade_left = self.xfade_len;
    }

    /// Filtre en place un bloc stéréo entrelacé ; `scratch` (même taille) sert au fondu.
    pub fn process(&mut self, buf: &mut [f32], scratch: &mut [f32]) {
        if self.xfade_left == 0 {
            self.cur_state.process(&self.cur, buf);
            return;
        }
        let scratch = &mut scratch[..buf.len()];
        scratch.copy_from_slice(buf);
        self.old_state.process(&self.old, scratch);
        self.cur_state.process(&self.cur, buf);
        let len = self.xfade_len as f32;
        for (frame, old) in buf.chunks_exact_mut(2).zip(scratch.chunks_exact(2)) {
            if self.xfade_left == 0 {
                break;
            }
            let t = 1.0 - self.xfade_left as f32 / len;
            frame[0] = old[0] + (frame[0] - old[0]) * t;
            frame[1] = old[1] + (frame[1] - old[1]) * t;
            self.xfade_left -= 1;
        }
        if self.xfade_left == 0 {
            self.old_state.reset();
            if self.cur.is_flat() {
                self.cur_state.reset();
            }
            if let Some(c) = self.pending.take() {
                self.begin(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn sine_gain_db(eq: &EqSettings, f: f64) -> f64 {
        let c = eq.coeffs(SR);
        let mut st = EqState::default();
        let n = SR as usize;
        let mut buf: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = (2.0 * PI * f * i as f64 / SR as f64).sin() as f32 * 0.5;
                [v, v]
            })
            .collect();
        st.process(&c, &mut buf);
        // Crête sur la seconde moitié (régime établi).
        let peak = buf[n..].iter().fold(0f32, |a, b| a.max(b.abs())) as f64;
        20.0 * (peak / 0.5).log10()
    }

    #[test]
    fn default_is_flat() {
        let eq = EqSettings::default();
        assert!(eq.is_flat());
        assert!(eq.coeffs(SR).is_flat());
    }

    #[test]
    fn peak_boosts_its_frequency() {
        let mut eq = EqSettings::default();
        eq.peak1 = Band { on: true, freq: 1000.0, gain_db: 6.0, q: 1.0 };
        assert!((sine_gain_db(&eq, 1000.0) - 6.0).abs() < 0.2);
        assert!(sine_gain_db(&eq, 50.0).abs() < 0.3);
        assert!((eq.coeffs(SR).response_db(1000.0, SR as f64) - 6.0).abs() < 0.01);
    }

    #[test]
    fn low_cut_removes_bass() {
        let mut eq = EqSettings::default();
        eq.low_cut.on = true;
        eq.low_cut.freq = 200.0;
        let g12 = sine_gain_db(&eq, 50.0);
        eq.low_cut_slope = CutSlope::Db24;
        let g24 = sine_gain_db(&eq, 50.0);
        assert!(g12 < -20.0, "12 dB/oct : {g12}");
        assert!(g24 < g12 - 15.0, "24 dB/oct : {g24}");
        assert!(sine_gain_db(&eq, 5000.0).abs() < 0.2);
        // -3 dB à la fréquence de coupure (Butterworth).
        assert!((eq.coeffs(SR).response_db(200.0, SR as f64) + 3.01).abs() < 0.1);
    }

    #[test]
    fn shelves_reach_their_gain() {
        let mut eq = EqSettings::default();
        eq.low_shelf = Band { on: true, freq: 200.0, gain_db: -6.0, q: 0.707 };
        eq.high_shelf = Band { on: true, freq: 5000.0, gain_db: 4.0, q: 0.707 };
        let c = eq.coeffs(SR);
        assert!((c.response_db(25.0, SR as f64) + 6.0).abs() < 0.3);
        assert!((c.response_db(18_000.0, SR as f64) - 4.0).abs() < 0.3);
        assert!(c.response_db(1000.0, SR as f64).abs() < 1.0);
    }

    #[test]
    fn bypass_disables_everything() {
        let mut eq = EqSettings::default();
        eq.peak2 = Band { on: true, freq: 3000.0, gain_db: 9.0, q: 2.0 };
        assert!(!eq.is_flat());
        eq.enabled = false;
        assert!(eq.is_flat());
    }
}
