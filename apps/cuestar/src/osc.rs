//! Commande OSC de CueStar, sur le serveur commun de `star_remote::osc` (repris de
//! JingleStar) : mêmes adresses que le Diaporama d'origine (préfixe `/diaporama` accepté
//! aussi), plus le Lecteur, la Cuelist et le noir général. En option : description OSCQuery
//! de l'état courant et annonce mDNS ; « <préfixe>/subscribe » abonne au retour d'état.

use rosc::{OscMessage, OscType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use star_remote::osc::{bool_type, insert, leaf, trigger_leaf, ServerConfig, ACCESS_READ, ACCESS_RW, ACCESS_WRITE};
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
    /// Serveur HTTP OSCQuery qui décrit les adresses et l'état courant.
    pub oscquery: bool,
    pub oscquery_port: u16,
    /// Annonce mDNS (Bonjour) pour la découverte automatique.
    pub zeroconf: bool,
    /// Nom annoncé sur le réseau.
    pub name: String,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self { enabled: false, port: 9000, network: false, prefix: "/cuestar".into(), oscquery: false, oscquery_port: 9001, zeroconf: false, name: "CueStar".into() }
    }
}

pub(crate) fn settings_path() -> Option<std::path::PathBuf> {
    crate::project::APP.config_file("osc.json")
}

impl OscSettings {
    /// Réglages OSC de cette machine.
    pub fn load() -> Self {
        settings_path().and_then(|p| star_core::prefs::load(&p)).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Some(p) = settings_path() {
            let _ = star_core::prefs::save(&p, self);
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

/// Adresse reçue → message de CueStar : préfixe de CueStar, ou celui du Diaporama d'origine.
pub fn parse(prefix: &str, m: &OscMessage) -> Option<Msg> {
    let prefix = star_remote::osc::normalize_prefix(prefix);
    let path = match m.addr.strip_prefix(&prefix).filter(|r| r.starts_with('/')) {
        Some(r) => r.trim_start_matches('/').to_string(),
        None => format!("diaporama/{}", m.addr.strip_prefix("/diaporama/")?),
    };
    (!path.is_empty()).then(|| Msg { path, args: m.args.clone() })
}

/// État publié pour OSCQuery et le retour d'état.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// Média à l'écran (nom de fichier), vide au noir.
    pub media: String,
    pub paused: bool,
    /// Position et durée du média en secondes (durée 0 : image fixe).
    pub time: f64,
    pub duration: f64,
    pub lecteur: LecteurState,
    pub cuelist: CuelistState,
    /// Diaporama : média n° (à partir de 1) sur le total, 0 sans diaporama.
    pub slide: (usize, usize),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LecteurState {
    pub items: usize,
    /// Élément en cours, compté à partir de 1 (0 : aucun).
    pub playing: usize,
    pub shuffle: bool,
    pub looping: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CuelistState {
    pub cues: usize,
    /// Cue en attente, compté à partir de 1 (0 : conduite terminée).
    pub standby: usize,
    pub standby_name: String,
    /// Dernier cue lancé.
    pub running_name: String,
}

impl Snapshot {
    /// Temps restant en secondes, au dixième ; -1 sans média minuté.
    pub fn remaining(&self) -> f64 {
        if self.duration > 0.0 {
            ((self.duration - self.time).max(0.0) * 10.0).round() / 10.0
        } else {
            -1.0
        }
    }
}

/// Arborescence OSCQuery : toutes les adresses de [`HELP`] et l'état courant.
pub fn tree(prefix: &str, snap: &Snapshot) -> Value {
    let p = star_remote::osc::normalize_prefix(prefix);
    let mut root = star_remote::osc::root("CueStar");
    let mut add = |path: &str, node: Value| insert(&mut root, &format!("{p}/{path}"), node);
    let idx = |d: &str, n: usize| leaf("i", ACCESS_WRITE, d, None, Some(json!({ "MIN": 1, "MAX": n.max(1) })));
    let flag = |d: &str, b: bool| leaf(bool_type(b), ACCESS_RW, d, Some(json!(b)), None);
    let read = |ty: &str, d: &str, v: Value| leaf(ty, ACCESS_READ, d, Some(v), None);
    add("noir", trigger_leaf("Écran de sortie au noir"));
    add("output/drops/reset", trigger_leaf("Remet à zéro le compteur d'images perdues et retardées"));
    add("subscribe", leaf("i", ACCESS_WRITE, "Abonnement au retour d'état (5 s, à renouveler) ; argument : port de réponse", None, None));
    add("media", read("s", "Média à l'écran", json!(snap.media)));
    add("paused", read(bool_type(snap.paused), "En pause", json!(snap.paused)));
    add("time", read("f", "Position du média (s)", json!((snap.time * 10.0).round() / 10.0)));
    add("remaining", read("f", "Temps restant (s, -1 : image fixe ou rien)", json!(snap.remaining())));
    for t in ["go", "pause", "play", "toggle", "stop", "next", "prev"] {
        add(&format!("lecteur/{t}"), trigger_leaf(&format!("Lecteur : {t}")));
        add(&format!("cuelist/{t}"), trigger_leaf(&format!("Cuelist : {t}")));
    }
    let l = &snap.lecteur;
    add("lecteur/goto", idx("Lance l'élément n° i", l.items));
    add("lecteur/shuffle", flag("Ordre aléatoire", l.shuffle));
    add("lecteur/loop", flag("Tout boucler", l.looping));
    add("lecteur/transition", leaf("sf", ACCESS_WRITE, "Transition (cut, black, fade) et durée", None, None));
    add("lecteur/items", read("i", "Nombre d'éléments", json!(l.items)));
    add("lecteur/playing", read("i", "Élément en cours (0 : aucun)", json!(l.playing)));
    let c = &snap.cuelist;
    add("cuelist/standby", leaf("i", ACCESS_RW, "Cue en attente (0 : conduite terminée)", Some(json!(c.standby)), Some(json!({ "MIN": 1, "MAX": c.cues.max(1) }))));
    add("cuelist/standby_name", read("s", "Nom du cue en attente", json!(c.standby_name)));
    add("cuelist/running_name", read("s", "Dernier cue lancé", json!(c.running_name)));
    add("cuelist/cues", read("i", "Nombre de cues", json!(c.cues)));
    for t in ["play", "pause", "toggle", "stop", "next", "prev"] {
        add(&format!("diaporama/{t}"), trigger_leaf(&format!("Diaporama : {t}")));
    }
    add("diaporama/goto", idx("Média n° i", snap.slide.1));
    add("diaporama/index", read("i", "Média en cours (0 : aucun)", json!(snap.slide.0)));
    add("diaporama/count", read("i", "Nombre de médias", json!(snap.slide.1)));
    for s in ["shuffle", "loop", "recursive", "video_sound"] {
        add(&format!("diaporama/settings/{s}"), leaf("T", ACCESS_WRITE, "Réglage du Diaporama (0 ou 1)", None, None));
    }
    add("diaporama/settings/photo_duration", leaf("f", ACCESS_WRITE, "Durée des photos (s)", None, Some(json!({ "MIN": 1, "MAX": 60 }))));
    add("diaporama/settings/fit", leaf("s", ACCESS_WRITE, "Ajustement (contain, cover, auto)", None, None));
    add("diaporama/settings/folder", leaf("s", ACCESS_WRITE, "Dossier à lire", None, None));
    root
}

/// Retour d'état envoyé aux abonnés.
pub fn feedback(prefix: &str, snap: &Snapshot) -> Vec<OscMessage> {
    let p = star_remote::osc::normalize_prefix(prefix);
    let m = |a: &str, v: OscType| OscMessage { addr: format!("{p}/{a}"), args: vec![v] };
    vec![
        m("media", OscType::String(snap.media.clone())),
        m("paused", OscType::Bool(snap.paused)),
        m("time", OscType::Float(snap.time as f32)),
        m("remaining", OscType::Float(snap.remaining() as f32)),
        m("lecteur/playing", OscType::Int(snap.lecteur.playing as i32)),
        m("cuelist/standby", OscType::Int(snap.cuelist.standby as i32)),
        m("cuelist/standby_name", OscType::String(snap.cuelist.standby_name.clone())),
        m("diaporama/index", OscType::Int(snap.slide.0 as i32)),
    ]
}

/// Serveurs réseau en marche ; tout s'arrête quand on le libère.
pub struct OscServer {
    pub server: Option<star_remote::osc::OscServer<Msg, Snapshot>>,
    pub settings: OscSettings,
}

impl OscServer {
    pub fn start(s: &OscSettings, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        if !s.enabled {
            return Self { server: None, settings: s.clone() };
        }
        let cfg = ServerConfig {
            name: s.name.clone(),
            port: s.port,
            network: s.network,
            prefix: s.prefix.clone(),
            oscquery_port: s.oscquery.then_some(s.oscquery_port),
            zeroconf: (s.oscquery && s.zeroconf).then(|| "cuestar.local.".to_string()),
        };
        let (p1, p2) = (s.prefix.clone(), s.prefix.clone());
        let server = star_remote::osc::OscServer::start(cfg, Arc::new(move |m| Ok(parse(&p1, m))), Arc::new(move |snap| tree(&p2, snap)), repaint);
        Self { server: Some(server), settings: s.clone() }
    }

    /// Messages reçus depuis la dernière image.
    pub fn poll(&self) -> Vec<Msg> {
        self.server.as_ref().map(|s| s.commands.try_iter().collect()).unwrap_or_default()
    }

    /// Publie l'état (OSCQuery) et l'envoie aux abonnés.
    pub fn publish(&self, snap: Snapshot) {
        let Some(s) = self.server.as_ref() else { return };
        if s.has_subscribers() {
            s.send_to_subscribers(feedback(&self.settings.prefix, &snap));
        }
        s.publish(snap);
    }
}

/// Liste de référence des adresses (plus affichée dans la fenêtre, comme dans JingleStar).
#[allow(dead_code)]
pub const HELP: &[(&str, &str)] = &[
    ("noir", "écran de sortie au noir"),
    ("output/drops/reset", "remet à zéro le compteur d'images perdues et retardées"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(addr: &str) -> OscMessage {
        OscMessage { addr: addr.into(), args: vec![OscType::Int(3)] }
    }

    #[test]
    fn addresses_of_cuestar_and_diaporama() {
        assert_eq!(parse("/cuestar", &msg("/cuestar/lecteur/goto")).map(|m| m.path), Some("lecteur/goto".into()));
        assert_eq!(parse("cuestar/", &msg("/cuestar/noir")).map(|m| m.path), Some("noir".into()));
        assert_eq!(parse("/cuestar", &msg("/diaporama/next")).map(|m| m.path), Some("diaporama/next".into()));
        assert_eq!(parse("/diaporama", &msg("/diaporama/next")).map(|m| m.path), Some("next".into()));
        assert!(parse("/cuestar", &msg("/cuestarx/noir")).is_none());
        assert!(parse("/cuestar", &msg("/autre/noir")).is_none());
        assert!(parse("/cuestar", &msg("/cuestar")).is_none());
    }

    #[test]
    fn oscquery_describes_help_and_state() {
        let snap = Snapshot {
            media: "intro.mp4".into(),
            time: 12.0,
            duration: 30.04,
            cuelist: CuelistState { cues: 3, standby: 2, standby_name: "Pluie".into(), running_name: "Ouverture".into() },
            ..Default::default()
        };
        let root = tree("/cuestar", &snap);
        let get = |path: &str| {
            let mut n = &root;
            for seg in path.split('/').filter(|s| !s.is_empty()) {
                n = &n["CONTENTS"][seg];
            }
            n.clone()
        };
        for (a, _) in HELP {
            for (k, addr) in a.split(" | ").enumerate() {
                let first = addr.split(' ').next().unwrap();
                // « lecteur/pause | play » : les suivantes sont relatives au même dossier.
                let full = if k == 0 { first.to_string() } else { format!("{}/{first}", a.rsplit_once('/').unwrap().0) };
                if full.contains('<') || full.contains("settings/shuffle") {
                    continue;
                }
                assert!(get(&format!("/cuestar/{full}"))["FULL_PATH"].is_string(), "{full} absent de l'arborescence");
            }
        }
        assert_eq!(get("/cuestar/media")["VALUE"][0], "intro.mp4");
        assert_eq!(get("/cuestar/remaining")["VALUE"][0].as_f64(), Some(18.0));
        assert_eq!(get("/cuestar/cuelist/standby_name")["VALUE"][0], "Pluie");
        let (_, info) = star_remote::osc::answer("/?HOST_INFO", "CueStar", 9000, || root.clone());
        assert_eq!(info["NAME"], "CueStar");
        assert_eq!(feedback("/cuestar", &snap)[0].addr, "/cuestar/media");
    }
}
