//! Serveur OSC des applis Star, repris de JingleStar : réception des commandes (UDP),
//! abonnements pour le retour d'état, description OSCQuery (HTTP/JSON) et annonce mDNS.
//!
//! L'appli fournit la traduction d'un message en commande et l'arborescence OSCQuery de son
//! état courant ; le reste (sockets, threads, abonnés, HTTP) est commun.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use rosc::{OscMessage, OscPacket, OscType};
use serde_json::{json, Map, Value};
use star_core::i18n::tr;

// ------------------------------------------------------------------ adresses et arguments

/// Préfixe normalisé : « /jinglestar », ou chaîne vide.
pub fn normalize_prefix(prefix: &str) -> String {
    let p = prefix.trim().trim_matches('/');
    if p.is_empty() {
        String::new()
    } else {
        format!("/{p}")
    }
}

/// Nombre reçu ; une valeur non numérique (« nan », infini) est refusée.
pub fn arg_f32(args: &[OscType]) -> Option<f32> {
    let v = match args.first()? {
        OscType::Float(v) => *v,
        OscType::Double(v) => *v as f32,
        OscType::Int(v) => *v as f32,
        OscType::Long(v) => *v as f32,
        OscType::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    v.is_finite().then_some(v)
}

/// 0/1, T/F, « on/off », « true/false », « yes/no ».
pub fn arg_bool(args: &[OscType]) -> Option<bool> {
    match args.first()? {
        OscType::Bool(b) => Some(*b),
        OscType::String(s) => match s.trim().to_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => Some(true),
            "0" | "false" | "off" | "no" => Some(false),
            _ => None,
        },
        _ => arg_f32(args).map(|v| v >= 0.5),
    }
}

/// Un déclencheur sans argument part ; avec un nombre, seul un appui (≠ 0) part.
/// Évite le double déclenchement des boutons qui envoient 1 à l'appui et 0 au relâché.
pub fn is_press(args: &[OscType]) -> bool {
    match args.first() {
        None => true,
        Some(OscType::Bool(b)) => *b,
        Some(OscType::Nil) | Some(OscType::Inf) => true,
        Some(_) => arg_f32(args).map(|v| v != 0.0).unwrap_or(true),
    }
}

/// Messages d'un paquet, paquets groupés dépliés.
pub fn flatten(packet: OscPacket, out: &mut Vec<OscMessage>) {
    match packet {
        OscPacket::Message(m) => out.push(m),
        OscPacket::Bundle(b) => b.content.into_iter().for_each(|p| flatten(p, out)),
    }
}

/// Adresse IP de l'ordinateur sur le réseau local (aucun paquet n'est envoyé).
pub fn local_ip() -> Option<std::net::IpAddr> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.connect(("8.8.8.8", 80)).ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

// ------------------------------------------------------------------ OSCQuery

pub const ACCESS_READ: u8 = 1;
pub const ACCESS_WRITE: u8 = 2;
pub const ACCESS_RW: u8 = 3;

/// Feuille de l'arborescence OSCQuery.
pub fn leaf(ty: &str, access: u8, description: &str, value: Option<Value>, range: Option<Value>) -> Value {
    let mut node = json!({ "TYPE": ty, "ACCESS": access, "DESCRIPTION": description });
    if let Some(v) = value {
        node["VALUE"] = json!([v]);
    }
    if let Some(r) = range {
        node["RANGE"] = json!([r]);
    }
    node
}

/// Déclencheur sans argument.
pub fn trigger_leaf(description: &str) -> Value {
    leaf("N", ACCESS_WRITE, description, None, None)
}

/// Dans OSC, le type d'un booléen porte sa valeur.
pub fn bool_type(b: bool) -> &'static str {
    if b {
        "T"
    } else {
        "F"
    }
}

/// Racine d'une arborescence.
pub fn root(description: &str) -> Value {
    json!({ "FULL_PATH": "/", "ACCESS": 0, "DESCRIPTION": description, "CONTENTS": {} })
}

/// Place `node` à `path` en créant les dossiers intermédiaires.
pub fn insert(root: &mut Value, path: &str, mut node: Value) {
    let mut cur = root;
    let mut full = String::new();
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    for (i, seg) in segs.iter().enumerate() {
        full.push('/');
        full.push_str(seg);
        let contents = cur
            .as_object_mut()
            .expect("nœud objet")
            .entry("CONTENTS")
            .or_insert_with(|| Value::Object(Map::new()));
        if i + 1 == segs.len() {
            node["FULL_PATH"] = json!(full);
            contents[*seg] = node;
            return;
        }
        if contents.get(*seg).is_none() {
            contents[*seg] = json!({ "FULL_PATH": full, "ACCESS": 0 });
        }
        cur = contents.get_mut(*seg).expect("dossier créé");
    }
}

const ATTRIBUTES: [&str; 7] = ["FULL_PATH", "CONTENTS", "TYPE", "ACCESS", "VALUE", "RANGE", "DESCRIPTION"];

/// Réponse HTTP à une requête OSCQuery : (code, corps JSON). `tree` n'est construit que si
/// la requête le demande.
pub fn answer(url: &str, name: &str, osc_port: u16, tree: impl FnOnce() -> Value) -> (u16, Value) {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if query == "HOST_INFO" {
        return (200, host_info(name, osc_port));
    }
    let root = tree();
    let mut node = &root;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        match node.get("CONTENTS").and_then(|c| c.get(seg)) {
            Some(n) => node = n,
            None => return (404, Value::Null),
        }
    }
    if query.is_empty() {
        return (200, node.clone());
    }
    let attr = query.split('&').next().unwrap_or("").to_uppercase();
    if !ATTRIBUTES.contains(&attr.as_str()) {
        return (400, Value::Null);
    }
    match node.get(&attr) {
        Some(v) => (200, json!({ attr: v })),
        None => (204, Value::Null),
    }
}

/// « HOST_INFO » : nom du service et port OSC. Companion s'en sert pour reconnaître l'appli.
pub fn host_info(name: &str, osc_port: u16) -> Value {
    json!({
        "NAME": name,
        "OSC_PORT": osc_port,
        "OSC_TRANSPORT": "UDP",
        "EXTENSIONS": {
            "ACCESS": true,
            "VALUE": true,
            "RANGE": true,
            "DESCRIPTION": true,
            "TAGS": false,
            "CLIPMODE": false,
            "UNIT": false,
            "CRITICAL": false,
            "LISTEN": false,
            "PATH_CHANGED": false,
        }
    })
}

// ------------------------------------------------------------------ serveur

/// Réglages d'un serveur.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    /// Nom annoncé (OSCQuery, mDNS).
    pub name: String,
    /// Port UDP de réception des commandes.
    pub port: u16,
    /// Vrai : écoute sur le réseau ; faux : seulement depuis ce PC (127.0.0.1).
    pub network: bool,
    /// Préfixe des adresses, l'abonnement est « <préfixe>/subscribe ».
    pub prefix: String,
    /// Port HTTP OSCQuery, ou aucun.
    pub oscquery_port: Option<u16>,
    /// Annonce mDNS (Bonjour) des deux services, avec ce nom d'hôte (« jinglestar.local. »).
    pub zeroconf: Option<String>,
}

/// Traduit un message reçu en commande. `Ok(None)` : message valide mais sans effet.
pub type Parser<C> = Arc<dyn Fn(&OscMessage) -> Result<Option<C>, String> + Send + Sync>;
/// Arborescence OSCQuery de l'état publié.
pub type Tree<S> = Arc<dyn Fn(&S) -> Value + Send + Sync>;

/// Serveurs réseau en marche ; tout s'arrête quand on le libère.
pub struct OscServer<C, S> {
    pub commands: Receiver<C>,
    /// Messages refusés ou erreurs de réception, pour la barre d'état.
    pub errors: Receiver<String>,
    /// État publié, décrit par OSCQuery.
    pub snapshot: Arc<Mutex<S>>,
    /// Ports effectivement ouverts.
    pub udp_port: Option<u16>,
    pub http_port: Option<u16>,
    /// Problèmes au démarrage (port occupé…).
    pub startup_errors: Vec<String>,
    /// Préfixe normalisé.
    pub prefix: String,
    /// Destinataires du retour d'état (adresse, dernier abonnement) et socket d'envoi.
    subscribers: Arc<Mutex<Vec<(SocketAddr, Instant)>>>,
    sender: Option<UdpSocket>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    mdns: Option<mdns_sd::ServiceDaemon>,
}

impl<C: Send + 'static, S: Clone + Default + PartialEq + Send + 'static> OscServer<C, S> {
    pub fn start(cfg: ServerConfig, parse: Parser<C>, tree: Tree<S>, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (cmd_tx, commands) = unbounded();
        let (err_tx, errors) = unbounded();
        let mut s = Self {
            commands,
            errors,
            snapshot: Arc::new(Mutex::new(S::default())),
            udp_port: None,
            http_port: None,
            startup_errors: Vec::new(),
            prefix: normalize_prefix(&cfg.prefix),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            sender: None,
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
            mdns: None,
        };
        let host = if cfg.network { "0.0.0.0" } else { "127.0.0.1" };
        match UdpSocket::bind((host, cfg.port)) {
            Ok(sock) => {
                let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
                s.udp_port = Some(cfg.port);
                s.sender = sock.try_clone().ok();
                let (stop, subs) = (s.stop.clone(), s.subscribers.clone());
                let subscribe_addr = format!("{}/subscribe", s.prefix);
                let repaint = repaint.clone();
                s.threads.push(
                    std::thread::Builder::new()
                        .name("osc-udp".into())
                        .spawn(move || udp_loop(sock, subscribe_addr, stop, parse, cmd_tx, err_tx, subs, repaint))
                        .expect("thread OSC"),
                );
            }
            Err(e) => s.startup_errors.push(format!("OSC : {} {} ({e})", tr("port UDP indisponible", "UDP port unavailable"), cfg.port)),
        }
        if let Some(http_port) = cfg.oscquery_port {
            match tiny_http::Server::http((host, http_port)) {
                Ok(http) => {
                    s.http_port = Some(http_port);
                    let (stop, snap) = (s.stop.clone(), s.snapshot.clone());
                    let (name, port) = (cfg.name.clone(), cfg.port);
                    s.threads.push(
                        std::thread::Builder::new()
                            .name("oscquery".into())
                            .spawn(move || http_loop(http, &name, port, tree, snap, stop))
                            .expect("thread OSCQuery"),
                    );
                    if let Some(mdns_host) = &cfg.zeroconf {
                        match advertise(&cfg.name, mdns_host, cfg.port, http_port) {
                            Ok(d) => s.mdns = Some(d),
                            Err(e) => s.startup_errors.push(format!("{} : {e}", tr("Annonce mDNS impossible", "mDNS announcement failed"))),
                        }
                    }
                }
                Err(e) => s.startup_errors.push(format!("OSCQuery : {} {} ({e})", tr("port HTTP indisponible", "HTTP port unavailable"), http_port)),
            }
        }
        s
    }

    /// Nouvel état, servi par OSCQuery.
    pub fn publish(&self, snap: S) {
        if let Ok(mut s) = self.snapshot.lock() {
            if *s != snap {
                *s = snap;
            }
        }
    }
}

impl<C, S> OscServer<C, S> {
    /// Quelqu'un attend-il le retour d'état ?
    pub fn has_subscribers(&self) -> bool {
        self.subscribers.lock().map(|s| !s.is_empty()).unwrap_or(false)
    }

    /// Envoie `messages`, groupés en un paquet, aux abonnés encore actifs.
    pub fn send_to_subscribers(&self, messages: Vec<OscMessage>) {
        let Some(sock) = self.sender.as_ref() else { return };
        let targets: Vec<SocketAddr> = match self.subscribers.lock() {
            Ok(mut subs) => {
                subs.retain(|(_, t)| t.elapsed() < SUBSCRIPTION);
                subs.iter().map(|(a, _)| *a).collect()
            }
            Err(_) => return,
        };
        if targets.is_empty() || messages.is_empty() {
            return;
        }
        let content = messages.into_iter().map(OscPacket::Message).collect();
        let bundle = OscPacket::Bundle(rosc::OscBundle { timetag: rosc::OscTime { seconds: 0, fractional: 1 }, content });
        if let Ok(bytes) = rosc::encoder::encode(&bundle) {
            for t in targets {
                let _ = sock.send_to(&bytes, t);
            }
        }
    }
}

/// Un abonnement dure ce temps sans être renouvelé.
pub const SUBSCRIPTION: Duration = Duration::from_secs(5);

/// « …/subscribe » : l'expéditeur recevra le retour d'état sur son adresse et son port
/// d'envoi, ou sur le port donné en argument.
fn subscribe(subs: &Mutex<Vec<(SocketAddr, Instant)>>, mut from: SocketAddr, msg: &OscMessage) {
    if let Some(OscType::Int(p)) = msg.args.first() {
        if (1..=65535).contains(p) {
            from.set_port(*p as u16);
        }
    }
    if let Ok(mut s) = subs.lock() {
        s.retain(|(a, _)| *a != from);
        if s.len() < 16 {
            s.push((from, Instant::now()));
        }
    }
}

impl<C, S> Drop for OscServer<C, S> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(d) = self.mdns.take() {
            let _ = d.shutdown();
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn udp_loop<C>(
    sock: UdpSocket,
    subscribe_addr: String,
    stop: Arc<AtomicBool>,
    parse: Parser<C>,
    commands: Sender<C>,
    errors: Sender<String>,
    subs: Arc<Mutex<Vec<(SocketAddr, Instant)>>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let mut buf = vec![0u8; 65_536];
    while !stop.load(Ordering::Relaxed) {
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(r) => r,
            // Délai écoulé ; sous Windows, un ICMP « port injoignable » remonte aussi en
            // erreur de réception : on l'ignore.
            Err(_) => continue,
        };
        match rosc::decoder::decode_udp(&buf[..n]) {
            Ok((_, packet)) => {
                let mut messages = Vec::new();
                flatten(packet, &mut messages);
                for msg in messages {
                    if msg.addr == subscribe_addr {
                        subscribe(&subs, from, &msg);
                        continue;
                    }
                    match parse(&msg) {
                        Ok(Some(cmd)) => {
                            let _ = commands.send(cmd);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = errors.send(e);
                        }
                    }
                }
                repaint();
            }
            Err(e) => {
                let _ = errors.send(format!("{} ({e})", tr("paquet OSC illisible", "unreadable OSC packet")));
                repaint();
            }
        }
    }
}

fn http_loop<S: Clone + Default>(http: tiny_http::Server, name: &str, port: u16, tree: Tree<S>, snap: Arc<Mutex<S>>, stop: Arc<AtomicBool>) {
    let json_header = tiny_http::Header::from_bytes("Content-Type", "application/json").expect("en-tête");
    let cors = tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").expect("en-tête");
    while !stop.load(Ordering::Relaxed) {
        let Ok(Some(req)) = http.recv_timeout(Duration::from_millis(200)) else { continue };
        let current = snap.lock().map(|s| s.clone()).unwrap_or_default();
        let (code, body) = answer(req.url(), name, port, || tree(&current));
        let resp = if code == 200 {
            tiny_http::Response::from_string(body.to_string()).with_header(json_header.clone())
        } else {
            tiny_http::Response::from_string(String::new())
        };
        let _ = req.respond(resp.with_status_code(code).with_header(cors.clone()));
    }
}

fn advertise(name: &str, host: &str, osc_port: u16, http_port: u16) -> Result<mdns_sd::ServiceDaemon, mdns_sd::Error> {
    let daemon = mdns_sd::ServiceDaemon::new()?;
    let query = mdns_sd::ServiceInfo::new("_oscjson._tcp.local.", name, host, (), http_port, None)?.enable_addr_auto();
    daemon.register(query)?;
    let osc = mdns_sd::ServiceInfo::new("_osc._udp.local.", name, host, (), osc_port, None)?.enable_addr_auto();
    daemon.register(osc)?;
    Ok(daemon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_normalized() {
        assert_eq!(normalize_prefix(" jinglestar/ "), "/jinglestar");
        assert_eq!(normalize_prefix("/"), "");
    }

    #[test]
    fn oscquery_answers() {
        let tree = || {
            let mut r = root("Test");
            insert(&mut r, "/t/go", trigger_leaf("GO"));
            insert(&mut r, "/t/name", leaf("s", ACCESS_READ, "Nom", Some(json!("Ouverture")), None));
            r
        };
        assert_eq!(answer("/t/name?VALUE", "Test", 8000, tree), (200, json!({ "VALUE": ["Ouverture"] })));
        assert_eq!(answer("/t/go", "Test", 8000, tree).1["FULL_PATH"], "/t/go");
        assert_eq!(answer("/t/absent", "Test", 8000, tree).0, 404);
        assert_eq!(answer("/t/go?NOPE", "Test", 8000, tree).0, 400);
        assert_eq!(answer("/t/go?VALUE", "Test", 8000, tree).0, 204);
        let (_, info) = answer("/?HOST_INFO", "Test", 8000, tree);
        assert_eq!((info["NAME"].as_str(), info["OSC_PORT"].as_u64()), (Some("Test"), Some(8000)));
    }

    #[test]
    fn local_server_roundtrip() {
        let cfg = ServerConfig { name: "Test".into(), port: 39_861, network: false, prefix: "/t".into(), oscquery_port: Some(39_862), zeroconf: None };
        let parse: Parser<String> = Arc::new(|m| Ok(m.addr.strip_prefix("/t/").map(str::to_owned)));
        let tree: Tree<u32> = Arc::new(|n| {
            let mut r = root("Test");
            insert(&mut r, "/t/count", leaf("i", ACCESS_READ, "", Some(json!(n)), None));
            r
        });
        let server = OscServer::start(cfg, parse, tree, Arc::new(|| {}));
        assert!(server.startup_errors.is_empty(), "{:?}", server.startup_errors);
        server.publish(7);

        let client = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let send = |addr: &str| {
            let p = OscPacket::Message(OscMessage { addr: addr.into(), args: vec![] });
            client.send_to(&rosc::encoder::encode(&p).unwrap(), ("127.0.0.1", 39_861)).unwrap();
        };
        send("/t/subscribe");
        send("/t/go");
        assert_eq!(server.commands.recv_timeout(Duration::from_secs(2)).unwrap(), "go");
        assert!(server.has_subscribers());
        server.send_to_subscribers(vec![OscMessage { addr: "/t/state".into(), args: vec![OscType::Int(1)] }]);
        let mut buf = [0u8; 512];
        let (n, _) = client.recv_from(&mut buf).unwrap();
        let OscPacket::Bundle(b) = rosc::decoder::decode_udp(&buf[..n]).unwrap().1 else { panic!("paquet groupé attendu") };
        assert!(matches!(&b.content[0], OscPacket::Message(m) if m.addr == "/t/state"));

        use std::io::{Read, Write};
        let mut tcp = std::net::TcpStream::connect(("127.0.0.1", 39_862)).unwrap();
        tcp.write_all(b"GET /t/count?VALUE HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        let mut resp = String::new();
        tcp.read_to_string(&mut resp).unwrap();
        assert!(resp.ends_with(r#"{"VALUE":[7]}"#), "{resp}");
    }
}
