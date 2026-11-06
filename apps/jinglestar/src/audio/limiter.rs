//! Limiteur de sécurité sur les sorties, juste avant la conversion vers le pilote.
//!
//! Tant que le signal reste sous 0 dBFS, il est rendu à l'identique (simplement
//! retardé de la prévision). Quand la somme des sons dépasse, le gain baisse en
//! douceur juste avant la crête au lieu d'écrêter : pas de saturation numérique.
//!
//! Le gain est calculé par paire de sorties (1-2, 3-4, …) : chaque paire porte
//! un programme indépendant. Les tampons sont alloués à la création ; `process`
//! n'alloue rien.

/// Niveau maximal en sortie (0 dBFS).
pub const CEILING: f32 = 1.0;
const LOOKAHEAD_SECS: f32 = 0.0015;
const RELEASE_SECS: f32 = 0.080;

struct Group {
    first: usize,
    count: usize,
    /// Minimum glissant des gains requis : file monotone (position, gain).
    min_pos: Vec<u64>,
    min_val: Vec<f32>,
    min_head: usize,
    min_len: usize,
    /// Moyenne glissante du minimum, pour une attaque sans marche.
    box_hist: Vec<f32>,
    box_sum: f64,
    /// Nombre de valeurs < 1 dans la moyenne : à zéro, le gain vaut exactement 1.
    box_reducing: usize,
    gain: f32,
}

pub struct Limiter {
    channels: usize,
    lookahead: usize,
    release: f32,
    groups: Vec<Group>,
    /// Ligne de retard entrelacée (`lookahead` images).
    delay: Vec<f32>,
    pos: u64,
}

impl Limiter {
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        let channels = channels.max(1);
        let sr = sample_rate.max(1) as f32;
        let lookahead = ((LOOKAHEAD_SECS * sr) as usize).max(1);
        let groups = (0..channels.div_ceil(2))
            .map(|g| Group {
                first: g * 2,
                count: (channels - g * 2).min(2),
                min_pos: vec![0; lookahead],
                min_val: vec![1.0; lookahead],
                min_head: 0,
                min_len: 0,
                box_hist: vec![1.0; lookahead],
                box_sum: lookahead as f64,
                box_reducing: 0,
                gain: 1.0,
            })
            .collect();
        Self {
            channels,
            lookahead,
            release: 1.0 - (-1.0 / (RELEASE_SECS * sr)).exp(),
            groups,
            delay: vec![0.0; lookahead * channels],
            pos: 0,
        }
    }

    /// Retard introduit, en images.
    #[cfg(test)]
    pub fn latency(&self) -> usize {
        self.lookahead - 1
    }

    /// Traite en place un bloc entrelacé de `channels` canaux.
    pub fn process(&mut self, buf: &mut [f32]) {
        let ch = self.channels;
        let la = self.lookahead;
        for frame in buf.chunks_exact_mut(ch) {
            let n = self.pos;
            let slot = (n % la as u64) as usize;
            // Échantillon qui sort de la ligne de retard (entré il y a `la - 1` images).
            let out_slot = ((n + 1) % la as u64) as usize;
            for s in frame.iter_mut() {
                if !s.is_finite() {
                    *s = 0.0;
                }
            }
            for g in self.groups.iter_mut() {
                let peak = frame[g.first..g.first + g.count].iter().fold(0f32, |a, s| a.max(s.abs()));
                // Légère marge pour que l'arrondi du produit ne dépasse pas le plafond.
                let required = if peak > CEILING { CEILING / peak * 0.999_999 } else { 1.0 };

                // Minimum des gains requis sur les `la` dernières images (file monotone :
                // on retire d'abord ce qui sort de la fenêtre, pour ne jamais dépasser `la`).
                while g.min_len > 0 && g.min_pos[g.min_head] + la as u64 <= n {
                    g.min_head = (g.min_head + 1) % la;
                    g.min_len -= 1;
                }
                while g.min_len > 0 {
                    let back = (g.min_head + g.min_len - 1) % la;
                    if g.min_val[back] >= required {
                        g.min_len -= 1;
                    } else {
                        break;
                    }
                }
                let back = (g.min_head + g.min_len) % la;
                g.min_pos[back] = n;
                g.min_val[back] = required;
                g.min_len += 1;
                let held = g.min_val[g.min_head];

                // Moyenne glissante : chaque terme est ≤ au gain requis par l'échantillon
                // qui sort, la moyenne aussi, donc la sortie ne dépasse jamais le plafond.
                let old = g.box_hist[slot];
                g.box_reducing -= (old < 1.0) as usize;
                g.box_reducing += (held < 1.0) as usize;
                g.box_hist[slot] = held;
                g.box_sum += held as f64 - old as f64;
                let env = if g.box_reducing == 0 {
                    g.box_sum = la as f64;
                    1.0
                } else {
                    ((g.box_sum / la as f64) as f32).min(1.0)
                };

                // Relâchement progressif, jamais au-dessus de l'enveloppe.
                // Le petit terme linéaire finit le retour à 1 exactement, en un temps borné.
                let released = (g.gain + (1.0 - g.gain) * self.release + self.release * 0.01).min(1.0);
                g.gain = released.min(env);

                for c in g.first..g.first + g.count {
                    self.delay[slot * ch + c] = frame[c];
                    let delayed = self.delay[out_slot * ch + c];
                    frame[c] = if g.gain == 1.0 { delayed } else { delayed * g.gain };
                }
            }
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_below_ceiling() {
        let mut lim = Limiter::new(2, 48_000);
        let input: Vec<f32> = (0..9600).map(|i| (i as f32 * 0.0131).sin() * 0.999).collect();
        let mut buf = input.clone();
        for chunk in buf.chunks_mut(256) {
            lim.process(chunk);
        }
        let d = lim.latency() * 2;
        assert!(buf[..d].iter().all(|&s| s == 0.0));
        assert_eq!(&buf[d..], &input[..input.len() - d], "identique au bit près, simplement retardé");
    }

    #[test]
    fn never_exceeds_ceiling_and_no_step() {
        let mut lim = Limiter::new(2, 48_000);
        // Sinus à 100 Hz qui monte brusquement de 0,5 à 4 (+18 dB).
        let input: Vec<f32> = (0..72_000)
            .flat_map(|i| {
                let a = if (12_000..36_000).contains(&i) { 4.0 } else { 0.5 };
                let v = (i as f32 * 100.0 * std::f32::consts::TAU / 48_000.0).sin() * a;
                [v, v]
            })
            .collect();
        let mut buf = input.clone();
        for chunk in buf.chunks_mut(512) {
            lim.process(chunk);
        }
        let peak = buf.iter().fold(0f32, |a, s| a.max(s.abs()));
        assert!(peak <= CEILING + 1e-6, "crête {peak}");
        // Pas de marche : variation entre deux images bornée (sinus 100 Hz à 1,0 ≈ 0,013, une coupure franche ≈ 0,5).
        let step = buf.chunks_exact(2).collect::<Vec<_>>().windows(2).fold(0f32, |a, w| a.max((w[1][0] - w[0][0]).abs()));
        assert!(step < 0.05, "saut {step}");
        // Après le relâchement, retour exact au signal d'origine.
        let d = lim.latency();
        for i in 64_000..72_000 {
            assert_eq!(buf[i * 2], input[(i - d) * 2]);
        }
    }

    #[test]
    fn pairs_are_independent_and_nan_is_silenced() {
        let mut lim = Limiter::new(4, 48_000);
        let mut buf: Vec<f32> = (0..4800).flat_map(|_| [3.0, -3.0, 0.25, f32::NAN]).collect();
        lim.process(&mut buf);
        let tail = &buf[4000 * 4..];
        assert!(tail.chunks_exact(4).all(|f| f[0].abs() <= CEILING && f[2] == 0.25 && f[3] == 0.0));
    }
}
