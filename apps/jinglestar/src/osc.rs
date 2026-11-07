//! Pilotage à distance : adresses OSC de JingleStar et arborescence OSCQuery de son état.
//! Le serveur (UDP, abonnements, HTTP, mDNS) est celui de `star_remote::osc`.
//!
//! Les numéros de module, de jingle et de morceau sont comptés à partir de 1 dans les adresses.

use std::sync::Arc;

use rosc::{OscMessage, OscType};
use serde_json::{json, Value};
use star_remote::osc::{arg_bool, arg_f32, insert, is_press, leaf, trigger_leaf, ServerConfig, ACCESS_READ, ACCESS_RW, ACCESS_WRITE};
pub use star_remote::osc::{bool_type, local_ip, normalize_prefix};

use crate::audio::mixer::{JINGLE_SLOTS, MAX_MODULES};
use star_core::i18n::tr;
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
    /// GO d'une conduite ; `None` : le premier module GO.
    CueGo(Option<usize>),
    /// Déplace le cue en attente.
    CueStep(usize, isize),
    /// Met le cue n° en attente.
    CueStandby(usize, usize),
    /// Pause des cues qui sonnent : état imposé, ou bascule (`None`).
    CuePause(usize, Option<bool>),
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
    pub cues: CueListState,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CueListState {
    pub names: Vec<String>,
    /// Cue en attente (index) ; `== names.len()` : fin de la conduite.
    pub standby: usize,
    /// Nombre de cues qui sonnent.
    pub playing: usize,
    /// Nom du dernier cue lancé qui sonne encore.
    pub playing_name: String,
    /// Temps restant de ce cue (s).
    pub remaining: Option<f32>,
    /// Avancement de ce cue (0 à 1).
    pub progress: Option<f32>,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JingleState {
    pub name: String,
    pub volume_db: f32,
    pub playing: bool,
    pub color: [u8; 3],
    /// Temps restant (s) ; `None` à l'arrêt ou en boucle.
    pub remaining: Option<f32>,
    /// Avancement entre les points de début et de fin (0 à 1) ; `None` à l'arrêt.
    pub progress: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistState {
    pub tracks: usize,
    pub current: Option<usize>,
    /// Titre du morceau sélectionné (nom du fichier).
    pub title: String,
    /// Temps restant du morceau en cours (s).
    pub remaining: Option<f32>,
    /// Avancement du morceau en cours (0 à 1).
    pub progress: Option<f32>,
    pub playing: bool,
    pub paused: bool,
    pub shuffle: bool,
    pub looping: bool,
}

// ------------------------------------------------------------------ adresses

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
        ["go"] => trigger(OscCommand::CueGo(None)),
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
                        Some(OscType::String(s)) if s.eq_ignore_ascii_case("go") => ModuleMode::Go,
                        _ => return Err(format!("{} : {}", tr("mode attendu (jingle, playlist, text ou go)", "mode expected (jingle, playlist, text or go)"), msg.addr)),
                    };
                    Ok(Some(OscCommand::ModuleMode(m, mode)))
                }
                ["go"] => trigger(OscCommand::CueGo(Some(m))),
                ["cue", action] => match *action {
                    "next" => trigger(OscCommand::CueStep(m, 1)),
                    "previous" => trigger(OscCommand::CueStep(m, -1)),
                    "pause" => trigger(OscCommand::CuePause(m, None)),
                    "paused" => Ok(Some(OscCommand::CuePause(m, Some(flag()?)))),
                    "standby" => {
                        let n = value()?;
                        if n < 1.0 {
                            return Err(format!("{} : {n}", tr("numéro de cue invalide", "invalid cue number")));
                        }
                        Ok(Some(OscCommand::CueStandby(m, n as usize - 1)))
                    }
                    _ => Err(format!("{} : {}", tr("adresse inconnue", "unknown address"), msg.addr)),
                },
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

/// Avancement de 0 à 1, au millième ; -1 quand rien ne joue.
fn progress_leaf(description: &str, progress: Option<f32>) -> Value {
    let v = progress.map(|p| (p.clamp(0.0, 1.0) * 1000.0).round() / 1000.0).unwrap_or(-1.0);
    leaf("f", ACCESS_READ, description, Some(json!(v)), None)
}

/// Temps restant en secondes, au dixième ; -1 quand il n'y en a pas.
fn remaining_leaf(description: &str, remaining: Option<f32>) -> Value {
    let v = remaining.map(|r| (r.max(0.0) * 10.0).round() / 10.0).unwrap_or(-1.0);
    leaf("f", ACCESS_READ, description, Some(json!(v)), None)
}

/// Arborescence complète, valeurs courantes comprises.
pub fn tree(prefix: &str, snap: &Snapshot) -> Value {
    let p = normalize_prefix(prefix);
    let mut root = star_remote::osc::root("JingleStar");
    let mut add = |path: String, node: Value| insert(&mut root, &format!("{p}{path}"), node);
    add("/stop_all".into(), trigger_leaf("Arrête tous les sons"));
    add("/go".into(), trigger_leaf("GO du premier module GO"));
    add(
        "/subscribe".into(),
        leaf("i", ACCESS_WRITE, "Reçoit les niveaux (…/module/<m>/meter, dB G et D) pendant 5 s ; argument : port de réception", None, None),
    );
    for (mi, m) in snap.modules.iter().enumerate() {
        let base = format!("/module/{}", mi + 1);
        add(format!("{base}/name"), leaf("s", ACCESS_READ, "Nom du module", Some(json!(m.name)), None));
        let mode = match m.mode {
            ModuleMode::Jingle => "jingle",
            ModuleMode::Playlist => "playlist",
            ModuleMode::Text => "text",
            ModuleMode::Go => "go",
        };
        add(
            format!("{base}/mode"),
            leaf("s", ACCESS_RW, "Mode du module", Some(json!(mode)), Some(json!({ "VALS": ["jingle", "playlist", "text", "go"] }))),
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
            ModuleMode::Go => {
                let c = &m.cues;
                add(format!("{base}/go"), trigger_leaf("Lance le cue en attente et passe au suivant"));
                add(format!("{base}/cue/next"), trigger_leaf("Cue suivant en attente"));
                add(format!("{base}/cue/previous"), trigger_leaf("Cue précédent en attente"));
                add(
                    format!("{base}/cue/standby"),
                    leaf(
                        "i",
                        ACCESS_RW,
                        "Numéro du cue en attente (0 : fin de la cuelist)",
                        Some(json!(if c.standby < c.names.len() { c.standby + 1 } else { 0 })),
                        Some(json!({ "MIN": 1, "MAX": c.names.len().max(1) })),
                    ),
                );
                let standby_name = c.names.get(c.standby).cloned().unwrap_or_default();
                add(format!("{base}/cue/name"), leaf("s", ACCESS_READ, "Nom du cue en attente", Some(json!(standby_name)), None));
                add(format!("{base}/cue/count"), leaf("i", ACCESS_READ, "Nombre de cues", Some(json!(c.names.len())), None));
                add(format!("{base}/cue/playing"), leaf("i", ACCESS_READ, "Nombre de cues qui sonnent", Some(json!(c.playing)), None));
                add(
                    format!("{base}/cue/playing_name"),
                    leaf("s", ACCESS_READ, "Nom du dernier cue lancé qui sonne", Some(json!(c.playing_name)), None),
                );
                add(format!("{base}/cue/remaining"), remaining_leaf("Temps restant du cue qui sonne (s, -1 : aucun)", c.remaining));
                add(format!("{base}/cue/progress"), progress_leaf("Avancement du cue qui sonne (0 à 1, -1 : aucun)", c.progress));
                add(format!("{base}/cue/pause"), trigger_leaf("Pause / reprise des cues qui sonnent"));
                add(format!("{base}/cue/paused"), leaf(bool_type(c.paused), ACCESS_RW, "La cuelist est en pause", Some(json!(c.paused)), None));
            }
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
                    let color = j.as_ref().map(|j| format!("#{:02x}{:02x}{:02x}", j.color[0], j.color[1], j.color[2])).unwrap_or_default();
                    add(format!("{jb}/color"), leaf("s", ACCESS_READ, "Couleur du jingle (#rrggbb)", Some(json!(color)), None));
                    add(
                        format!("{jb}/remaining"),
                        remaining_leaf("Temps restant (s, -1 : à l'arrêt ou en boucle)", j.as_ref().and_then(|j| j.remaining)),
                    );
                    add(format!("{jb}/progress"), progress_leaf("Avancement (0 à 1, -1 : à l'arrêt)", j.as_ref().and_then(|j| j.progress)));
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
                add(format!("{pb}/title"), leaf("s", ACCESS_READ, "Titre du morceau sélectionné", Some(json!(pl.title)), None));
                add(format!("{pb}/remaining"), remaining_leaf("Temps restant du morceau (s, -1 : à l'arrêt)", pl.remaining));
                add(format!("{pb}/progress"), progress_leaf("Avancement du morceau (0 à 1, -1 : à l'arrêt)", pl.progress));
                add(format!("{pb}/playing"), leaf(bool_type(pl.playing), ACCESS_READ, "La playlist joue", Some(json!(pl.playing)), None));
                add(format!("{pb}/shuffle"), leaf(bool_type(pl.shuffle), ACCESS_RW, "Lecture aléatoire", Some(json!(pl.shuffle)), None));
                add(format!("{pb}/loop"), leaf(bool_type(pl.looping), ACCESS_RW, "Boucle sur la liste", Some(json!(pl.looping)), None));
            }
        }
    }
    root
}

/// Réponse HTTP à une requête OSCQuery : (code, corps JSON).
#[cfg(test)]
pub fn answer(url: &str, settings: &OscSettings, snap: &Snapshot) -> (u16, Value) {
    star_remote::osc::answer(url, &settings.name, settings.port, || tree(&settings.prefix, snap))
}

// ------------------------------------------------------------------ serveur

/// Serveurs réseau en marche ; tout s'arrête quand on le libère.
pub type OscServer = star_remote::osc::OscServer<OscCommand, Snapshot>;

/// Démarre les serveurs selon les préférences (commandes acceptées depuis le réseau).
pub fn start(settings: OscSettings, repaint: Arc<dyn Fn() + Send + Sync>) -> OscServer {
    let cfg = ServerConfig {
        name: settings.name.clone(),
        port: settings.port,
        network: true,
        prefix: settings.prefix.clone(),
        oscquery_port: settings.oscquery.then_some(settings.oscquery_port),
        zeroconf: settings.zeroconf.then(|| "jinglestar.local.".to_string()),
    };
    let prefix = settings.prefix.clone();
    let tree_prefix = settings.prefix;
    OscServer::start(cfg, Arc::new(move |m| parse(&prefix, m)), Arc::new(move |snap| tree(&tree_prefix, snap)), repaint)
}

/// Envoie le niveau de chaque module (dB, gauche et droite) aux abonnés.
pub fn send_levels(server: &OscServer, levels: &[[f32; 2]]) {
    let messages = levels
        .iter()
        .enumerate()
        .map(|(m, l)| OscMessage {
            addr: format!("{}/module/{}/meter", server.prefix, m + 1),
            args: vec![OscType::Float(l[0].max(-120.0)), OscType::Float(l[1].max(-120.0))],
        })
        .collect();
    server.send_to_subscribers(messages);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::OscPacket;
    use std::net::UdpSocket;
    use std::time::{Duration, Instant};

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
        // Valeur non numérique : refusée, le volume ne bouge pas.
        for bad in [OscType::String("nan".into()), OscType::Float(f32::NAN), OscType::Double(f64::INFINITY), OscType::String("inf".into())] {
            assert!(parse(p, &msg("/jinglestar/module/1/volume", vec![bad.clone()])).is_err(), "{bad:?}");
            assert!(parse(p, &msg("/jinglestar/module/1/jingle/1/volume", vec![bad])).is_err());
        }
        assert_eq!(parse(p, &msg("/jinglestar/module/3/playlist/previous", vec![])), Ok(Some(OscCommand::PlaylistStep(2, -1))));
        assert_eq!(parse(p, &msg("/jinglestar/module/2/playlist/pause", vec![])), Ok(Some(OscCommand::PlaylistPause(1, None))));
        assert_eq!(parse(p, &msg("/jinglestar/go", vec![])), Ok(Some(OscCommand::CueGo(None))));
        assert_eq!(parse(p, &msg("/jinglestar/module/4/go", vec![OscType::Int(1)])), Ok(Some(OscCommand::CueGo(Some(3)))));
        assert_eq!(parse(p, &msg("/jinglestar/module/4/go", vec![OscType::Int(0)])), Ok(None));
        assert_eq!(parse(p, &msg("/jinglestar/module/4/cue/previous", vec![])), Ok(Some(OscCommand::CueStep(3, -1))));
        assert_eq!(parse(p, &msg("/jinglestar/module/4/cue/standby", vec![OscType::Int(5)])), Ok(Some(OscCommand::CueStandby(3, 4))));
        assert_eq!(parse(p, &msg("/jinglestar/module/4/cue/pause", vec![])), Ok(Some(OscCommand::CuePause(3, None))));
        assert_eq!(
            parse(p, &msg("/jinglestar/module/4/cue/paused", vec![OscType::Bool(true)])),
            Ok(Some(OscCommand::CuePause(3, Some(true))))
        );
        assert_eq!(
            parse(p, &msg("/jinglestar/module/1/mode", vec![OscType::String("go".into())])),
            Ok(Some(OscCommand::ModuleMode(0, ModuleMode::Go)))
        );
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
        jingles[0] = Some(JingleState { name: "Intro".into(), volume_db: -3.0, playing: true, color: [255, 128, 0], remaining: Some(4.26), progress: Some(0.25) });
        Snapshot {
            modules: vec![
                ModuleState {
                    name: "Module 1".into(),
                    mode: ModuleMode::Jingle,
                    volume_db: 0.0,
                    jingles,
                    playlist: PlaylistState::default(),
                    cues: CueListState::default(),
                },
                ModuleState {
                    name: "Musique".into(),
                    mode: ModuleMode::Playlist,
                    volume_db: -6.0,
                    jingles: vec![None; JINGLE_SLOTS],
                    playlist: PlaylistState { tracks: 5, current: Some(1), playing: true, ..Default::default() },
                    cues: CueListState::default(),
                },
                ModuleState {
                    name: "Conduite".into(),
                    mode: ModuleMode::Go,
                    volume_db: 0.0,
                    jingles: vec![None; JINGLE_SLOTS],
                    playlist: PlaylistState::default(),
                    cues: CueListState {
                        names: vec!["Ouverture".into(), "Pluie".into()],
                        standby: 1,
                        playing: 1,
                        playing_name: "Ouverture".into(),
                        ..Default::default()
                    },
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
        let (code, v) = answer("/jinglestar/module/3/cue/standby?VALUE", &settings, &snap);
        assert_eq!((code, v), (200, json!({ "VALUE": [2] })));
        let (_, v) = answer("/jinglestar/module/3/cue/name?VALUE", &settings, &snap);
        assert_eq!(v, json!({ "VALUE": ["Pluie"] }));
        assert_eq!(jingle["CONTENTS"]["color"]["VALUE"][0], "#ff8000");
        assert_eq!(jingle["CONTENTS"]["progress"]["VALUE"][0].as_f64(), Some(0.25));
        assert_eq!(jingle["CONTENTS"]["remaining"]["VALUE"][0].as_f64(), Some(4.3f32 as f64));
        let (_, v) = answer("/jinglestar/module/2/playlist/remaining?VALUE", &settings, &snap);
        assert_eq!(v, json!({ "VALUE": [-1.0] }));
        let (_, v) = answer("/jinglestar/module/3/cue/playing_name?VALUE", &settings, &snap);
        assert_eq!(v, json!({ "VALUE": ["Ouverture"] }));
        let (_, info) = answer("/?HOST_INFO", &settings, &snap);
        assert_eq!(info["OSC_PORT"], 8000);
        assert_eq!(info["OSC_TRANSPORT"], "UDP");
    }

    #[test]
    fn server_roundtrip() {
        let settings = OscSettings { enabled: true, port: 39_871, oscquery_port: 39_872, zeroconf: false, ..Default::default() };
        let server = start(settings, Arc::new(|| {}));
        assert!(server.startup_errors.is_empty(), "{:?}", server.startup_errors);
        server.publish(snapshot());

        // Abonnement aux niveaux : réponse sur le port d'envoi.
        let listener = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        listener.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let sub = OscPacket::Message(msg("/jinglestar/subscribe", vec![]));
        listener.send_to(&rosc::encoder::encode(&sub).unwrap(), ("127.0.0.1", 39_871)).unwrap();
        let t0 = Instant::now();
        while !server.has_subscribers() && t0.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        send_levels(&server, &[[-6.0, -12.0]]);
        let mut buf = [0u8; 1024];
        let (n, _) = listener.recv_from(&mut buf).unwrap();
        let OscPacket::Bundle(b) = rosc::decoder::decode_udp(&buf[..n]).unwrap().1 else { panic!("paquet groupé attendu") };
        let OscPacket::Message(m) = &b.content[0] else { panic!("message attendu") };
        assert_eq!(m.addr, "/jinglestar/module/1/meter");
        assert_eq!(m.args, vec![OscType::Float(-6.0), OscType::Float(-12.0)]);

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
