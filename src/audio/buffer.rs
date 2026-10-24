/// Son décodé en mémoire, stéréo entrelacé (L, R, L, R…), au taux du périphérique.
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub frames: usize,
    pub sample_rate: u32,
    /// Aperçu de la forme d'onde : crête (0..1) par tranche, pour l'éditeur.
    pub overview: Vec<f32>,
}

pub const OVERVIEW_BINS: usize = 1500;

impl AudioBuffer {
    pub fn from_planar(left: Vec<f32>, right: Vec<f32>, sample_rate: u32) -> Self {
        let frames = left.len().min(right.len());
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            samples.push(left[i]);
            samples.push(right[i]);
        }
        let mut overview = vec![0f32; OVERVIEW_BINS.min(frames.max(1))];
        let bins = overview.len();
        for i in 0..frames {
            let b = i * bins / frames.max(1);
            let v = left[i].abs().max(right[i].abs());
            if v > overview[b] {
                overview[b] = v;
            }
        }
        Self { samples, frames, sample_rate, overview }
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
