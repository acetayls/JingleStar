//! Commande OSC (UDP) de CueStar : mêmes adresses que le Diaporama d'origine (préfixe
//! `/diaporama` accepté aussi), plus le Lecteur et le noir général.

use rosc::{OscPacket, OscType};
use serde::{Deserialize, Serialize};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OscSettings {
    pub enabled: bool,
    pub port: u16,
    /// Vrai : écoute sur le réseau ; faux : seulement depuis ce PC (127.0.0.1).
    pub network: bool,
    /// Préfixe de toutes les adresses.
    pub prefix: String,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self { enabled: false, port: 9000, network: false, prefix: "/cuestar".into() }
    }
}

fn settings_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("com", "acetayls", "CueStar").map(|d| d.config_dir().join("osc.json"))
}

impl OscSettings {
    /// Réglages OSC de cette machine.
    pub fn load() -> Self {
        settings_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Some(p) = settings_path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}

/// Message reçu : adresse sans le préfixe (« lecteur/go ») et arguments.
#[derive(Debug, Clone)]
pub struct Msg {
    pub path: String,
    pub args: Vec<OscType>,
}

impl Msg {
    pub fn f64(&self) -> Option<f64> {
        match self.args.first()? {
            OscType::Float(v) => Some(*v as f64),
            OscType::Double(v) => Some(*v),
            OscType::Int(v) => Some(*v as f64),
            OscType::Long(v) => Some(*v as f64),
            OscType::String(s) => s.trim().replace(',', ".").parse().ok(),
            _ => None,
        }
    }

    /// 0/1, T/F, entier, flottant ou « on/off ». Sans argument : vrai.
    pub fn flag(&self) -> bool {
        match self.args.first() {
            None | Some(OscType::Nil) | Some(OscType::Inf) => true,
            Some(OscType::Bool(b)) => *b,
            Some(OscType::String(s)) => matches!(s.trim().to_lowercase().as_str(), "1" | "on" | "true" | "oui" | "yes"),
            _ => self.f64().is_some_and(|v| v >= 0.5),
        }
    }

    pub fn text(&self) -> Option<String> {
        match self.args.first()? {
            OscType::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

pub struct OscServer {
    pub rx: Receiver<Msg>,
    stop: Arc<AtomicBool>,
    pub error: Option<String>,
    pub settings: OscSettings,
}

impl OscServer {
    pub fn start(s: &OscSettings, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut error = None;
        if s.enabled {
            let host = if s.network { "0.0.0.0" } else { "127.0.0.1" };
            match UdpSocket::bind((host, s.port)) {
                Ok(sock) => {
                    let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(250)));
                    let (stop2, prefix) = (stop.clone(), s.prefix.trim_end_matches('/').to_string());
                    std::thread::Builder::new()
                        .name("osc".into())
                        .spawn(move || {
                            let mut buf = [0u8; rosc::decoder::MTU];
                            while !stop2.load(Ordering::Relaxed) {
                                let Ok((n, _)) = sock.recv_from(&mut buf) else { continue };
                                if let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..n]) {
                                    let mut out = Vec::new();
                                    flatten(packet, &mut out);
                                    for m in out {
                                        // Préfixe de CueStar, ou celui du Diaporama d'origine.
                                        let path = if let Some(r) = m.addr.strip_prefix(&prefix) {
                                            Some(r.trim_start_matches('/').to_string())
                                        } else {
                                            m.addr.strip_prefix("/diaporama/").map(|r| format!("diaporama/{r}"))
                                        };
                                        if let Some(path) = path {
                                            let _ = tx.send(Msg { path, args: m.args });
                                            repaint();
                                        }
                                    }
                                }
                            }
                        })
                        .ok();
                }
                Err(e) => error = Some(format!("port OSC {} indisponible : {e}", s.port)),
            }
        }
        Self { rx, stop, error, settings: s.clone() }
    }
}

impl Drop for OscServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn flatten(p: OscPacket, out: &mut Vec<rosc::OscMessage>) {
    match p {
        OscPacket::Message(m) => out.push(m),
        OscPacket::Bundle(b) => b.content.into_iter().for_each(|p| flatten(p, out)),
    }
}

/// Liste de référence des adresses (plus affichée dans la fenêtre, comme dans JingleStar).
#[allow(dead_code)]
pub const HELP: &[(&str, &str)] = &[
    ("noir", "écran de sortie au noir"),
    ("lecteur/go", "lance l'élément sélectionné (ou reprend)"),
    ("lecteur/pause | play | toggle | stop", "transport du Lecteur"),
    ("lecteur/next | prev", "élément suivant / précédent"),
    ("lecteur/goto <i>", "lance l'élément n° i (1 = premier)"),
    ("lecteur/shuffle <0|1>", "ordre aléatoire"),
    ("lecteur/loop <0|1>", "tout boucler"),
    ("lecteur/transition <cut|black|fade> [s]", "transition et durée"),
    ("cuelist/go", "lance le cue en attente (ou reprend)"),
    ("cuelist/pause | play | toggle | stop", "transport de la Cuelist"),
    ("cuelist/next | prev", "cue en attente suivant / précédent"),
    ("cuelist/standby <i>", "met le cue n° i en attente"),
    ("diaporama/play | pause | toggle | stop", "transport du Diaporama"),
    ("diaporama/next | prev", "média suivant / précédent"),
    ("diaporama/goto <i>", "média n° i (1 = premier)"),
    ("diaporama/settings/shuffle | loop | recursive | video_sound <0|1>", "réglages"),
    ("diaporama/settings/photo_duration <s>", "durée des photos (1 à 60)"),
    ("diaporama/settings/fit <contain|cover|auto>", "ajustement"),
    ("diaporama/settings/folder <chemin>", "dossier à lire"),
];
