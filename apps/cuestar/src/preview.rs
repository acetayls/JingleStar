//! Aperçu navigable de la régie : un petit lecteur muet où l'on se déplace dans un média sans
//! toucher à la sortie, et la bande de vignettes de l'éditeur d'élément.
//!
//! Il tourne dans un processus à part (`--preview`) avec sa propre libmpv, comme les vignettes :
//! un fichier qui fait planter le décodeur ne touche ni la régie ni la sortie à l'antenne.
//! mpv y rend l'image en logiciel (API de rendu « sw ») dans une petite image RGBA.
//!
//! Dialogue : commandes JSON sur l'entrée standard ; sur la sortie standard, des messages
//! « type (1 octet) + longueur (4 octets) + contenu » : événement JSON (0) ou image (1).

use crate::output::mpv::{self, Handle, Mpv, RenderParam};
use serde::{Deserialize, Serialize};
use std::ffi::{c_int, c_void, CString};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Largeur de l'image de l'aperçu.
pub const LIVE_W: u32 = 640;
/// Largeur des vignettes de la frise.
pub const STRIP_W: u32 = 160;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PreviewCmd {
    /// Ouvre un média, en pause sur l'image à `at` secondes.
    Open {
        path: String,
        at: f64,
    },
    Close,
    /// `exact` faux : saut à l'image clé la plus proche (fluide pendant un glisser).
    Seek {
        seconds: f64,
        exact: bool,
    },
    Play,
    Pause,
    /// Image suivante ou précédente (en pause).
    Step {
        back: bool,
    },
    /// Une image toutes les durée / `count` secondes, pour la frise de l'éditeur.
    Strip {
        path: String,
        count: usize,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PreviewEvent {
    Ready { mpv: String },
    Missing { detail: String },
    Opened { path: String, duration: f64, image: bool },
    Position { time: f64, paused: bool },
    StripInfo { path: String, duration: f64, count: usize },
    Error { message: String },
}

/// Image envoyée : 0 = aperçu, 1 + i = vignette n° i de la frise.
struct FrameMsg {
    tag: u32,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

fn send_msg(out: &mut impl Write, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    out.write_all(&[kind])?;
    out.write_all(&(payload.len() as u32).to_le_bytes())?;
    out.write_all(payload)?;
    out.flush()
}

fn send_event(ev: &PreviewEvent) {
    if let Ok(s) = serde_json::to_vec(ev) {
        let _ = send_msg(&mut std::io::stdout().lock(), 0, &s);
    }
}

fn send_frame(f: &FrameMsg) {
    let mut buf = Vec::with_capacity(12 + f.rgba.len());
    buf.extend_from_slice(&f.tag.to_le_bytes());
    buf.extend_from_slice(&f.w.to_le_bytes());
    buf.extend_from_slice(&f.h.to_le_bytes());
    buf.extend_from_slice(&f.rgba);
    let _ = send_msg(&mut std::io::stdout().lock(), 1, &buf);
}

// --- Processus `--preview` -------------------------------------------------------------------

const RENDER_PARAM_SW_SIZE: c_int = 17;
const RENDER_PARAM_SW_FORMAT: c_int = 18;
const RENDER_PARAM_SW_STRIDE: c_int = 19;
const RENDER_PARAM_SW_POINTER: c_int = 20;

unsafe extern "C" fn on_update(data: *mut c_void) {
    let flag = unsafe { &*(data as *const AtomicBool) };
    flag.store(true, Ordering::Release);
}

/// Un lecteur mpv muet avec son contexte de rendu logiciel.
struct SwPlayer {
    rc: Handle,
    api: mpv::RenderApi,
    dirty: &'static AtomicBool,
    // Détruit après le contexte de rendu (ordre des champs).
    m: Mpv,
}

impl SwPlayer {
    fn new() -> Result<Self, String> {
        let m = Mpv::create()?;
        for (k, v) in [
            ("terminal", "no"),
            ("vo", "libmpv"),
            ("aid", "no"),
            ("sid", "no"),
            ("audio", "no"),
            ("hwdec", "no"),
            ("idle", "yes"),
            ("keep-open", "always"),
            ("pause", "yes"),
            ("hr-seek", "yes"),
            ("image-display-duration", "inf"),
            ("osd-level", "0"),
            ("input-default-bindings", "no"),
            ("demuxer-max-bytes", "32MiB"),
        ] {
            let _ = m.set_option(k, v);
        }
        m.initialize()?;
        let api = m.render_api().ok_or("API de rendu de mpv absente (libmpv trop ancienne)")?;
        let sw = CString::new("sw").unwrap_or_default();
        let mut params = [RenderParam { kind: mpv::RENDER_PARAM_API_TYPE, data: sw.as_ptr() as *mut c_void }, RenderParam::END];
        let mut rc: Handle = std::ptr::null_mut();
        let r = unsafe { (api.create)(&mut rc, m.raw(), params.as_mut_ptr()) };
        if r < 0 || rc.is_null() {
            return Err(format!("rendu logiciel de mpv indisponible (code {r})"));
        }
        let dirty: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        unsafe { (api.set_update_callback)(rc, Some(on_update), dirty as *const AtomicBool as *mut c_void) };
        Ok(Self { rc, api, dirty, m })
    }

    /// Taille de l'image affichée (après rotation et rapport d'aspect).
    fn video_size(&self) -> Option<(u32, u32)> {
        let get = |k| self.m.get_property(k).and_then(|s| s.parse::<f64>().ok()).filter(|v| *v > 0.0);
        Some((get("video-out-params/dw").or_else(|| get("dwidth"))? as u32, get("video-out-params/dh").or_else(|| get("dheight"))? as u32))
    }

    /// Rend l'image en cours si mpv en a une nouvelle, à `width` points de large.
    fn render_if_new(&self, width: u32) -> Option<(u32, u32, Vec<u8>)> {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return None;
        }
        let flags = unsafe { (self.api.update)(self.rc) };
        if flags & mpv::RENDER_UPDATE_FRAME == 0 {
            return None;
        }
        self.render(width)
    }

    fn render(&self, width: u32) -> Option<(u32, u32, Vec<u8>)> {
        let (vw, vh) = self.video_size()?;
        let w = width.min(vw).max(2) & !1;
        let h = ((vh as f64 * w as f64 / vw as f64).round() as u32).max(2) & !1;
        let mut size: [c_int; 2] = [w as c_int, h as c_int];
        let fmt = CString::new("rgb0").unwrap_or_default();
        let mut stride: usize = w as usize * 4;
        let mut buf = vec![0u8; stride * h as usize];
        let mut params = [
            RenderParam { kind: RENDER_PARAM_SW_SIZE, data: size.as_mut_ptr() as *mut c_void },
            RenderParam { kind: RENDER_PARAM_SW_FORMAT, data: fmt.as_ptr() as *mut c_void },
            RenderParam { kind: RENDER_PARAM_SW_STRIDE, data: &mut stride as *mut usize as *mut c_void },
            RenderParam { kind: RENDER_PARAM_SW_POINTER, data: buf.as_mut_ptr() as *mut c_void },
            RenderParam::END,
        ];
        if unsafe { (self.api.render)(self.rc, params.as_mut_ptr()) } < 0 {
            return None;
        }
        for px in buf.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Some((w, h, buf))
    }

    fn get_f64(&self, k: &str) -> Option<f64> {
        self.m.get_property(k).and_then(|s| s.parse().ok())
    }

    /// Vide la file d'événements ; vrai si un fichier vient d'être chargé.
    fn drain(&self) -> (bool, bool) {
        let (mut loaded, mut failed) = (false, false);
        while let Some(ev) = self.m.next_event() {
            match ev.event_id {
                mpv::EVENT_FILE_LOADED => loaded = true,
                mpv::EVENT_END_FILE => {
                    let end = unsafe { &*(ev.data as *const mpv::RawEndFile) };
                    failed |= end.reason == mpv::END_FILE_ERROR;
                }
                _ => {}
            }
        }
        (loaded, failed)
    }
}

impl Drop for SwPlayer {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.rc) };
    }
}

/// Frise en cours de fabrication.
struct StripJob {
    path: String,
    count: usize,
    next: usize,
    duration: f64,
    /// Attente de l'image demandée (saut lancé à cet instant).
    waiting: Option<Instant>,
    loaded: bool,
}

/// Mode `--preview` : sert l'aperçu jusqu'à la fermeture de l'entrée standard.
pub fn run() {
    let live = match SwPlayer::new() {
        Ok(p) => p,
        Err(e) => {
            send_event(&PreviewEvent::Missing { detail: e });
            return;
        }
    };
    let strip = SwPlayer::new().ok();
    send_event(&PreviewEvent::Ready { mpv: live.m.get_property("mpv-version").unwrap_or_default() });

    let (tx, rx): (Sender<Option<PreviewCmd>>, Receiver<Option<PreviewCmd>>) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if let Ok(c) = serde_json::from_str(&line) {
                if tx.send(Some(c)).is_err() {
                    return;
                }
            }
        }
        let _ = tx.send(None);
    });

    let mut current: Option<String> = None;
    let mut job: Option<StripJob> = None;
    let mut last_pos = Instant::now();
    let mut sent_pos: Option<(f64, bool)> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(8)) {
            Ok(None) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Some(cmd)) => match cmd {
                PreviewCmd::Open { path, at } => {
                    let _ = live.m.set_property("pause", "yes");
                    let _ = live.m.set_property("start", &format!("{:.3}", at.max(0.0)));
                    if let Err(e) = live.m.command(&["loadfile", &path, "replace"]) {
                        send_event(&PreviewEvent::Error { message: e });
                    }
                    current = Some(path);
                    sent_pos = None;
                }
                PreviewCmd::Close => {
                    let _ = live.m.command(&["stop"]);
                    current = None;
                }
                PreviewCmd::Seek { seconds, exact } => {
                    let _ = live.m.command(&["seek", &format!("{:.3}", seconds.max(0.0)), if exact { "absolute+exact" } else { "absolute+keyframes" }]);
                }
                PreviewCmd::Play => {
                    let _ = live.m.set_property("pause", "no");
                }
                PreviewCmd::Pause => {
                    let _ = live.m.set_property("pause", "yes");
                }
                PreviewCmd::Step { back } => {
                    let _ = live.m.command(&[if back { "frame-back-step" } else { "frame-step" }]);
                }
                PreviewCmd::Strip { path, count } => {
                    if let Some(s) = &strip {
                        let _ = s.m.set_property("start", "0");
                        if s.m.command(&["loadfile", &path, "replace"]).is_ok() {
                            job = Some(StripJob { path, count: count.clamp(1, 200), next: 0, duration: 0.0, waiting: None, loaded: false });
                        }
                    }
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
        }

        // Aperçu : nouvel état du fichier, position, image.
        let (loaded, failed) = live.drain();
        if loaded {
            let image = live.m.get_property("current-tracks/video/image").as_deref() == Some("yes");
            send_event(&PreviewEvent::Opened { path: current.clone().unwrap_or_default(), duration: live.get_f64("duration").unwrap_or(0.0), image });
        }
        if failed {
            send_event(&PreviewEvent::Error { message: format!("aperçu impossible ({})", current.clone().unwrap_or_default()) });
        }
        if current.is_some() && last_pos.elapsed() >= Duration::from_millis(40) {
            last_pos = Instant::now();
            let t = live.get_f64("time-pos").unwrap_or(0.0);
            let paused = live.m.get_property("pause").as_deref() != Some("no");
            if sent_pos.is_none_or(|(st, sp)| (t - st).abs() > 0.001 || paused != sp) {
                sent_pos = Some((t, paused));
                send_event(&PreviewEvent::Position { time: t, paused });
            }
        }
        if let Some((w, h, rgba)) = live.render_if_new(LIVE_W) {
            send_frame(&FrameMsg { tag: 0, w, h, rgba });
        }

        // Frise : une vignette à la fois, sans gêner l'aperçu.
        if let (Some(s), Some(j)) = (&strip, &mut job) {
            let (loaded, failed) = s.drain();
            if failed {
                job = None;
                continue;
            }
            if loaded {
                j.loaded = true;
                j.duration = s.get_f64("duration").unwrap_or(0.0);
                send_event(&PreviewEvent::StripInfo { path: j.path.clone(), duration: j.duration, count: j.count });
            }
            if !j.loaded {
                continue;
            }
            match j.waiting {
                None => {
                    let t = (j.next as f64 + 0.5) * j.duration / j.count as f64;
                    s.dirty.store(false, Ordering::Release);
                    let _ = s.m.command(&["seek", &format!("{t:.3}"), "absolute+keyframes"]);
                    j.waiting = Some(Instant::now());
                }
                Some(since) => {
                    let frame = s.render_if_new(STRIP_W);
                    let late = since.elapsed() > Duration::from_secs(3);
                    if frame.is_some() || late {
                        if let Some((w, h, rgba)) = frame.or_else(|| if late { s.render(STRIP_W) } else { None }) {
                            send_frame(&FrameMsg { tag: 1 + j.next as u32, w, h, rgba });
                        }
                        j.next += 1;
                        j.waiting = None;
                        if j.next >= j.count || j.duration <= 0.0 {
                            let _ = s.m.command(&["stop"]);
                            job = None;
                        }
                    }
                }
            }
        }
    }
}

// --- Côté régie -------------------------------------------------------------------------------

/// Image reçue (RGBA), avec un numéro qui change à chaque nouvelle image.
#[derive(Default)]
struct Pixels {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
    serial: u64,
}

#[derive(Default)]
struct Shared {
    live: Pixels,
    strip: Vec<Option<Pixels>>,
    strip_serial: u64,
    events: Vec<PreviewEvent>,
}

/// État de l'aperçu tel que la régie le voit.
#[derive(Default, Clone)]
pub struct PreviewState {
    pub available: bool,
    pub missing: Option<String>,
    /// Média ouvert dans l'aperçu, sa durée et sa nature.
    pub path: Option<PathBuf>,
    pub duration: f64,
    pub image: bool,
    pub time: f64,
    pub paused: bool,
    pub error: Option<String>,
    /// Frise prête pour ce média : (chemin, durée, nombre de vignettes).
    pub strip: Option<(PathBuf, f64, usize)>,
}

/// L'aperçu côté régie : lance le processus, lui envoie les commandes, garde la dernière image.
pub struct Preview {
    child: Option<std::process::Child>,
    stdin: Option<std::process::ChildStdin>,
    shared: Arc<Mutex<Shared>>,
    pub state: PreviewState,
    live_tex: Option<(egui::TextureHandle, u64)>,
    strip_tex: Vec<Option<egui::TextureHandle>>,
    strip_seen: u64,
    started: bool,
    /// Dernière demande de déplacement envoyée (pour ne pas inonder le processus).
    last_seek: Option<(f64, bool)>,
    /// Position approchée pendant un glisser, à rendre exacte au relâché.
    pending_exact: Option<f64>,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            child: None,
            stdin: None,
            shared: Arc::default(),
            state: PreviewState::default(),
            live_tex: None,
            strip_tex: Vec::new(),
            strip_seen: 0,
            started: false,
            last_seek: None,
            pending_exact: None,
        }
    }
}

fn read_msg(r: &mut impl Read) -> Option<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head).ok()?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > 64 << 20 {
        return None;
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    Some((head[0], buf))
}

impl Preview {
    /// Lance le processus d'aperçu à la première utilisation.
    fn ensure(&mut self, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        let Ok(exe) = std::env::current_exe() else { return };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--preview").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let Ok(mut child) = cmd.spawn() else {
            self.state.missing = Some("processus d'aperçu impossible à lancer".into());
            return;
        };
        self.stdin = child.stdin.take();
        if let Some(mut stdout) = child.stdout.take() {
            let shared = self.shared.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new()
                .name("aperçu".into())
                .spawn(move || {
                    while let Some((kind, buf)) = read_msg(&mut stdout) {
                        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                        if kind == 0 {
                            if let Ok(ev) = serde_json::from_slice::<PreviewEvent>(&buf) {
                                s.events.push(ev);
                            }
                        } else if buf.len() >= 12 {
                            let u = |i: usize| u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]) as usize;
                            let (tag, w, h) = (u(0), u(4), u(8));
                            if buf.len() >= 12 + w * h * 4 {
                                let px = Pixels { w, h, rgba: buf[12..12 + w * h * 4].to_vec(), serial: 0 };
                                if tag == 0 {
                                    let serial = s.live.serial + 1;
                                    s.live = Pixels { serial, ..px };
                                } else {
                                    let i = tag - 1;
                                    if s.strip.len() <= i {
                                        s.strip.resize_with(i + 1, || None);
                                    }
                                    s.strip[i] = Some(px);
                                    s.strip_serial += 1;
                                }
                            }
                        }
                        drop(s);
                        ctx.request_repaint();
                    }
                    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    s.events.push(PreviewEvent::Missing { detail: "le processus d'aperçu s'est arrêté".into() });
                    ctx.request_repaint();
                })
                .ok();
        }
        self.child = Some(child);
    }

    fn send(&mut self, ctx: &egui::Context, c: PreviewCmd) {
        self.ensure(ctx);
        let Some(stdin) = &mut self.stdin else { return };
        if let Ok(s) = serde_json::to_string(&c) {
            if writeln!(stdin, "{s}").and_then(|_| stdin.flush()).is_err() {
                self.stdin = None;
            }
        }
    }

    /// À appeler à chaque image de la régie : lit les événements du processus.
    pub fn poll(&mut self) {
        let events = std::mem::take(&mut self.shared.lock().unwrap_or_else(|e| e.into_inner()).events);
        for ev in events {
            match ev {
                PreviewEvent::Ready { .. } => {
                    self.state.available = true;
                    self.state.missing = None;
                }
                PreviewEvent::Missing { detail } => {
                    self.state.available = false;
                    self.state.missing = Some(detail);
                }
                PreviewEvent::Opened { path, duration, image } => {
                    if self.state.path.as_deref() == Some(Path::new(&path)) {
                        self.state.duration = duration;
                        self.state.image = image;
                        self.state.error = None;
                    }
                }
                PreviewEvent::Position { time, paused } => {
                    self.state.time = time;
                    self.state.paused = paused;
                }
                PreviewEvent::StripInfo { path, duration, count } => self.state.strip = Some((PathBuf::from(path), duration, count)),
                PreviewEvent::Error { message } => self.state.error = Some(message),
            }
        }
    }

    /// Ouvre `path` dans l'aperçu (sans effet s'il y est déjà).
    pub fn open(&mut self, ctx: &egui::Context, path: &Path, at: f64) {
        if self.state.path.as_deref() == Some(path) {
            return;
        }
        self.state.path = Some(path.to_path_buf());
        self.state.duration = 0.0;
        self.state.time = at;
        self.state.image = false;
        self.state.error = None;
        self.last_seek = None;
        self.live_tex = None;
        self.send(ctx, PreviewCmd::Open { path: path.to_string_lossy().into_owned(), at });
    }

    pub fn seek(&mut self, ctx: &egui::Context, seconds: f64, exact: bool) {
        self.pending_exact = (!exact).then_some(seconds);
        if self.last_seek == Some((seconds, exact)) {
            return;
        }
        self.last_seek = Some((seconds, exact));
        self.state.time = seconds;
        self.send(ctx, PreviewCmd::Seek { seconds, exact });
    }

    /// À chaque image : un glisser terminé ramène l'aperçu sur l'image exacte.
    pub fn settle(&mut self, ctx: &egui::Context) {
        if self.pending_exact.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            if let Some(t) = self.pending_exact.take() {
                self.seek(ctx, t, true);
            }
        }
    }

    pub fn play(&mut self, ctx: &egui::Context, on: bool) {
        self.last_seek = None;
        self.send(ctx, if on { PreviewCmd::Play } else { PreviewCmd::Pause });
    }

    pub fn step(&mut self, ctx: &egui::Context, back: bool) {
        self.last_seek = None;
        self.send(ctx, PreviewCmd::Step { back });
    }

    /// Vrai si l'aperçu montre `path` et en connaît la durée.
    pub fn shows(&self, path: &Path) -> bool {
        self.state.path.as_deref() == Some(path) && (self.state.duration > 0.0 || self.state.image)
    }

    /// Demande la frise de `path` (vignettes réparties sur toute la durée).
    pub fn request_strip(&mut self, ctx: &egui::Context, path: &Path, count: usize) {
        if self.state.strip.as_ref().is_some_and(|s| s.0 == path) {
            return;
        }
        self.state.strip = Some((path.to_path_buf(), 0.0, count));
        {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            s.strip.clear();
            s.strip_serial += 1;
        }
        self.strip_tex.clear();
        self.send(ctx, PreviewCmd::Strip { path: path.to_string_lossy().into_owned(), count });
    }

    /// Image de l'aperçu, pour le média ouvert.
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = s.live.serial > 0 && self.live_tex.as_ref().is_none_or(|(_, n)| *n != s.live.serial);
        if fresh && s.live.w > 0 {
            let img = egui::ColorImage::from_rgba_unmultiplied([s.live.w, s.live.h], &s.live.rgba);
            let serial = s.live.serial;
            drop(s);
            match &mut self.live_tex {
                Some((t, n)) => {
                    t.set(img, egui::TextureOptions::LINEAR);
                    *n = serial;
                }
                None => self.live_tex = Some((ctx.load_texture("apercu", img, egui::TextureOptions::LINEAR), serial)),
            }
        }
        self.live_tex.as_ref().map(|(t, _)| t)
    }

    /// Vignettes de la frise reçues jusqu'ici (None : pas encore arrivée).
    pub fn strip_textures(&mut self, ctx: &egui::Context) -> &[Option<egui::TextureHandle>] {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if s.strip_serial != self.strip_seen {
            self.strip_seen = s.strip_serial;
            self.strip_tex.resize_with(s.strip.len(), || None);
            for (i, px) in s.strip.iter().enumerate() {
                if let (Some(px), None) = (px, &self.strip_tex[i]) {
                    let img = egui::ColorImage::from_rgba_unmultiplied([px.w, px.h], &px.rgba);
                    self.strip_tex[i] = Some(ctx.load_texture(format!("frise-{i}"), img, egui::TextureOptions::LINEAR));
                }
            }
        }
        &self.strip_tex
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.stdin = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
