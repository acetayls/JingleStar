//! Retour image vers la régie (fenêtre de monitoring) : vignettes RGBA envoyées sur une
//! connexion TCP locale. Un fil dédié écrit ; si la régie ne suit pas, l'image est sautée,
//! jamais la sortie ralentie.

use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};

/// Largeur des vignettes envoyées.
pub const THUMB_W: usize = 640;

/// Image confiée au fil d'envoi.
enum Frame {
    /// Trame prête (en-tête + RGBA).
    Ready(Vec<u8>),
    /// Pixels bruts de la carte graphique, convertis par le fil d'envoi (pas par l'affichage).
    Raw { w: usize, h: usize, pitch: usize, bgra: bool, flip: bool, data: Vec<u8> },
}

fn encode(w: usize, h: usize, pitch: usize, bgra: bool, flip: bool, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + w * h * 4);
    buf.extend_from_slice(&(w as u32).to_le_bytes());
    buf.extend_from_slice(&(h as u32).to_le_bytes());
    for y in 0..h {
        let sy = if flip { h - 1 - y } else { y };
        let row = &data[sy * pitch..sy * pitch + w * 4];
        if bgra {
            for p in row.chunks_exact(4) {
                buf.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        } else {
            buf.extend_from_slice(row);
        }
    }
    buf
}

pub struct MonitorFeed {
    tx: Option<SyncSender<Frame>>,
}

impl MonitorFeed {
    pub fn connect(port: u16) -> Self {
        let tx = TcpStream::connect(("127.0.0.1", port)).ok().map(|mut s| {
            let _ = s.set_nodelay(true);
            let (tx, rx) = sync_channel::<Frame>(1);
            std::thread::Builder::new()
                .name("sortie-monitoring".into())
                .spawn(move || {
                    for frame in rx {
                        let bytes = match frame {
                            Frame::Ready(b) => b,
                            Frame::Raw { w, h, pitch, bgra, flip, data } => encode(w, h, pitch, bgra, flip, &data),
                        };
                        if s.write_all(&bytes).is_err() {
                            break;
                        }
                    }
                })
                .ok();
            tx
        });
        Self { tx }
    }

    pub fn alive(&self) -> bool {
        self.tx.is_some()
    }

    /// Réduit l'image et l'envoie (en-tête : largeur, hauteur en u32 petit-boutiste).
    pub fn send(&mut self, w: usize, h: usize, stride: usize, rgba: &[u8]) {
        let Some(tx) = &self.tx else { return };
        let tw = THUMB_W.min(w).max(1);
        let th = (h * tw / w.max(1)).max(1);
        let mut buf = Vec::with_capacity(8 + tw * th * 4);
        buf.extend_from_slice(&(tw as u32).to_le_bytes());
        buf.extend_from_slice(&(th as u32).to_le_bytes());
        for y in 0..th {
            let sy = y * h / th;
            let row = &rgba[sy * stride..];
            for x in 0..tw {
                let sx = x * w / tw;
                let p = &row[sx * 4..sx * 4 + 4];
                buf.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        match tx.try_send(Frame::Ready(buf)) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => self.tx = None,
        }
    }

    /// Vignette déjà à la bonne taille, lue sur la carte graphique : la conversion des pixels
    /// (ordre BGRA, image retournée) se fait dans le fil d'envoi.
    pub fn send_raw(&mut self, w: usize, h: usize, pitch: usize, bgra: bool, flip: bool, data: Vec<u8>) {
        let Some(tx) = &self.tx else { return };
        match tx.try_send(Frame::Raw { w, h, pitch, bgra, flip, data }) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => self.tx = None,
        }
    }

    /// Image noire (rien à l'antenne).
    pub fn send_black(&mut self) {
        let (w, h) = (160usize, 90usize);
        let px = vec![0u8; w * h * 4];
        self.send(w, h, w * 4, &px);
    }
}
