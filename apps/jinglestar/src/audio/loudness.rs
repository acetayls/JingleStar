//! Mesure du niveau des morceaux, pour équilibrer la playlist (comme « Normalize items » de REAPER).
//!
//! - Sonie intégrée en LUFS selon ITU-R BS.1770-4 / EBU R128 : pondération K,
//!   blocs de 400 ms avec 75 % de recouvrement, porte absolue à -70 LUFS puis
//!   porte relative à -10 LU.
//! - Crête échantillon (dBFS), crête vraie (dBTP, suréchantillonnage ×4 par
//!   sinus cardinal fenêtré) et RMS (dBFS).
//!
//! La mesure se fait hors du thread audio, sur le son entier décodé.

use std::f64::consts::PI;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

/// Plancher des mesures en dB, pour un son muet (JSON ne connaît pas -∞).
pub const FLOOR_DB: f32 = -120.0;

/// Résultat de la mesure d'un morceau.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Loudness {
    /// Sonie intégrée (LUFS) ; `None` si le son est trop court ou muet.
    pub integrated: Option<f32>,
    /// Crête échantillon (dBFS).
    pub peak: f32,
    /// Crête vraie (dBTP).
    pub true_peak: f32,
    /// Valeur efficace sur tout le son, deux canaux confondus (dBFS).
    pub rms: f32,
}

/// Grandeur visée par la normalisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NormalizeMode {
    #[default]
    Lufs,
    Peak,
    TruePeak,
    Rms,
}

impl NormalizeMode {
    pub const ALL: [NormalizeMode; 4] = [NormalizeMode::Lufs, NormalizeMode::Peak, NormalizeMode::TruePeak, NormalizeMode::Rms];

    pub fn unit(self) -> &'static str {
        match self {
            NormalizeMode::Lufs => "LUFS",
            NormalizeMode::Peak => "dBFS",
            NormalizeMode::TruePeak => "dBTP",
            NormalizeMode::Rms => "dBFS",
        }
    }
}

/// Réglages de normalisation d'une playlist. Les fichiers ne sont jamais modifiés :
/// seul le gain de lecture de chaque morceau change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NormalizeSettings {
    pub enabled: bool,
    pub mode: NormalizeMode,
    /// Cibles par grandeur, gardées séparément pour passer de l'une à l'autre.
    pub target_lufs: f32,
    pub target_peak: f32,
    pub target_true_peak: f32,
    pub target_rms: f32,
    /// Plafonne le gain pour que la crête vraie ne dépasse pas `ceiling` (dBTP).
    pub limit: bool,
    pub ceiling: f32,
}

impl Default for NormalizeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: NormalizeMode::Lufs,
            target_lufs: -16.0,
            target_peak: -1.0,
            target_true_peak: -1.0,
            target_rms: -18.0,
            limit: true,
            ceiling: -1.0,
        }
    }
}

/// Plage du gain appliqué, par sécurité (morceau presque muet, mesure aberrante).
const MAX_CUT_DB: f32 = -60.0;
const MAX_BOOST_DB: f32 = 30.0;

impl NormalizeSettings {
    pub fn target(&self) -> f32 {
        match self.mode {
            NormalizeMode::Lufs => self.target_lufs,
            NormalizeMode::Peak => self.target_peak,
            NormalizeMode::TruePeak => self.target_true_peak,
            NormalizeMode::Rms => self.target_rms,
        }
    }

    pub fn target_mut(&mut self) -> &mut f32 {
        match self.mode {
            NormalizeMode::Lufs => &mut self.target_lufs,
            NormalizeMode::Peak => &mut self.target_peak,
            NormalizeMode::TruePeak => &mut self.target_true_peak,
            NormalizeMode::Rms => &mut self.target_rms,
        }
    }

    /// Gain à appliquer au morceau (dB), et s'il a été réduit par la limite de crête vraie.
    /// `None` si la mesure ne permet pas de le calculer (son muet ou trop court).
    pub fn gain_db(&self, l: &Loudness) -> Option<(f32, bool)> {
        let measured = match self.mode {
            NormalizeMode::Lufs => l.integrated?,
            NormalizeMode::Peak => l.peak,
            NormalizeMode::TruePeak => l.true_peak,
            NormalizeMode::Rms => l.rms,
        };
        if measured <= FLOOR_DB {
            return None;
        }
        let wanted = (self.target() - measured).clamp(MAX_CUT_DB, MAX_BOOST_DB);
        if self.limit && l.true_peak > FLOOR_DB {
            let max = self.ceiling - l.true_peak;
            if max < wanted {
                return Some((max.max(MAX_CUT_DB), true));
            }
        }
        Some((wanted, false))
    }
}

fn to_db(power: f64) -> f32 {
    if power <= 1e-12 {
        FLOOR_DB
    } else {
        ((10.0 * power.log10()) as f32).max(FLOOR_DB)
    }
}

/// Cellule biquad (forme directe I), en f64 pour la précision sur de longs fichiers.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self { b, a, x: [0.0; 2], y: [0.0; 2] }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Filtre de pondération K (shelf aigu + passe-haut) recalculé pour tout taux
/// d'échantillonnage ; à 48 kHz il redonne les coefficients de la norme.
fn k_weighting(rate: f64) -> [Biquad; 2] {
    // Étage 1 : shelf aigu (+4 dB).
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / rate).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad::new(
        [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
    );
    // Étage 2 : passe-haut (RLB).
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = Biquad::new([1.0, -2.0, 1.0], [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0]);
    [shelf, hp]
}

/// Échantillons par phase du filtre d'interpolation de la crête vraie.
const TP_TAPS: usize = 16;

/// Détecteur de crête vraie : interpole `factor - 1` points entre chaque échantillon.
struct TruePeak {
    factor: usize,
    /// Coefficients par phase (1..factor), `TP_TAPS` chacun.
    coefs: Vec<[f64; TP_TAPS]>,
    /// Historique doublé (écrit deux fois) pour lire une tranche contiguë.
    hist: [[f64; TP_TAPS * 2]; 2],
    w: usize,
    max: f64,
}

impl TruePeak {
    fn new(rate: f64) -> Self {
        // ×4 jusqu'à 48 kHz, ×2 jusqu'à 96 kHz, rien au-delà (BS.1770-4, annexe 2).
        let factor = ((192_000.0 / rate).ceil() as usize).clamp(1, 4);
        let half = (factor * TP_TAPS / 2) as f64;
        let coefs = (1..factor)
            .map(|p| {
                std::array::from_fn(|i| {
                    let d = (factor * (TP_TAPS / 2 - 1)) as f64 - (factor * i) as f64 + p as f64;
                    let x = d / factor as f64;
                    let sinc = if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
                    let u = d / half;
                    let blackman = 0.42 + 0.5 * (PI * u).cos() + 0.08 * (2.0 * PI * u).cos();
                    sinc * blackman
                })
            })
            .collect();
        Self { factor, coefs, hist: [[0.0; TP_TAPS * 2]; 2], w: 0, max: 0.0 }
    }

    fn push(&mut self, frame: [f64; 2]) {
        for (c, &x) in frame.iter().enumerate() {
            self.max = self.max.max(x.abs());
            self.hist[c][self.w] = x;
            self.hist[c][self.w + TP_TAPS] = x;
        }
        self.w = (self.w + 1) % TP_TAPS;
        if self.factor < 2 {
            return;
        }
        // Les TP_TAPS derniers échantillons, du plus ancien au plus récent.
        for c in 0..2 {
            let h = &self.hist[c][self.w..self.w + TP_TAPS];
            for coef in &self.coefs {
                let y: f64 = h.iter().zip(coef.iter()).map(|(a, b)| a * b).sum();
                self.max = self.max.max(y.abs());
            }
        }
    }

    fn finish(&mut self) -> f64 {
        // Vide le filtre pour interpoler jusqu'au dernier échantillon.
        for _ in 0..TP_TAPS / 2 {
            if self.factor >= 2 {
                self.push([0.0, 0.0]);
            }
        }
        self.max
    }
}

/// Mesure en flux : on pousse les images stéréo une à une, puis `finish`.
pub struct Meter {
    k: [[Biquad; 2]; 2],
    /// Énergie pondérée K de chaque tranche de 100 ms (somme des deux canaux).
    segments: Vec<f64>,
    seg_len: usize,
    seg_fill: usize,
    seg_sum: f64,
    sum_sq: f64,
    frames: u64,
    peak: f64,
    tp: TruePeak,
}

impl Meter {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(1) as f64;
        let k = k_weighting(rate);
        Self {
            k: [k, k],
            segments: Vec::new(),
            seg_len: ((rate * 0.1).round() as usize).max(1),
            seg_fill: 0,
            seg_sum: 0.0,
            sum_sq: 0.0,
            frames: 0,
            peak: 0.0,
            tp: TruePeak::new(rate),
        }
    }

    pub fn push(&mut self, l: f32, r: f32) {
        let frame = [l as f64, r as f64];
        let mut e = 0.0;
        for (c, &x) in frame.iter().enumerate() {
            let shelf = self.k[c][0].process(x);
            let y = self.k[c][1].process(shelf);
            e += y * y;
            self.sum_sq += x * x;
            self.peak = self.peak.max(x.abs());
        }
        self.tp.push(frame);
        self.frames += 1;
        self.seg_sum += e;
        self.seg_fill += 1;
        if self.seg_fill == self.seg_len {
            self.segments.push(self.seg_sum);
            self.seg_sum = 0.0;
            self.seg_fill = 0;
        }
    }

    pub fn finish(mut self) -> Loudness {
        let tp = self.tp.finish().max(self.peak);
        let rms = if self.frames == 0 { 0.0 } else { self.sum_sq / (self.frames as f64 * 2.0) };
        Loudness {
            integrated: integrated(&self.segments, self.seg_len),
            peak: to_db(self.peak * self.peak),
            true_peak: to_db(tp * tp),
            rms: to_db(rms),
        }
    }
}

/// Sonie d'un bloc d'énergie moyenne `z` (somme des canaux, poids 1 pour G et D).
fn lufs(z: f64) -> f64 {
    -0.691 + 10.0 * z.log10()
}

/// Sonie intégrée à double porte, à partir des tranches de 100 ms.
fn integrated(segments: &[f64], seg_len: usize) -> Option<f32> {
    if segments.len() < 4 {
        return None;
    }
    // Blocs de 400 ms (4 tranches), avancés de 100 ms : 75 % de recouvrement.
    let blocks: Vec<f64> = segments.windows(4).map(|w| w.iter().sum::<f64>() / (4 * seg_len) as f64).collect();
    let above = |gate: f64| -> Vec<f64> { blocks.iter().copied().filter(|&z| z > 0.0 && lufs(z) > gate).collect() };
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let abs = above(-70.0);
    if abs.is_empty() {
        return None;
    }
    let relative = lufs(mean(&abs)) - 10.0;
    let gated: Vec<f64> = abs.into_iter().filter(|&z| lufs(z) > relative).collect();
    (!gated.is_empty()).then(|| lufs(mean(&gated)) as f32)
}

/// Mesure un son stéréo entrelacé (L, R, L, R…).
#[cfg(test)]
pub fn analyze_interleaved(samples: &[f32], rate: u32) -> Loudness {
    let mut m = Meter::new(rate);
    for f in samples.chunks_exact(2) {
        m.push(f[0], f[1]);
    }
    m.finish()
}

/// Version de l'algorithme : un changement invalide les mesures en cache.
const CACHE_VERSION: u32 = 1;

/// Cache disque des mesures, indexé par chemin et invalidé si le fichier change
/// (taille ou date de modification).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LoudnessCache {
    version: u32,
    entries: HashMap<PathBuf, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    size: u64,
    modified: u64,
    loudness: Loudness,
}

/// Empreinte d'un fichier : (taille, date de modification en ms).
fn stamp(path: &Path) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let modified = md.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
    Some((md.len(), modified))
}

impl LoudnessCache {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .filter(|c| c.version == CACHE_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string(&Self { version: CACHE_VERSION, entries: self.entries.clone() })?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Mesure en cache, si le fichier n'a pas changé depuis.
    pub fn get(&self, path: &Path) -> Option<Loudness> {
        let e = self.entries.get(path)?;
        (stamp(path)? == (e.size, e.modified)).then_some(e.loudness)
    }

    pub fn insert(&mut self, path: &Path, loudness: Loudness) {
        if let Some((size, modified)) = stamp(path) {
            self.entries.insert(path.to_path_buf(), CacheEntry { size, modified, loudness });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f64, freq: f64, amp: f64, phase: f64) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let s = (amp * (2.0 * PI * freq * i as f64 / rate as f64 + phase).sin()) as f32;
                [s, s]
            })
            .collect()
    }

    fn db(v: f64) -> f64 {
        10f64.powf(v / 20.0)
    }

    #[test]
    fn ebu_1khz_sine_reads_minus_23() {
        // EBU Tech 3341, cas 1 : sinus 1 kHz à -23 dBFS sur les deux canaux = -23,0 LUFS.
        for rate in [44_100, 48_000] {
            let l = analyze_interleaved(&sine(rate, 5.0, 1000.0, db(-23.0), 0.0), rate);
            let i = l.integrated.unwrap();
            assert!((i + 23.0).abs() < 0.1, "{rate} Hz : {i} LUFS");
            assert!((l.peak + 23.0).abs() < 0.01);
            assert!((l.rms + 26.01).abs() < 0.02, "RMS {}", l.rms);
        }
    }

    #[test]
    fn relative_gate_ignores_quiet_parts() {
        // EBU Tech 3341, cas 3 : -36 / -23 / -36 dBFS = -23 LUFS (les passages calmes sont exclus).
        let rate = 48_000;
        let mut s = sine(rate, 3.0, 1000.0, db(-36.0), 0.0);
        s.extend(sine(rate, 18.0, 1000.0, db(-23.0), 0.0));
        s.extend(sine(rate, 3.0, 1000.0, db(-36.0), 0.0));
        let i = analyze_interleaved(&s, rate).integrated.unwrap();
        assert!((i + 23.0).abs() < 0.1, "{i} LUFS");
    }

    #[test]
    fn silence_has_no_loudness() {
        let l = analyze_interleaved(&vec![0.0; 48_000 * 2], 48_000);
        assert_eq!(l.integrated, None);
        assert_eq!(l.peak, FLOOR_DB);
        assert_eq!(NormalizeSettings::default().gain_db(&l), None);
    }

    #[test]
    fn true_peak_finds_inter_sample_peaks() {
        // Sinus à fs/4 décalé de 45° : les échantillons tombent à ±0,707, la vraie crête est à 1.
        let rate = 48_000;
        let l = analyze_interleaved(&sine(rate, 1.0, 12_000.0, 0.5, PI / 4.0), rate);
        assert!((l.peak - (-9.03)).abs() < 0.05, "crête {}", l.peak);
        assert!((l.true_peak - (-6.02)).abs() < 0.3, "crête vraie {}", l.true_peak);
    }

    #[test]
    fn gain_reaches_target_and_respects_ceiling() {
        let quiet = Loudness { integrated: Some(-26.0), peak: -12.0, true_peak: -11.5, rms: -29.0 };
        let mut n = NormalizeSettings { enabled: true, ..Default::default() };
        n.target_lufs = -16.0;
        assert_eq!(n.gain_db(&quiet), Some((10.0, false)));
        // Morceau très dynamique : +10 dB pousserait la crête vraie à -0,5 dBTP > -1.
        let spiky = Loudness { true_peak: -1.5, ..quiet };
        assert_eq!(n.gain_db(&spiky), Some((0.5, true)));
        n.limit = false;
        assert_eq!(n.gain_db(&spiky), Some((10.0, false)));
        n.mode = NormalizeMode::TruePeak;
        n.limit = true;
        assert_eq!(n.gain_db(&spiky), Some((0.5, false)));
    }

    #[test]
    fn cache_is_invalidated_when_file_changes() {
        let dir = std::env::temp_dir().join(format!("jinglestar-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let track = dir.join("a.wav");
        std::fs::write(&track, b"1234").unwrap();
        let l = Loudness { integrated: Some(-20.0), peak: -3.0, true_peak: -2.5, rms: -22.0 };
        let mut c = LoudnessCache::default();
        c.insert(&track, l);
        let file = dir.join("cache.json");
        c.save(&file).unwrap();
        let c = LoudnessCache::load(&file);
        assert_eq!(c.get(&track), Some(l));
        std::fs::write(&track, b"123456").unwrap();
        assert_eq!(c.get(&track), None, "taille changée");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
