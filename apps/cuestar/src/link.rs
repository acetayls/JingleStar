//! Côté régie : lance le moteur de sortie et dialogue avec lui.

use crate::prefs::MachinePrefs;
use crate::protocol::{Command, Event, Monitor};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Vignette de monitoring : largeur, hauteur, pixels RGBA, numéro d'image.
#[derive(Default)]
pub struct MonitorFrame {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    pub serial: u64,
}

/// État de la sortie tel que la régie le connaît.
#[derive(Default, Clone)]
pub struct OutputState {
    pub running: bool,
    pub ready: bool,
    pub mpv: Option<String>,
    pub mpv_missing: Option<String>,
    pub monitor: Option<Monitor>,
    pub fullscreen: bool,
    pub monitors: Vec<Monitor>,
    pub current: Option<String>,
    pub time: f64,
    pub duration: f64,
    pub paused: bool,
    pub hwdec: String,
    /// Images perdues et retardées depuis la dernière remise à zéro, tous médias confondus.
    pub dropped: i64,
    pub delayed: i64,
    /// Compteurs bruts de mpv pour le média en cours (repartent de zéro à chaque fichier).
    pub media_dropped: i64,
    pub media_delayed: i64,
    pub last_error: Option<String>,
    /// Diaporama en cours : (index, nombre, chemin).
    pub slide: Option<(usize, usize, String)>,
    /// Compteur de fins de média (le Lecteur enchaîne quand il change).
    pub ended: u64,
    pub video_fps: f64,
    /// Sorties audio vues par mpv : (nom, description).
    pub audio_devices: Vec<(String, String)>,
    pub display_fps: f64,
    /// Touches reçues de la fenêtre de sortie, pas encore traitées (module Présentation).
    pub keys: Vec<crate::protocol::RemoteKey>,
    /// Temps entre le dernier chargement envoyé et sa première image à l'écran (ms).
    pub go_latency_ms: Option<f64>,
}

pub struct OutputLink {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Option<Receiver<Option<(Instant, Event)>>>,
    /// Heure d'envoi du dernier chargement (pour mesurer son arrivée à l'écran).
    load_sent: Option<Instant>,
    /// Change à chaque commande qui peut occuper le lecteur en attente (chargement, arrêt,
    /// diaporama, mire, relance) : la préparation du cue suivant est alors à refaire.
    pub generation: u64,
    pub state: OutputState,
    /// Dernière vignette reçue de la sortie.
    pub monitor: Arc<Mutex<MonitorFrame>>,
}

impl OutputLink {
    pub fn new() -> Self {
        Self { child: None, stdin: None, rx: None, load_sent: None, generation: 0, state: OutputState::default(), monitor: Arc::new(Mutex::new(MonitorFrame::default())) }
    }

    /// (Re)lance le moteur de sortie avec les préférences de la machine.
    pub fn start(&mut self, prefs: &MachinePrefs, repaint: Arc<dyn Fn() + Send + Sync>) {
        self.stop();
        self.generation += 1;
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                self.state.last_error = Some(format!("exécutable introuvable : {e}"));
                return;
            }
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--output").arg(serde_json::to_string(prefs).unwrap_or_default());
        // Canal du monitoring : la sortie s'y connecte quand la régie demande des vignettes.
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) {
            if let Ok(addr) = listener.local_addr() {
                cmd.arg(addr.port().to_string());
                let frame = self.monitor.clone();
                let repaint = repaint.clone();
                std::thread::Builder::new()
                    .name("monitoring".into())
                    .spawn(move || monitor_reader(listener, frame, repaint))
                    .ok();
            }
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.state.last_error = Some(format!("lancement de la sortie impossible : {e}"));
                return;
            }
        };
        let stdout = child.stdout.take().expect("stdout piped");
        self.stdin = child.stdin.take();
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("sortie-evenements".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(ev) = serde_json::from_str::<Event>(&line) {
                        if tx.send(Some((Instant::now(), ev))).is_err() {
                            return;
                        }
                        repaint();
                    }
                }
                let _ = tx.send(None);
                repaint();
            })
            .ok();
        self.child = Some(child);
        self.rx = Some(rx);
        self.state = OutputState { running: true, ..Default::default() };
    }

    pub fn stop(&mut self) {
        self.send(Command::Quit);
        self.stdin = None;
        if let Some(mut c) = self.child.take() {
            // Laisse une seconde au moteur pour fermer proprement, puis tranche.
            for _ in 0..20 {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
        self.rx = None;
        self.state.running = false;
        self.state.ready = false;
    }

    /// Remet à zéro le compteur d'images perdues et retardées (la sortie n'est pas touchée).
    pub fn reset_drops(&mut self) {
        self.state.dropped = 0;
        self.state.delayed = 0;
    }

    pub fn send(&mut self, cmd: Command) {
        let Some(stdin) = self.stdin.as_mut() else { return };
        if matches!(cmd, Command::Load { .. }) {
            self.load_sent = Some(Instant::now());
        }
        if matches!(cmd, Command::Load { .. } | Command::Stop | Command::Slideshow { .. } | Command::Pattern { .. }) {
            self.generation += 1;
        }
        let mut line = serde_json::to_string(&cmd).unwrap_or_default();
        line.push('\n');
        if stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).is_err() {
            self.stdin = None;
        }
    }

    /// Lit les événements reçus depuis la dernière image.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut closed = false;
        while let Ok(msg) = rx.try_recv() {
            let Some((at, ev)) = msg else {
                closed = true;
                break;
            };
            let s = &mut self.state;
            match ev {
                Event::OnScreen => {
                    if let Some(t) = self.load_sent.take() {
                        s.go_latency_ms = Some(at.saturating_duration_since(t).as_secs_f64() * 1000.0);
                    }
                }
                Event::Ready { mpv, monitor, fullscreen, monitors } => {
                    s.ready = true;
                    s.mpv = mpv;
                    s.monitor = Some(monitor);
                    s.fullscreen = fullscreen;
                    s.monitors = monitors;
                }
                Event::MpvMissing { detail } => s.mpv_missing = Some(detail),
                Event::Loaded { path } => {
                    s.current = Some(path);
                    s.time = 0.0;
                }
                Event::Position { time, duration } => {
                    s.time = time;
                    s.duration = duration;
                }
                Event::Paused { paused } => s.paused = paused,
                Event::Hwdec { name } => s.hwdec = name,
                Event::Drops { dropped, delayed } => {
                    // mpv repart de zéro à chaque fichier (et chaque lecteur a ses compteurs) :
                    // un compteur qui baisse est un nouveau départ, sa valeur s'ajoute entière.
                    let add = |total: &mut i64, last: i64, now: i64| *total += if now >= last { now - last } else { now.max(0) };
                    add(&mut s.dropped, s.media_dropped, dropped);
                    add(&mut s.delayed, s.media_delayed, delayed);
                    s.media_dropped = dropped;
                    s.media_delayed = delayed;
                }
                Event::Ended | Event::Idle => {
                    if matches!(ev, Event::Ended) {
                        s.ended += 1;
                    }
                    s.current = None;
                    s.time = 0.0;
                    s.duration = 0.0;
                }
                Event::Error { message } => s.last_error = Some(message),
                Event::AudioDevices { list } => s.audio_devices = list,
                Event::Fps { video, display } => {
                    s.video_fps = video;
                    s.display_fps = display;
                }
                Event::Slide { index, count, path } => s.slide = (count > 0).then_some((index, count, path)),
                Event::Key { key } => s.keys.push(key),
            }
        }
        if closed {
            self.rx = None;
            self.stdin = None;
            if let Some(mut c) = self.child.take() {
                let _ = c.wait();
            }
            self.state.running = false;
            self.state.ready = false;
        }
    }
}

fn monitor_reader(listener: TcpListener, frame: Arc<Mutex<MonitorFrame>>, repaint: Arc<dyn Fn() + Send + Sync>) {
    use std::io::Read;
    // Une seule connexion par sortie lancée ; elle se ferme avec la sortie.
    let Ok((mut s, _)) = listener.accept() else { return };
    let mut head = [0u8; 8];
    while s.read_exact(&mut head).is_ok() {
        let w = u32::from_le_bytes(head[0..4].try_into().unwrap()) as usize;
        let h = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        if w == 0 || h == 0 || w * h > 4096 * 4096 {
            return;
        }
        let mut px = vec![0u8; w * h * 4];
        if s.read_exact(&mut px).is_err() {
            return;
        }
        if let Ok(mut f) = frame.lock() {
            f.w = w;
            f.h = h;
            f.rgba = px;
            f.serial += 1;
        }
        repaint();
    }
}

impl Drop for OutputLink {
    fn drop(&mut self) {
        self.stop();
    }
}
