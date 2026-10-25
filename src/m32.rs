//! Écoute des consoles Midas M32 / Behringer X32 : OSC sur UDP 10023.
//!
//! Ces consoles n'envoient leurs changements qu'aux clients abonnés : il faut leur envoyer
//! « /xremote » au moins toutes les 10 s. Une écoute sans aucun envoi est donc impossible.
//! JingleStar n'envoie que les requêtes de [`Query`] (abonnement et demande d'information) :
//! la socket n'est accessible qu'à travers [`link::Link`], qui n'a pas d'autre moyen d'écrire.

use std::collections::HashMap;
use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use rosc::{OscMessage, OscPacket, OscType};

use crate::console::ConsoleStatus;
use crate::i18n::tr;
use crate::project::M32Settings;
use crate::remote::{InputEvent, CONSOLE_MIN};

/// Seules requêtes que JingleStar peut envoyer : aucune ne modifie la console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    /// Abonnement aux changements pour 10 s, renouvelé toutes les 8 s.
    XRemote,
    /// Version et modèle de la console.
    Info,
}

impl Query {
    #[cfg(test)]
    pub const ALL: [Query; 2] = [Query::XRemote, Query::Info];

    pub const fn address(self) -> &'static str {
        match self {
            Query::XRemote => "/xremote",
            Query::Info => "/info",
        }
    }

    fn packet(self) -> Vec<u8> {
        let msg = OscPacket::Message(OscMessage { addr: self.address().into(), args: Vec::new() });
        rosc::encoder::encode(&msg).unwrap_or_default()
    }
}

mod link {
    use std::net::UdpSocket;

    use super::Query;

    /// Socket « connectée » à la console. Le seul chemin d'écriture est `query`,
    /// limité aux requêtes de [`Query`], toujours sans argument.
    pub struct Link {
        sock: UdpSocket,
    }

    impl Link {
        pub fn new(sock: UdpSocket) -> Self {
            Self { sock }
        }

        pub fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.sock.recv(buf)
        }

        pub fn query(&self, q: Query) -> std::io::Result<()> {
            self.sock.send(&q.packet()).map(|_| ())
        }
    }
}

/// Écoute en tâche de fond ; tout s'arrête quand on la libère.
pub struct M32Client {
    pub events: Receiver<InputEvent>,
    status: Arc<Mutex<ConsoleStatus>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl M32Client {
    pub fn start(settings: M32Settings, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, events) = unbounded();
        let status = Arc::new(Mutex::new(ConsoleStatus::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (status.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("m32".into())
            .spawn(move || run(settings, tx, st, sp, repaint))
            .expect("thread M32");
        Self { events, status, stop, thread: Some(thread) }
    }

    pub fn status(&self) -> ConsoleStatus {
        let mut s = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        // UDP : la console est « connectée » tant qu'elle répond.
        s.connected = s.last_rx.is_some_and(|t| t.elapsed() < Duration::from_secs(15));
        s
    }
}

impl Drop for M32Client {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open(settings: &M32Settings) -> Result<link::Link, String> {
    let addr = (settings.host.trim(), settings.port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| tr("adresse introuvable", "address not found").to_string())?;
    let sock = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
    sock.connect(addr).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
    Ok(link::Link::new(sock))
}

fn run(
    settings: M32Settings,
    events: Sender<InputEvent>,
    status: Arc<Mutex<ConsoleStatus>>,
    stop: Arc<AtomicBool>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let set_error = |e: Option<String>| {
        if let Ok(mut s) = status.lock() {
            s.error = e;
        }
        repaint();
    };
    let link = loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match open(&settings) {
            Ok(l) => break l,
            Err(e) => {
                set_error(Some(format!("{} {}:{} ({e})", tr("Adresse invalide", "Invalid address"), settings.host.trim(), settings.port)));
                std::thread::sleep(Duration::from_secs(3));
            }
        }
    };
    let mut last_sub: Option<Instant> = None;
    let mut values: HashMap<String, i32> = HashMap::new();
    let mut buf = vec![0u8; 65_536];
    while !stop.load(Ordering::Relaxed) {
        if last_sub.is_none_or(|t| t.elapsed() > Duration::from_secs(8)) {
            last_sub = Some(Instant::now());
            let sent = link.query(Query::XRemote).and_then(|_| link.query(Query::Info));
            set_error(sent.err().map(|e| format!("{} ({e})", tr("Envoi impossible", "Cannot send"))));
        }
        let n = match link.recv(&mut buf) {
            Ok(n) => n,
            // Délai dépassé, ou « port injoignable » quand la console est absente.
            Err(_) => continue,
        };
        let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..n]) else { continue };
        let mut messages = Vec::new();
        flatten(packet, &mut messages);
        for msg in messages {
            if let Ok(mut s) = status.lock() {
                s.last_rx = Some(Instant::now());
                s.error = None;
                if msg.addr == "/info" {
                    s.model = info_model(&msg);
                } else {
                    s.last_line = Some(describe(&msg));
                }
            }
            if let Some(ev) = to_event(&msg, &mut values) {
                let _ = events.send(ev);
            }
        }
        repaint();
    }
}

fn flatten(packet: OscPacket, out: &mut Vec<OscMessage>) {
    match packet {
        OscPacket::Message(m) => out.push(m),
        OscPacket::Bundle(b) => b.content.into_iter().for_each(|p| flatten(p, out)),
    }
}

/// Réponse à /info : version, nom du serveur, modèle, version du logiciel.
fn info_model(msg: &OscMessage) -> Option<String> {
    match msg.args.get(2) {
        Some(OscType::String(m)) => Some(m.clone()),
        _ => None,
    }
}

fn describe(msg: &OscMessage) -> String {
    let args: Vec<String> = msg
        .args
        .iter()
        .map(|a| match a {
            OscType::Int(i) => i.to_string(),
            OscType::Float(f) => format!("{f:.4}"),
            OscType::String(s) => format!("\"{s}\""),
            other => format!("{other:?}"),
        })
        .collect();
    format!("{} {}", msg.addr, args.join(" "))
}

/// Position d'un fader (0 à 1) → dB, loi des X32 / M32 (0,75 = 0 dB, 1 = +10 dB).
pub fn fader_db(f: f32) -> Option<f32> {
    let f = f.clamp(0.0, 1.0);
    if f <= 0.0 {
        None
    } else if f >= 0.5 {
        Some(f * 40.0 - 30.0)
    } else if f >= 0.25 {
        Some(f * 80.0 - 50.0)
    } else if f >= 0.0625 {
        Some(f * 160.0 - 70.0)
    } else {
        Some(f * 480.0 - 90.0)
    }
}

/// Changement de paramètre → événement. Les niveaux sont convertis en centièmes de dB,
/// comme ceux des consoles Yamaha, pour partager les mêmes correspondances.
fn to_event(msg: &OscMessage, values: &mut HashMap<String, i32>) -> Option<InputEvent> {
    // Rappel de scène : numéro affiché sur la console.
    if msg.addr == "/-show/prepos/current" {
        if let Some(OscType::Int(n)) = msg.args.first() {
            return (*n >= 0).then_some(InputEvent::Scene { number: *n as u32 });
        }
    }
    let value = match msg.args.first()? {
        OscType::Int(i) => *i,
        OscType::Float(f) if msg.addr.ends_with("/fader") || msg.addr.ends_with("/level") => {
            fader_db(*f).map_or(CONSOLE_MIN, |db| (db * 100.0).round() as i32)
        }
        OscType::Float(f) => (f * 1000.0).round() as i32,
        _ => return None,
    };
    let previous = values.insert(msg.addr.clone(), value);
    Some(InputEvent::Console { address: msg.addr.clone(), x: 0, y: 0, value, previous })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(addr: &str, arg: OscType) -> OscMessage {
        OscMessage { addr: addr.into(), args: vec![arg] }
    }

    #[test]
    fn fader_law() {
        assert_eq!(fader_db(0.0), None);
        assert_eq!(fader_db(0.75), Some(0.0));
        assert_eq!(fader_db(1.0), Some(10.0));
        assert_eq!(fader_db(0.5), Some(-10.0));
        assert_eq!(fader_db(0.25), Some(-30.0));
        assert_eq!(fader_db(0.0625), Some(-60.0));
    }

    #[test]
    fn converts_messages() {
        let mut v = HashMap::new();
        let on = to_event(&msg("/ch/05/mix/on", OscType::Int(0)), &mut v).unwrap();
        assert_eq!(on, InputEvent::Console { address: "/ch/05/mix/on".into(), x: 0, y: 0, value: 0, previous: None });
        let on = to_event(&msg("/ch/05/mix/on", OscType::Int(1)), &mut v).unwrap();
        assert!(matches!(on, InputEvent::Console { value: 1, previous: Some(0), .. }));
        let fader = to_event(&msg("/ch/05/mix/fader", OscType::Float(0.5)), &mut v).unwrap();
        assert!(matches!(fader, InputEvent::Console { value: -1000, .. }));
        let off = to_event(&msg("/dca/1/fader", OscType::Float(0.0)), &mut v).unwrap();
        assert!(matches!(off, InputEvent::Console { value: CONSOLE_MIN, .. }));
        assert_eq!(to_event(&msg("/-show/prepos/current", OscType::Int(12)), &mut v), Some(InputEvent::Scene { number: 12 }));
        assert_eq!(to_event(&msg("/ch/05/config/name", OscType::String("Voix".into())), &mut v), None);
    }

    #[test]
    fn subscribes_and_receives() {
        let console = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        console.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let port = console.local_addr().unwrap().port();
        let client = M32Client::start(M32Settings { enabled: true, host: "127.0.0.1".into(), port }, Arc::new(|| {}));
        // La console attend l'abonnement, puis envoie un changement à son auteur.
        let mut buf = [0u8; 1024];
        let mut received = Vec::new();
        let (n, from) = console.recv_from(&mut buf).unwrap();
        received.push(buf[..n].to_vec());
        let change = OscPacket::Message(msg("/ch/01/mix/on", OscType::Int(1)));
        console.send_to(&rosc::encoder::encode(&change).unwrap(), from).unwrap();
        let ev = client.events.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(ev, InputEvent::Console { value: 1, .. }));
        // Tout ce qui a été envoyé fait partie de la liste blanche, sans argument.
        console.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        while let Ok((n, _)) = console.recv_from(&mut buf) {
            received.push(buf[..n].to_vec());
        }
        let allowed: Vec<&str> = Query::ALL.iter().map(|q| q.address()).collect();
        for p in received {
            let (_, packet) = rosc::decoder::decode_udp(&p).unwrap();
            let OscPacket::Message(m) = packet else { panic!("paquet inattendu") };
            assert!(allowed.contains(&m.addr.as_str()), "requête non autorisée : {}", m.addr);
            assert!(m.args.is_empty(), "argument envoyé : {:?}", m.args);
        }
    }
}
