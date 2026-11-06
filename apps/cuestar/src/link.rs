//! Côté régie : lance le moteur de sortie et dialogue avec lui.

use crate::prefs::MachinePrefs;
use crate::protocol::{Command, Event, Monitor};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver};

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
    pub dropped: i64,
    pub delayed: i64,
    pub last_error: Option<String>,
    /// Diaporama en cours : (index, nombre, chemin).
    pub slide: Option<(usize, usize, String)>,
}

pub struct OutputLink {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Option<Receiver<Option<Event>>>,
    pub state: OutputState,
}

impl OutputLink {
    pub fn new() -> Self {
        Self { child: None, stdin: None, rx: None, state: OutputState::default() }
    }

    /// (Re)lance le moteur de sortie avec les préférences de la machine.
    pub fn start(&mut self, prefs: &MachinePrefs, repaint: impl Fn() + Send + 'static) {
        self.stop();
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                self.state.last_error = Some(format!("exécutable introuvable : {e}"));
                return;
            }
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--output").arg(serde_json::to_string(prefs).unwrap_or_default());
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
                        if tx.send(Some(ev)).is_err() {
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

    pub fn send(&mut self, cmd: Command) {
        let Some(stdin) = self.stdin.as_mut() else { return };
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
            let Some(ev) = msg else {
                closed = true;
                break;
            };
            let s = &mut self.state;
            match ev {
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
                    s.dropped = dropped;
                    s.delayed = delayed;
                }
                Event::Ended | Event::Idle => {
                    s.current = None;
                    s.time = 0.0;
                    s.duration = 0.0;
                }
                Event::Error { message } => s.last_error = Some(message),
                Event::Slide { index, count, path } => s.slide = (count > 0).then_some((index, count, path)),
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

impl Drop for OutputLink {
    fn drop(&mut self) {
        self.stop();
    }
}
