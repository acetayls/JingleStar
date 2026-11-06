//! Déroulé du diaporama, tenu par le moteur de sortie : le minutage des photos ne dépend
//! pas de la régie.

use crate::protocol::Fit;
use std::time::{Duration, Instant};

const VIDEO_EXT: &[&str] = &["mp4", "mov", "mkv", "avi", "webm", "m4v", "mxf", "mpg", "mpeg", "ts", "wmv", "gif"];

pub fn is_video(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .map(|e| VIDEO_EXT.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

pub struct Show {
    pub items: Vec<String>,
    order: Vec<usize>,
    pub pos: usize,
    pub photo: Duration,
    pub looping: bool,
    pub shuffle: bool,
    pub fit: Fit,
    pub video_sound: bool,
    /// Fin de la photo affichée (None : vidéo en cours, ou pause).
    pub deadline: Option<Instant>,
    /// Temps restant de la photo pendant une pause.
    remaining: Option<Duration>,
    /// Début d'affichage de la photo en cours (hors pauses), pour changer sa durée à chaud.
    shown_since: Option<Instant>,
    /// Échecs de lecture consécutifs (évite de tourner en rond sur un dossier illisible).
    pub failures: usize,
    rng: u64,
}

impl Show {
    pub fn new(items: Vec<String>, photo_seconds: f64, looping: bool, shuffle: bool, fit: Fit, video_sound: bool) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
        let mut s = Self {
            order: (0..items.len()).collect(),
            items,
            pos: 0,
            photo: Duration::from_secs_f64(photo_seconds.clamp(0.5, 3600.0)),
            looping,
            shuffle,
            fit,
            video_sound,
            deadline: None,
            remaining: None,
            shown_since: None,
            failures: 0,
            rng: seed,
        };
        if shuffle {
            s.reshuffle();
        }
        s
    }

    fn rand(&mut self) -> u64 {
        // xorshift64 : largement suffisant pour mélanger des photos.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn reshuffle(&mut self) {
        for i in (1..self.order.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            self.order.swap(i, j);
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn current(&self) -> Option<&str> {
        self.order.get(self.pos).map(|i| self.items[*i].as_str())
    }

    /// Passe au média suivant. Faux quand le diaporama est fini (pas de boucle).
    pub fn advance(&mut self) -> bool {
        if self.pos + 1 < self.order.len() {
            self.pos += 1;
            true
        } else if self.looping && !self.order.is_empty() {
            if self.shuffle {
                self.reshuffle();
            }
            self.pos = 0;
            true
        } else {
            false
        }
    }

    pub fn back(&mut self) {
        if self.pos > 0 {
            self.pos -= 1;
        } else if self.looping && !self.order.is_empty() {
            self.pos = self.order.len() - 1;
        }
    }

    pub fn goto(&mut self, index: usize) {
        if index < self.order.len() {
            self.pos = index;
        }
    }

    /// Le média courant vient de s'afficher : lance le minutage s'il s'agit d'une photo.
    pub fn started(&mut self, paused: bool) {
        self.failures = 0;
        let photo = self.current().map(|p| !is_video(p)).unwrap_or(false);
        self.deadline = None;
        self.remaining = None;
        self.shown_since = photo.then(Instant::now);
        if photo {
            if paused {
                self.remaining = Some(self.photo);
            } else {
                self.deadline = Some(Instant::now() + self.photo);
            }
        }
    }

    /// Réglages modifiés pendant la lecture : le média affiché reste à l'écran.
    pub fn update(&mut self, photo_seconds: f64, looping: bool, shuffle: bool, video_sound: bool) {
        let photo = Duration::from_secs_f64(photo_seconds.clamp(0.5, 3600.0));
        if photo != self.photo {
            // La photo affichée prend la nouvelle durée, comptée depuis son apparition.
            if let (Some(since), Some(_)) = (self.shown_since, self.deadline) {
                self.deadline = Some(since + photo);
            }
            if let Some(r) = self.remaining {
                let shown = self.photo.saturating_sub(r);
                self.remaining = Some(photo.saturating_sub(shown));
            }
            self.photo = photo;
        }
        self.looping = looping;
        self.video_sound = video_sound;
        if shuffle != self.shuffle {
            self.shuffle = shuffle;
            let cur = self.order.get(self.pos).copied();
            if shuffle {
                // La suite est mélangée, le média courant ne bouge pas.
                let rest = self.pos + 1;
                for i in (rest + 1..self.order.len()).rev() {
                    let j = rest + (self.rand() % (i - rest + 1) as u64) as usize;
                    self.order.swap(i, j);
                }
            } else {
                self.order = (0..self.items.len()).collect();
                self.pos = cur.unwrap_or(0);
            }
        }
    }

    pub fn pause(&mut self) {
        if let Some(d) = self.deadline.take() {
            self.remaining = Some(d.saturating_duration_since(Instant::now()));
        }
    }

    pub fn resume(&mut self) {
        if let Some(r) = self.remaining.take() {
            self.deadline = Some(Instant::now() + r);
            // Le temps de pause ne compte pas dans la durée affichée.
            self.shown_since = self.deadline.and_then(|d| d.checked_sub(self.photo));
        }
    }
}
