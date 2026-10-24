/// Son décodé en mémoire, stéréo entrelacé (L, R, L, R…), au taux du périphérique.
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub frames: usize,
    pub sample_rate: u32,
    /// Aperçu de la forme d'onde : crête (0..1) par tranche, pour l'éditeur.
    pub overview: Vec<f32>,
    /// Crête par tranche de `DETAIL_FRAMES` images, pour la forme d'onde zoomée.
    pub detail: Vec<f32>,
}

pub const OVERVIEW_BINS: usize = 1500;
pub const DETAIL_FRAMES: usize = 64;

impl AudioBuffer {
    pub fn from_planar(left: Vec<f32>, right: Vec<f32>, sample_rate: u32) -> Self {
        let frames = left.len().min(right.len());
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            samples.push(left[i]);
            samples.push(right[i]);
        }
        let mut overview = vec![0f32; OVERVIEW_BINS.min(frames.max(1))];
        let mut detail = vec![0f32; frames.div_ceil(DETAIL_FRAMES).max(1)];
        let bins = overview.len();
        for i in 0..frames {
            let b = i * bins / frames.max(1);
            let v = left[i].abs().max(right[i].abs());
            if v > overview[b] {
                overview[b] = v;
            }
            let d = &mut detail[i / DETAIL_FRAMES];
            *d = d.max(v);
        }
        Self { samples, frames, sample_rate, overview, detail }
    }

    /// Crêtes de `cols` colonnes couvrant `t0..t1` secondes, au niveau de détail qui suffit :
    /// aperçu global, tranches de 64 images, ou échantillons quand on est très zoomé.
    pub fn peaks(&self, t0: f64, t1: f64, cols: usize) -> Vec<f32> {
        let sr = self.sample_rate.max(1) as f64;
        let n = self.frames;
        if n == 0 || cols == 0 {
            return vec![0.0; cols];
        }
        let f0 = (t0 * sr).clamp(0.0, n as f64);
        let f1 = (t1 * sr).clamp(f0, n as f64);
        let per_col = (f1 - f0) / cols as f64;
        let bins = self.overview.len();
        let max = |s: &[f32]| s.iter().cloned().fold(0.0f32, f32::max);
        (0..cols)
            .map(|c| {
                let a = (f0 + c as f64 * per_col) as usize;
                let b = ((f0 + (c + 1) as f64 * per_col).ceil() as usize).clamp(a + 1, n.max(a + 1));
                if a >= n {
                    0.0
                } else if per_col >= (n / bins) as f64 * 2.0 {
                    max(&self.overview[a * bins / n..(b * bins).div_ceil(n).clamp(a * bins / n + 1, bins)])
                } else if per_col >= DETAIL_FRAMES as f64 {
                    let da = a / DETAIL_FRAMES;
                    max(&self.detail[da..b.div_ceil(DETAIL_FRAMES).clamp(da + 1, self.detail.len())])
                } else {
                    self.samples[a * 2..b.min(n) * 2].iter().fold(0.0f32, |m, v| m.max(v.abs()))
                }
            })
            .collect()
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames as f64 / self.sample_rate.max(1) as f64
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_match_samples_at_every_zoom() {
        // 10 s à 48 kHz, une seule crête de 0,9 à 4,5 s.
        let sr = 48_000;
        let mut l = vec![0.1f32; sr as usize * 10];
        l[sr as usize * 9 / 2] = -0.9;
        let b = AudioBuffer::from_planar(l.clone(), l, sr);
        for (t0, t1) in [(0.0, 10.0), (4.0, 5.0), (4.49, 4.51), (4.4999, 4.5001)] {
            let p = b.peaks(t0, t1, 400);
            assert_eq!(p.len(), 400);
            let max = p.iter().cloned().fold(0.0, f32::max);
            assert!((max - 0.9).abs() < 1e-6, "crête vue entre {t0} et {t1} : {max}");
        }
        assert!(b.peaks(0.0, 4.0, 300).iter().all(|&v| (v - 0.1).abs() < 1e-6));
    }
}
