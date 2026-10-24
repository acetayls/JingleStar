//! Pilotage à distance : réception des commandes OSC (UDP), serveur OSCQuery (HTTP/JSON)
//! qui décrit l'arborescence, et annonce mDNS pour la découverte automatique.
//!
//! Les numéros de module, de jingle et de morceau sont comptés à partir de 1 dans les adresses.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver, Sender};
use rosc::{OscMessage, OscPacket, OscType};
use serde_json::{json, Map, Value};

use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use crate::i18n::tr;
use crate::project::{ModuleMode, OscSettings};

pub const MODULE_DB: (f32, f32) = (-60.0, 10.0);
pub const JINGLE_DB: (f32, f32) = (-40.0, 10.0);

/// Commande reçue, avec des index comptés à partir de 0.
#[derive(Debug, Clone, PartialEq)]
pub enum OscCommand {
    StopAll,
    ModuleStop(usize),
    ModuleVolume(usize, f32),
    ModuleMode(usize, ModuleMode),
    JinglePlay(usize, usize),
    JingleStop(usize, usize),
    JingleToggle(usize, usize),
    JingleVolume(usize, usize, f32),
    /// Lecture, au morceau donné ou au morceau sélectionné.
    PlaylistPlay(usize, Option<usize>),
    PlaylistStop(usize),
    /// Pause : état imposé, ou bascule (`None`).
    PlaylistPause(usize, Option<bool>),
    PlaylistStep(usize, isize),
    PlaylistShuffle(usize, bool),
    PlaylistLoop(usize, bool),
}

// ------------------------------------------------------------------ état publié

/// Image de l'état de l'application, servie par OSCQuery.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub modules: Vec<ModuleState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleState {
    pub name: String,
    pub mode: ModuleMode,
    pub volume_db: f32,
    pub jingles: Vec<Option<JingleState>>,
    pub playlist: PlaylistState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JingleState {
    pub name: String,
    pub volume_db: f32,
    pub playing: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistState {
    pub tracks: usize,
    pub current: Option<usize>,
    pub playing: bool,
    pub paused: bool,
    pub shuffle: bool,
    pub looping: bool,
}

// ------------------------------------------------------------------ adresses

/// Préfixe normalisé : « /jinglestar », ou chaîne vide.
pub fn normalize_prefix(prefix: &str) -> String {
    let p = prefix.trim().trim_matches('/');
    if p.is_empty() {
        String::new()
    } else {
        format!("/{p}")
    }
}

fn arg_f32(args: &[OscType]) -> Option<f32> {
    match args.first()? {
        OscType::Float(v) => Some(*v),
        OscType::Double(v) => Some(*v as f32),
        OscType::Int(v) => Some(*v as f32),
        OscType::Long(v) => Some(*v as f32),
        OscType::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn arg_bool(args: &[OscType]) -> Option<bool> {
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
fn is_press(args: &[OscType]) -> bool {
    match args.first() {
        None => true,
        Some(OscType::Bool(b)) => *b,
        Some(OscType::Nil) | Some(OscType::Inf) => true,
        Some(_) => arg_f32(args).map(|v| v != 0.0).unwrap_or(true),
    }
}

/// Numéro compté à partir de 1 dans l'adresse → index.
fn index(seg: Option<&str>, count: usize) -> Result<usize, String> {
    let seg = seg.ok_or(tr("numéro manquant", "missing number"))?;
    match seg.parse::<usize>() {
        Ok(n) if n >= 1 && n <= count => Ok(n - 1),
        _ => Err(format!("{} : {seg}", tr("numéro invalide", "invalid number"))),
    }
}

/// Traduit un message en commande. `Ok(None)` : message valide mais sans effet (relâché d'un bouton).
pub fn parse(prefix: &str, msg: &OscMessage) -> Result<Option<OscCommand>, String> {
    let prefix = normalize_prefix(prefix);
    let rest = msg
        .addr
        .strip_prefix(prefix.as_str())
        .filter(|r| r.starts_with('/'))
        .ok_or_else(|| format!("{} : {}", tr("adresse inconnue", "unknown address"), msg.addr))?;
    let segs: Vec<&str> = rest.split('/').skip(1).collect();
    let args = &msg.args;
    let press = is_press(args);
    let trigger = |c: OscCommand| Ok(press.then_some(c));
    let value = || arg_f32(args).ok_or_else(|| format!("{} : {}", tr("valeur numérique attendue", "number expected"), msg.addr));
    let flag = || arg_bool(args).ok_or_else(|| format!("{} : {}", tr("valeur booléenne attendue", "boolean expected"), msg.addr));
    match segs.as_slice() {
        ["stop_all"] => trigger(OscCommand::StopAll),
        ["module", m, tail @ ..] => {
            let m = index(Some(m), MAX_MODULES)?;
            match tail {
                ["stop"] => trigger(OscCommand::ModuleStop(m)),
                ["volume"] => Ok(Some(OscCommand::ModuleVolume(m, value()?.clamp(MODULE_DB.0, MODULE_DB.1)))),
                ["mode"] => {
                    let mode = match args.first() {
                        Some(OscType::String(s)) if s.eq_ignore_ascii_case("jingle") => ModuleMode::Jingle,
                        Some(OscType::String(s)) if s.eq_ignore_ascii_case("playlist") => ModuleMode::Playlist,
                        Some(OscType::String(s)) if s.eq_ignore_ascii_case("text") => ModuleMode::Text,
                        _ => return Err(format!("{} : {}", tr("mode attendu (jingle, playlist ou text)", "mode expected (jingle, playlist or text)"), msg.addr)),
                    };
                    Ok(Some(OscCommand::ModuleMode(m, mode)))
                }
                ["jingle", s, action] => {
                    let s = index(Some(s), JINGLE_SLOTS)?;
                    match *action {
                        "play" => trigger(OscCommand::JinglePlay(m, s)),
                        "stop" => trigger(OscCommand::JingleStop(m, s)),
                        "toggle" => trigger(OscCommand::JingleToggle(m, s)),
                        "volume" => Ok(Some(OscCommand::JingleVolume(m, s, value()?.clamp(JINGLE_DB.0, JINGLE_DB.1)))),
                        _ => Err(format!("{} : {}", tr("adresse inconnue", "unknown address"), msg.addr)),
                    }
                }
                ["playlist", action] => match *action {
                    "play" => {
                        // Un entier ≥ 1 choisit le morceau ; sans argument, reprend au morceau sélectionné.
                        let track = match args.first() {
                            Some(OscType::Int(n)) if *n >= 1 => Some(*n as usize - 1),
                            Some(OscType::Long(n)) if *n >= 1 => Some(*n as usize - 1),
                            _ => None,
                        };
                        if track.is_some() {
                            Ok(Some(OscCommand::PlaylistPlay(m, track)))
                        } else {
                            trigger(OscCommand::PlaylistPlay(m, None))
                        }
                    }
                    "track" => {
                        let n = value()?;
                        if n < 1.0 {
                            return Err(format!("{} : {n}", tr("numéro de morceau invalide", "invalid track number")));
                        }
                        Ok(Some(OscCommand::PlaylistPlay(m, Some(n as usize - 1))))
                    }
                    "stop" => trigger(OscCommand::PlaylistStop(m)),
                    "pause" => trigger(OscCommand::PlaylistPause(m, None)),
                    "paused" => Ok(Some(OscCommand::PlaylistPause(m, Some(flag()?)))),
                    "next" => trigger(OscCommand::PlaylistStep(m, 1)),
                    "previous" => trigger(OscCommand::PlaylistStep(m, -1)),
                    "shuffle" => Ok(Some(OscCommand::PlaylistShuffle(m, flag()?))),
                    "loop" => Ok(Some(OscCommand::PlaylistLoop(m, flag()?))),
                    _ => Err(format!("{} : {}", tr("adresse inconnue", "unknown address"), msg.addr)),
                },
                _ => Err(format!("{} : {}", tr("adresse inconnue", "unknown address"), msg.addr)),
            }
        }
        _ => Err(format!("{} : {}", tr("adresse inconnue", "unknown address"), msg.addr)),
    }
}

// ------------------------------------------------------------------ OSCQuery

const ACCESS_READ: u8 = 1;
const ACCESS_WRITE: u8 = 2;
const ACCESS_RW: u8 = 3;

/// Feuille de l'arborescence OSCQuery.
fn leaf(ty: &str, access: u8, description: &str, value: Option<Value>, range: Option<Value>) -> Value {
    let mut node = json!({ "TYPE": ty, "ACCESS": access, "DESCRIPTION": description });
    if let Some(v) = value {
        node["VALUE"] = json!([v]);
    }
    if let Some(r) = range {
        node["RANGE"] = json!([r]);
    }
    node
}

fn trigger_leaf(description: &str) -> Value {
    leaf("N", ACCESS_WRITE, description, None, None)
}

/// Place `node` à `path` en créant les dossiers intermédiaires.
fn insert(root: &mut Value, path: &str, mut node: Value) {
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

/// Arborescence complète, valeurs courantes comprises.
pub fn tree(prefix: &str, snap: &Snapshot) -> Value {
    let p = normalize_prefix(prefix);
    let mut root = json!({ "FULL_PATH": "/", "ACCESS": 0, "DESCRIPTION": "JingleStar", "CONTENTS": {} });
    let mut add = |path: String, node: Value| insert(&mut root, &format!("{p}{path}"), node);
    add("/stop_all".into(), trigger_leaf("Arrête tous les sons"));
    for (mi, m) in snap.modules.iter().enumerate() {
        let base = format!("/module/{}", mi + 1);
        add(format!("{base}/name"), leaf("s", ACCESS_READ, "Nom du module", Some(json!(m.name)), None));
        let mode = match m.mode {
            ModuleMode::Jingle => "jingle",
            ModuleMode::Playlist => "playlist",
            ModuleMode::Text => "text",
        };
        add(
            format!("{base}/mode"),
            leaf("s", ACCESS_RW, "Mode du module", Some(json!(mode)), Some(json!({ "VALS": ["jingle", "playlist", "text"] }))),
        );
        add(
            format!("{base}/volume"),
            leaf(
                "f",
                ACCESS_RW,
                "Volume du module (dB)",
                Some(json!(m.volume_db)),
                Some(json!({ "MIN": MODULE_DB.0, "MAX": MODULE_DB.1 })),
            ),
        );
        add(format!("{base}/stop"), trigger_leaf("Arrête tous les sons du module"));
        match m.mode {
            ModuleMode::Text => {}
            ModuleMode::Jingle => {
                for (si, j) in m.jingles.iter().enumerate() {
                    let jb = format!("{base}/jingle/{}", si + 1);
                    let name = j.as_ref().map(|j| j.name.as_str()).unwrap_or("");
                    add(format!("{jb}/name"), leaf("s", ACCESS_READ, "Nom du jingle", Some(json!(name)), None));
                    add(format!("{jb}/play"), trigger_leaf("Lance le jingle depuis le début"));
                    add(format!("{jb}/stop"), trigger_leaf("Arrête le jingle"));
                    add(format!("{jb}/toggle"), trigger_leaf("Lance ou arrête le jingle"));
                    add(
                        format!("{jb}/volume"),
                        leaf(
                            "f",
                            ACCESS_RW,
                            "Volume du jingle (dB)",
                            Some(json!(j.as_ref().map(|j| j.volume_db).unwrap_or(0.0))),
                            Some(json!({ "MIN": JINGLE_DB.0, "MAX": JINGLE_DB.1 })),
                        ),
                    );
                    let playing = j.as_ref().map(|j| j.playing).unwrap_or(false);
                    add(format!("{jb}/playing"), leaf(bool_type(playing), ACCESS_READ, "Le jingle joue", Some(json!(playing)), None));
                }
            }
            ModuleMode::Playlist => {
                let pl = &m.playlist;
                let pb = format!("{base}/playlist");
                add(format!("{pb}/play"), trigger_leaf("Lecture (un entier choisit le morceau)"));
                add(format!("{pb}/stop"), trigger_leaf("Arrête la playlist"));
                add(format!("{pb}/pause"), trigger_leaf("Pause / reprise"));
                add(format!("{pb}/paused"), leaf(bool_type(pl.paused), ACCESS_RW, "La playlist est en pause", Some(json!(pl.paused)), None));
                add(format!("{pb}/next"), trigger_leaf("Morceau suivant"));
                add(format!("{pb}/previous"), trigger_leaf("Morceau précédent"));
                add(
                    format!("{pb}/track"),
                    leaf(
                        "i",
                        ACCESS_RW,
                        "Numéro du morceau (écrire lance sa lecture)",
                        Some(json!(pl.current.map(|c| c + 1).unwrap_or(0))),
                        Some(json!({ "MIN": 1, "MAX": pl.tracks.max(1) })),
                    ),
                );
                add(format!("{pb}/tracks"), leaf("i", ACCESS_READ, "Nombre de morceaux", Some(json!(pl.tracks)), None));
                add(format!("{pb}/playing"), leaf(bool_type(pl.playing), ACCESS_READ, "La playlist joue", Some(json!(pl.playing)), None));
                add(format!("{pb}/shuffle"), leaf(bool_type(pl.shuffle), ACCESS_RW, "Lecture aléatoire", Some(json!(pl.shuffle)), None));
                add(format!("{pb}/loop"), leaf(bool_type(pl.looping), ACCESS_RW, "Boucle sur la liste", Some(json!(pl.looping)), None));
            }
        }
    }
    root
}

/// Dans OSC, le type d'un booléen porte sa valeur.
fn bool_type(b: bool) -> &'static str {
    if b {
        "T"
    } else {
        "F"
    }
}

const ATTRIBUTES: [&str; 7] = ["FULL_PATH", "CONTENTS", "TYPE", "ACCESS", "VALUE", "RANGE", "DESCRIPTION"];

/// Réponse HTTP à une requête OSCQuery : (code, corps JSON).
pub fn answer(url: &str, settings: &OscSettings, snap: &Snapshot) -> (u16, Value) {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if query == "HOST_INFO" {
        return (200, host_info(settings));
    }
    let root = tree(&settings.prefix, snap);
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

fn host_info(settings: &OscSettings) -> Value {
    json!({
        "NAME": settings.name,
        "OSC_PORT": settings.port,
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

// ------------------------------------------------------------------ serveurs

/// Serveurs réseau en marche ; tout s'arrête quand on le libère.
pub struct OscServer {
    pub commands: Receiver<OscCommand>,
    /// Messages refusés ou erreurs de réception, pour la barre d'état.
    pub errors: Receiver<String>,
    pub snapshot: Arc<Mutex<Snapshot>>,
    /// Ports effectivement ouverts.
    pub udp_port: Option<u16>,
    pub http_port: Option<u16>,
    /// Problèmes au démarrage (port occupé…).
    pub startup_errors: Vec<String>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    mdns: Option<mdns_sd::ServiceDaemon>,
}

impl OscServer {
    pub fn start(settings: OscSettings, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (cmd_tx, commands) = unbounded();
        let (err_tx, errors) = unbounded();
        let mut s = Self {
            commands,
            errors,
            snapshot: Arc::new(Mutex::new(Snapshot::default())),
            udp_port: None,
            http_port: None,
            startup_errors: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
            mdns: None,
        };
        match UdpSocket::bind(("0.0.0.0", settings.port)) {
            Ok(sock) => {
                let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
                s.udp_port = Some(settings.port);
                let (stop, prefix) = (s.stop.clone(), settings.prefix.clone());
                let repaint = repaint.clone();
                s.threads.push(
                    std::thread::Builder::new()
                        .name("osc-udp".into())
                        .spawn(move || udp_loop(sock, prefix, stop, cmd_tx, err_tx, repaint))
                        .expect("thread OSC"),
                );
            }
            Err(e) => s.startup_errors.push(format!("OSC : {} {} ({e})", tr("port UDP indisponible", "UDP port unavailable"), settings.port)),
        }
        if settings.oscquery {
            match tiny_http::Server::http(("0.0.0.0", settings.oscquery_port)) {
                Ok(http) => {
                    s.http_port = Some(settings.oscquery_port);
                    let (stop, snap, st) = (s.stop.clone(), s.snapshot.clone(), settings.clone());
                    s.threads.push(
                        std::thread::Builder::new()
                            .name("oscquery".into())
                            .spawn(move || http_loop(http, st, snap, stop))
                            .expect("thread OSCQuery"),
                    );
                    if settings.zeroconf {
                        match advertise(&settings) {
                            Ok(d) => s.mdns = Some(d),
                            Err(e) => s.startup_errors.push(format!("{} : {e}", tr("Annonce mDNS impossible", "mDNS announcement failed"))),
                        }
                    }
                }
                Err(e) => s.startup_errors.push(format!("OSCQuery : {} {} ({e})", tr("port HTTP indisponible", "HTTP port unavailable"), settings.oscquery_port)),
            }
        }
        s
    }

    pub fn publish(&self, snap: Snapshot) {
        if let Ok(mut s) = self.snapshot.lock() {
            if *s != snap {
                *s = snap;
            }
        }
    }
}

impl Drop for OscServer {
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

fn udp_loop(
    sock: UdpSocket,
    prefix: String,
    stop: Arc<AtomicBool>,
    commands: Sender<OscCommand>,
    errors: Sender<String>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let mut buf = vec![0u8; 65_536];
    while !stop.load(Ordering::Relaxed) {
        let n = match sock.recv_from(&mut buf) {
            Ok((n, _)) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            // Sous Windows, un ICMP « port injoignable » remonte en erreur de réception : on l'ignore.
            Err(_) => continue,
        };
        match rosc::decoder::decode_udp(&buf[..n]) {
            Ok((_, packet)) => {
                let mut messages = Vec::new();
                flatten(packet, &mut messages);
                for msg in messages {
                    match parse(&prefix, &msg) {
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

fn flatten(packet: OscPacket, out: &mut Vec<OscMessage>) {
    match packet {
        OscPacket::Message(m) => out.push(m),
        OscPacket::Bundle(b) => b.content.into_iter().for_each(|p| flatten(p, out)),
    }
}

fn http_loop(http: tiny_http::Server, settings: OscSettings, snap: Arc<Mutex<Snapshot>>, stop: Arc<AtomicBool>) {
    let json_header = tiny_http::Header::from_bytes("Content-Type", "application/json").expect("en-tête");
    let cors = tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").expect("en-tête");
    while !stop.load(Ordering::Relaxed) {
        let Ok(Some(req)) = http.recv_timeout(Duration::from_millis(200)) else { continue };
        let current = snap.lock().map(|s| s.clone()).unwrap_or_default();
        let (code, body) = answer(req.url(), &settings, &current);
        let resp = if code == 200 {
            tiny_http::Response::from_string(body.to_string()).with_header(json_header.clone())
        } else {
            tiny_http::Response::from_string(String::new())
        };
        let _ = req.respond(resp.with_status_code(code).with_header(cors.clone()));
    }
}

/// Adresse IP de l'ordinateur sur le réseau local (aucun paquet n'est envoyé).
pub fn local_ip() -> Option<std::net::IpAddr> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.connect(("8.8.8.8", 80)).ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

fn advertise(settings: &OscSettings) -> Result<mdns_sd::ServiceDaemon, mdns_sd::Error> {
    let daemon = mdns_sd::ServiceDaemon::new()?;
    let host = "jinglestar.local.";
    let query = mdns_sd::ServiceInfo::new("_oscjson._tcp.local.", &settings.name, host, (), settings.oscquery_port, None)?
        .enable_addr_auto();
    daemon.register(query)?;
    let osc = mdns_sd::ServiceInfo::new("_osc._udp.local.", &settings.name, host, (), settings.port, None)?.enable_addr_auto();
    daemon.register(osc)?;
    Ok(daemon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(addr: &str, args: Vec<OscType>) -> OscMessage {
        OscMessage { addr: addr.into(), args }
    }

    #[test]
    fn prefix_is_normalized() {
        assert_eq!(normalize_prefix("jinglestar/"), "/jinglestar");
        assert_eq!(normalize_prefix(" / "), "");
    }

    #[test]
    fn parses_commands() {
        let p = "/jinglestar";
        assert_eq!(parse(p, &msg("/jinglestar/stop_all", vec![])), Ok(Some(OscCommand::StopAll)));
        assert_eq!(parse(p, &msg("/jinglestar/module/2/jingle/12/play", vec![])), Ok(Some(OscCommand::JinglePlay(1, 11))));
        assert_eq!(
            parse(p, &msg("/jinglestar/module/1/volume", vec![OscType::Float(-6.0)])),
            Ok(Some(OscCommand::ModuleVolume(0, -6.0)))
        );
        assert_eq!(
            parse(p, &msg("/jinglestar/module/1/volume", vec![OscType::Float(40.0)])),
            Ok(Some(OscCommand::ModuleVolume(0, MODULE_DB.1)))
        );
        assert_eq!(
            parse(p, &msg("/jinglestar/module/3/playlist/play", vec![OscType::Int(4)])),
            Ok(Some(OscCommand::PlaylistPlay(2, Some(3))))
        );
        assert_eq!(parse(p, &msg("/jinglestar/module/3/playlist/previous", vec![])), Ok(Some(OscCommand::PlaylistStep(2, -1))));
        assert_eq!(parse(p, &msg("/jinglestar/module/2/playlist/pause", vec![])), Ok(Some(OscCommand::PlaylistPause(1, None))));
        assert_eq!(parse(p, &msg("/jinglestar/module/2/playlist/pause", vec![OscType::Int(0)])), Ok(None));
        assert_eq!(
            parse(p, &msg("/jinglestar/module/2/playlist/paused", vec![OscType::Bool(false)])),
            Ok(Some(OscCommand::PlaylistPause(1, Some(false))))
        );
        assert_eq!(
            parse(p, &msg("/jinglestar/module/1/playlist/shuffle", vec![OscType::Bool(true)])),
            Ok(Some(OscCommand::PlaylistShuffle(0, true)))
        );
        assert_eq!(
            parse(p, &msg("/jinglestar/module/1/mode", vec![OscType::String("playlist".into())])),
            Ok(Some(OscCommand::ModuleMode(0, ModuleMode::Playlist)))
        );
        assert_eq!(parse("", &msg("/stop_all", vec![])), Ok(Some(OscCommand::StopAll)));
    }

    #[test]
    fn button_release_is_ignored() {
        let m = msg("/jinglestar/module/1/jingle/1/toggle", vec![OscType::Float(0.0)]);
        assert_eq!(parse("/jinglestar", &m), Ok(None));
        let m = msg("/jinglestar/module/1/jingle/1/toggle", vec![OscType::Float(1.0)]);
        assert_eq!(parse("/jinglestar", &m), Ok(Some(OscCommand::JingleToggle(0, 0))));
    }

    #[test]
    fn rejects_bad_addresses() {
        assert!(parse("/jinglestar", &msg("/autre/stop_all", vec![])).is_err());
        assert!(parse("/jinglestar", &msg("/jinglestarx/stop_all", vec![])).is_err());
        assert!(parse("/jinglestar", &msg("/jinglestar/module/9/stop", vec![])).is_err());
        assert!(parse("/jinglestar", &msg("/jinglestar/module/1/jingle/13/play", vec![])).is_err());
    }

    fn snapshot() -> Snapshot {
        let mut jingles = vec![None; JINGLE_SLOTS];
        jingles[0] = Some(JingleState { name: "Intro".into(), volume_db: -3.0, playing: true });
        Snapshot {
            modules: vec![
                ModuleState {
                    name: "Module 1".into(),
                    mode: ModuleMode::Jingle,
                    volume_db: 0.0,
                    jingles,
                    playlist: PlaylistState::default(),
                },
                ModuleState {
                    name: "Musique".into(),
                    mode: ModuleMode::Playlist,
                    volume_db: -6.0,
                    jingles: vec![None; JINGLE_SLOTS],
                    playlist: PlaylistState { tracks: 5, current: Some(1), playing: true, ..Default::default() },
                },
            ],
        }
    }

    #[test]
    fn oscquery_tree() {
        let settings = OscSettings::default();
        let snap = snapshot();
        let (code, root) = answer("/", &settings, &snap);
        assert_eq!(code, 200);
        let jingle = &root["CONTENTS"]["jinglestar"]["CONTENTS"]["module"]["CONTENTS"]["1"]["CONTENTS"]["jingle"]["CONTENTS"]["1"];
        assert_eq!(jingle["CONTENTS"]["name"]["VALUE"][0], "Intro");
        assert_eq!(jingle["CONTENTS"]["play"]["FULL_PATH"], "/jinglestar/module/1/jingle/1/play");
        let (code, v) = answer("/jinglestar/module/2/playlist/track?VALUE", &settings, &snap);
        assert_eq!((code, v), (200, json!({ "VALUE": [2] })));
        let (code, v) = answer("/jinglestar/module/2/volume?RANGE", &settings, &snap);
        assert_eq!((code, v["RANGE"][0]["MIN"].as_f64()), (200, Some(-60.0)));
        assert_eq!(answer("/jinglestar/module/2/jingle", &settings, &snap).0, 404);
        assert_eq!(answer("/jinglestar/stop_all?VALUE", &settings, &snap).0, 204);
        let (_, info) = answer("/?HOST_INFO", &settings, &snap);
        assert_eq!(info["OSC_PORT"], 8000);
        assert_eq!(info["OSC_TRANSPORT"], "UDP");
    }

    #[test]
    fn server_roundtrip() {
        let settings = OscSettings { enabled: true, port: 39_871, oscquery_port: 39_872, zeroconf: false, ..Default::default() };
        let server = OscServer::start(settings, Arc::new(|| {}));
        assert!(server.startup_errors.is_empty(), "{:?}", server.startup_errors);
        server.publish(snapshot());

        let packet = OscPacket::Message(msg("/jinglestar/module/1/jingle/3/play", vec![]));
        let sock = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        sock.send_to(&rosc::encoder::encode(&packet).unwrap(), ("127.0.0.1", 39_871)).unwrap();
        let cmd = server.commands.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(cmd, OscCommand::JinglePlay(0, 2));

        use std::io::{Read, Write};
        let mut tcp = std::net::TcpStream::connect(("127.0.0.1", 39_872)).unwrap();
        tcp.write_all(b"GET /jinglestar/module/2/volume?VALUE HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        let mut resp = String::new();
        tcp.read_to_string(&mut resp).unwrap();
        assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
        assert!(resp.ends_with(r#"{"VALUE":[-6.0]}"#), "{resp}");
    }
}
