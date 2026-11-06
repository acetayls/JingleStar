//! Moteur de sortie : processus séparé de la régie.
//!
//! Il ouvre la fenêtre de sortie (plein écran sur l'écran choisi) et y fait rendre libmpv
//! directement au GPU. Les commandes arrivent sur l'entrée standard, les événements repartent
//! sur la sortie standard (voir `protocol`). Une régie qui rame ne retarde aucune image.

mod monitor;
pub mod mpv;
mod pattern;
mod slideshow;
pub mod warp;

use crate::prefs::MachinePrefs;
use crate::protocol::{Command, Event, Fit, Monitor, TransitionKind};
use slideshow::Show;
use mpv::Mpv;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::{BufRead, Write};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowId};

enum UserEv {
    Cmd(Command),
    /// mpv a des événements à lire.
    Mpv,
    /// La régie a fermé le lien (fermeture ou plantage).
    Detached,
}

fn emit(ev: &Event) {
    let mut out = std::io::stdout().lock();
    if let Ok(s) = serde_json::to_string(ev) {
        let _ = writeln!(out, "{s}");
        let _ = out.flush();
    }
}

unsafe extern "C" fn mpv_wakeup(data: *mut c_void) {
    let proxy = unsafe { &*(data as *const EventLoopProxy<UserEv>) };
    let _ = proxy.send_event(UserEv::Mpv);
}

// Identifiants des propriétés observées.
const P_TIME: u64 = 1;
const P_DURATION: u64 = 2;
const P_PAUSE: u64 = 3;
const P_HWDEC: u64 = 4;
const P_DROPS: u64 = 5;
const P_DELAYED: u64 = 6;
const P_VFPS: u64 = 7;
const P_DFPS: u64 = 8;

struct FadeAnim {
    from: f64,
    to: f64,
    start: Instant,
    secs: f64,
}

struct PendingLoad {
    path: String,
    paused: bool,
    looping: bool,
    hold: bool,
}

struct OutputApp {
    prefs: MachinePrefs,
    // Ordre des champs = ordre de destruction : mpv avant le proxy qu'il référence.
    mpv: Option<Mpv>,
    proxy: Box<EventLoopProxy<UserEv>>,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    pattern_on: bool,
    show: Option<Show>,
    shaders: warp::ShaderFiles,
    /// Coins du mapping en cours (None : image droite).
    warp_corners: Option<[[f32; 2]; 4]>,
    /// Le shader porte le réglage de niveau (fondu au noir possible).
    fade_ok: bool,
    fade_level: f64,
    anim: Option<FadeAnim>,
    /// Média à charger une fois le fondu sortant terminé.
    pending: Option<PendingLoad>,
    /// Durée du fondu d'ouverture à lancer dès la première image du média chargé.
    fade_in_after: Option<f64>,
    monitor_port: Option<u16>,
    feed: Option<monitor::MonitorFeed>,
    monitor_every: Option<Duration>,
    monitor_next: Instant,
    duration: f64,
    last_pos: Instant,
    drops: (i64, i64),
    fps: (f64, f64),
    last_fps: Instant,
    current: Option<String>,
}

fn describe(m: &MonitorHandle, primary: &Option<MonitorHandle>) -> Monitor {
    let size = m.size();
    Monitor {
        name: m.name().unwrap_or_else(|| "écran".into()),
        width: size.width,
        height: size.height,
        refresh_hz: m.refresh_rate_millihertz().map(|r| r as f64 / 1000.0).unwrap_or(0.0),
        primary: primary.as_ref() == Some(m),
    }
}

impl OutputApp {
    fn open_window(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        let primary = el.primary_monitor();
        let monitors: Vec<MonitorHandle> = el.available_monitors().collect();
        let chosen = self
            .prefs
            .screen
            .as_ref()
            .and_then(|name| monitors.iter().find(|m| m.name().as_ref() == Some(name)).cloned())
            .or_else(|| monitors.iter().find(|m| Some(*m) != primary.as_ref()).cloned())
            .or_else(|| primary.clone())
            .or_else(|| monitors.first().cloned());
        let single = monitors.len() <= 1;
        let fullscreen = !(single && self.prefs.windowed_if_single);

        let mut attrs = Window::default_attributes().with_title("CueStar — Sortie");
        if fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(chosen.clone())));
        } else {
            attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(960.0, 540.0));
        }
        let window = Arc::new(el.create_window(attrs).map_err(|e| format!("fenêtre de sortie : {e}"))?);
        if fullscreen {
            window.set_cursor_visible(false);
        }
        self.window = Some(window.clone());

        let mpv_version = match self.start_mpv(&window) {
            Ok(v) => Some(v),
            Err(e) => {
                emit(&Event::MpvMissing { detail: e });
                self.pattern_on = true;
                let ctx = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
                self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).map_err(|e| e.to_string())?);
                None
            }
        };
        let monitor = chosen.map(|m| describe(&m, &primary)).unwrap_or_default();
        emit(&Event::Ready { mpv: mpv_version, monitor, fullscreen, monitors: monitors.iter().map(|m| describe(m, &primary)).collect() });
        self.emit_audio_devices();
        window.request_redraw();
        Ok(())
    }

    fn start_mpv(&mut self, window: &Window) -> Result<String, String> {
        let m = Mpv::create()?;
        let wid: i64 = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Win32(h) => h.hwnd.get() as i64,
            RawWindowHandle::Xlib(h) => h.window as i64,
            RawWindowHandle::Xcb(h) => h.window.get() as i64,
            RawWindowHandle::AppKit(h) => h.ns_view.as_ptr() as i64,
            other => return Err(format!("type de fenêtre non géré pour l'incrustation mpv : {other:?}")),
        };
        let p = &self.prefs;
        m.set_option("wid", &wid.to_string())?;
        m.set_option("vo", if p.light { "gpu" } else { "gpu-next" })?;
        if let Some(api) = p.render.mpv_gpu_api() {
            m.set_option("gpu-api", api)?;
        }
        m.set_option("hwdec", p.decode.mpv_hwdec())?;
        if let Some(dev) = &p.audio_device {
            let _ = m.set_option("audio-device", dev);
        }
        let _ = m.set_option("volume", &format!("{:.1}", p.volume.clamp(0.0, 100.0)));
        // Réglages « régie » : pas d'interface mpv, pas de clavier, fenêtre toujours là.
        for (k, v) in [
            ("terminal", "no"),
            ("idle", "yes"),
            ("force-window", "yes"),
            ("keep-open", "yes"),
            ("osc", "no"),
            ("osd-level", "0"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("input-cursor", "no"),
            ("cursor-autohide", "always"),
            ("image-display-duration", "inf"),
            ("audio-display", "no"),
            ("hr-seek", "yes"),
            ("stop-screensaver", "yes"),
            ("background-color", "#000000"),
        ] {
            let _ = m.set_option(k, v);
        }
        if p.display_sync {
            // Cale la lecture sur la fréquence réelle de l'écran : pas de saccade 24/25/50/60.
            let _ = m.set_option("video-sync", "display-resample");
        }
        if p.light {
            let _ = m.set_option("profile", "fast");
        }
        m.initialize()?;
        m.observe(P_TIME, "time-pos", mpv::FORMAT_DOUBLE);
        m.observe(P_DURATION, "duration", mpv::FORMAT_DOUBLE);
        m.observe(P_PAUSE, "pause", mpv::FORMAT_FLAG);
        m.observe(P_HWDEC, "hwdec-current", mpv::FORMAT_STRING);
        m.observe(P_DROPS, "frame-drop-count", mpv::FORMAT_INT64);
        m.observe(P_DELAYED, "vo-delayed-frame-count", mpv::FORMAT_INT64);
        m.observe(P_VFPS, "estimated-vf-fps", mpv::FORMAT_DOUBLE);
        m.observe(P_DFPS, "display-fps", mpv::FORMAT_DOUBLE);
        m.set_wakeup(mpv_wakeup, &*self.proxy as *const EventLoopProxy<UserEv> as *mut c_void);
        let version = m.get_property("mpv-version").unwrap_or_else(|| format!("mpv (API {})", m.api_version()));
        self.mpv = Some(m);
        self.warp_corners = self.prefs.warp.active();
        self.apply_warp();
        Ok(version)
    }

    fn paint_cpu(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else { return };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buf) = surface.buffer_mut() {
            if self.pattern_on {
                pattern::draw(&mut buf, w.get() as usize, h.get() as usize);
            } else {
                buf.fill(0);
            }
            let _ = buf.present();
        }
    }

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        if let Command::Quit = cmd {
            el.exit();
            return;
        }
        if let Command::Monitor { on } = cmd {
            self.monitor_every = on.then(|| self.monitor_interval());
            if on && !self.feed.as_ref().is_some_and(|f| f.alive()) {
                self.feed = self.monitor_port.map(monitor::MonitorFeed::connect);
            }
            self.monitor_next = Instant::now();
            return;
        }
        if let Command::Warp { corners } = cmd {
            self.warp_corners = corners;
            self.apply_warp();
            return;
        }
        if let Command::Pattern { kind } = cmd {
            self.end_show();
            self.pattern_on = kind.is_some();
            if let Some(m) = &self.mpv {
                let res = if let Some(k) = kind {
                    let (w, h) = self.window.as_ref().map(|w| (w.inner_size().width, w.inner_size().height)).unwrap_or((1920, 1080));
                    let _ = m.set_property("panscan", "0");
                    let _ = m.set_property("loop-file", "no");
                    let _ = m.set_property("pause", "no");
                    m.command(&["loadfile", &k.lavfi(w, h), "replace"])
                } else {
                    m.command(&["stop"])
                };
                if let Err(e) = res {
                    emit(&Event::Error { message: e });
                }
            } else if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }
        if self.mpv.is_none() {
            emit(&Event::Error { message: "lecture impossible : libmpv n'est pas chargé".into() });
            return;
        }
        let res = match cmd {
            Command::Load { path, paused, looping, hold, transition } => {
                self.end_show();
                let load = PendingLoad { path, paused, looping, hold };
                let black = transition.kind != TransitionKind::Cut && self.fade_ok && transition.secs > 0.0;
                if !black {
                    self.anim = None;
                    self.pending = None;
                    self.fade_in_after = None;
                    self.set_fade(1.0);
                    self.do_load(load)
                } else {
                    let half = transition.secs / 2.0;
                    if self.current.is_some() && self.fade_level > 0.0 {
                        // Fondu sortant, puis chargement, puis fondu d'ouverture.
                        self.anim = Some(FadeAnim { from: self.fade_level, to: 0.0, start: Instant::now(), secs: half });
                        self.pending = Some(load);
                        self.fade_in_after = Some(half);
                        Ok(())
                    } else {
                        self.anim = None;
                        self.set_fade(0.0);
                        self.fade_in_after = Some(half);
                        self.do_load(load)
                    }
                }
            }
            Command::Play => {
                if let Some(s) = &mut self.show {
                    s.resume();
                }
                self.m().set_property("pause", "no")
            }
            Command::Pause => {
                if let Some(s) = &mut self.show {
                    s.pause();
                }
                self.m().set_property("pause", "yes")
            }
            Command::Stop => {
                self.end_show();
                self.anim = None;
                self.pending = None;
                self.fade_in_after = None;
                self.set_fade(1.0);
                self.m().command(&["stop"])
            }
            Command::Seek { seconds } => self.m().command(&["seek", &format!("{seconds:.3}"), "absolute+exact"]),
            Command::Volume { value } => self.m().set_property("volume", &format!("{value:.1}")),
            Command::Slideshow { items, photo_seconds, looping, shuffle, fit, video_sound } => {
                if items.is_empty() {
                    Err("diaporama vide : aucun média dans le dossier".into())
                } else {
                    let _ = self.m().set_property("pause", "no");
                    self.show = Some(Show::new(items, photo_seconds, looping, shuffle, fit, video_sound));
                    self.show_current();
                    Ok(())
                }
            }
            Command::SlideshowSettings { photo_seconds, looping, shuffle, video_sound } => {
                if let Some(s) = &mut self.show {
                    s.update(photo_seconds, looping, shuffle, video_sound);
                    let video = s.current().map(slideshow::is_video).unwrap_or(false);
                    if video {
                        let _ = self.m().set_property("mute", if video_sound { "no" } else { "yes" });
                    }
                }
                Ok(())
            }
            Command::AudioDevice { name } => self.m().set_property("audio-device", name.as_deref().unwrap_or("auto")),
            Command::SetFit { fit } => {
                if let Some(s) = &mut self.show {
                    s.fit = fit;
                }
                self.apply_fit();
                Ok(())
            }
            Command::Next => {
                if self.show.as_mut().map(|s| s.advance()).unwrap_or(false) {
                    self.show_current();
                }
                Ok(())
            }
            Command::Prev => {
                if let Some(s) = &mut self.show {
                    s.back();
                    self.show_current();
                }
                Ok(())
            }
            Command::Goto { index } => {
                if let Some(s) = &mut self.show {
                    s.goto(index);
                    self.show_current();
                }
                Ok(())
            }
            Command::Pattern { .. } | Command::Warp { .. } | Command::Monitor { .. } | Command::Quit => Ok(()),
        };
        if let Err(e) = res {
            emit(&Event::Error { message: e });
        }
    }

    fn m(&self) -> &Mpv {
        self.mpv.as_ref().expect("mpv chargé")
    }

    fn do_load(&mut self, l: PendingLoad) -> Result<(), String> {
        self.current = Some(l.path.clone());
        let m = self.m();
        // Lecteur : la dernière image reste à l'écran en fin de média.
        let _ = m.set_property("keep-open", if l.hold { "yes" } else { "no" });
        let _ = m.set_property("loop-file", if l.looping { "inf" } else { "no" });
        let _ = m.set_property("panscan", "0");
        let _ = m.set_property("mute", "no");
        let _ = m.set_property("pause", if l.paused { "yes" } else { "no" });
        m.command(&["loadfile", &l.path, "replace"])
    }

    /// Niveau de l'image (1 = normal, 0 = noir), via le paramètre du shader.
    fn set_fade(&mut self, v: f64) {
        self.fade_level = v.clamp(0.0, 1.0);
        if self.fade_ok {
            if let Some(m) = &self.mpv {
                let _ = m.set_property("glsl-shader-opts", &format!("fade={:.4}", self.fade_level));
            }
        }
    }

    /// Avance le fondu en cours. Vrai tant qu'il faut continuer à se réveiller.
    fn step_fade(&mut self) -> bool {
        let Some(a) = &self.anim else { return false };
        let k = if a.secs <= 0.0 { 1.0 } else { (a.start.elapsed().as_secs_f64() / a.secs).min(1.0) };
        let v = a.from + (a.to - a.from) * k;
        self.set_fade(v);
        if k < 1.0 {
            return true;
        }
        self.anim = None;
        if let Some(l) = self.pending.take() {
            if let Err(e) = self.do_load(l) {
                emit(&Event::Error { message: e });
            }
        }
        false
    }

    /// Shader de sortie : mapping 4 coins, et réglage de niveau pour les fondus (vo gpu-next).
    fn apply_warp(&mut self) {
        let with_fade = !self.prefs.light;
        let corners = if with_fade { Some(self.warp_corners.unwrap_or(crate::prefs::Warp::IDENTITY)) } else { self.warp_corners };
        let Some(m) = &self.mpv else { return };
        self.fade_ok = false;
        let res = match corners.and_then(|c| warp::shader(c, with_fade)) {
            Some(src) => match self.shaders.write(&src) {
                Ok(p) => {
                    let r = m.set_property("glsl-shaders", &p.to_string_lossy());
                    self.fade_ok = with_fade && r.is_ok();
                    r
                }
                Err(e) => Err(format!("mapping 4 coins : {e}")),
            },
            None => m.set_property("glsl-shaders", ""),
        };
        if let Err(e) = res {
            emit(&Event::Error { message: e });
        }
        if self.fade_ok {
            let v = self.fade_level;
            self.set_fade(v);
        }
    }

    /// Intervalle du monitoring : celui des images de la vidéo (25 i/s sans vidéo, 60 au plus).
    fn monitor_interval(&self) -> Duration {
        let fps = if self.fps.0 > 1.0 { self.fps.0.min(60.0) } else { 25.0 };
        Duration::from_secs_f64(1.0 / fps)
    }

    /// Envoie une vignette de l'image en cours à la régie.
    fn capture(&mut self) {
        let Some(feed) = &mut self.feed else { return };
        match self.mpv.as_ref().and_then(|m| m.screenshot_rgba()) {
            Some((w, h, stride, px)) => feed.send(w, h, stride, &px),
            None => feed.send_black(),
        }
    }

    /// Sorties audio vues par mpv, pour le choix dans la régie.
    fn emit_audio_devices(&self) {
        let Some(m) = &self.mpv else { return };
        let list = m
            .get_property("audio-device-list")
            .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|d| Some((d.get("name")?.as_str()?.to_string(), d.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string())))
            .collect();
        emit(&Event::AudioDevices { list });
    }

    fn end_show(&mut self) {
        if self.show.take().is_some() {
            emit(&Event::Slide { index: 0, count: 0, path: String::new() });
        }
    }

    /// Envoie à mpv le média courant du diaporama.
    fn show_current(&mut self) {
        let (Some(m), Some(s)) = (&self.mpv, &mut self.show) else { return };
        let Some(path) = s.current().map(str::to_string) else { return };
        s.deadline = None;
        let video = slideshow::is_video(&path);
        // Diaporama : une vidéo finie passe la main au média suivant.
        let _ = m.set_property("keep-open", "no");
        let _ = m.set_property("loop-file", "no");
        let _ = m.set_property("mute", if video && !s.video_sound { "yes" } else { "no" });
        self.current = Some(path.clone());
        emit(&Event::Slide { index: s.pos, count: s.len(), path: path.clone() });
        if let Err(e) = m.command(&["loadfile", &path, "replace"]) {
            emit(&Event::Error { message: e });
        }
    }

    /// Ajustement de l'image courante (panscan 1 = remplir en recadrant).
    fn apply_fit(&self) {
        let (Some(m), Some(s)) = (&self.mpv, &self.show) else { return };
        let fill = match s.fit {
            Fit::Contain => false,
            Fit::Cover => true,
            Fit::Auto => {
                let img = m.get_property("video-params/aspect").and_then(|a| a.parse::<f64>().ok());
                let scr = self.window.as_ref().map(|w| w.inner_size()).map(|z| z.width as f64 / z.height.max(1) as f64);
                matches!((img, scr), (Some(i), Some(s)) if ((i - s) / s).abs() < 0.2)
            }
        };
        let _ = m.set_property("panscan", if fill { "1" } else { "0" });
    }

    /// Fin de photo ou de vidéo : média suivant, ou fin du diaporama.
    fn show_next(&mut self) {
        let more = self.show.as_mut().map(|s| s.advance()).unwrap_or(false);
        if more {
            self.show_current();
        } else if self.show.is_some() {
            self.end_show();
            emit(&Event::Ended);
        }
    }

    fn mpv_events(&mut self, el: &ActiveEventLoop) {
        // On sort mpv le temps de vider sa file : `property` a besoin de `&mut self`.
        let Some(m) = self.mpv.take() else { return };
        // Actions du diaporama, faites une fois mpv rendu.
        let mut next = false;
        let mut started = false;
        let mut refit = false;
        while let Some(ev) = m.next_event() {
            match ev.event_id {
                mpv::EVENT_SHUTDOWN => el.exit(),
                mpv::EVENT_FILE_LOADED => {
                    if !self.pattern_on {
                        emit(&Event::Loaded { path: self.current.clone().unwrap_or_default() });
                    }
                    started = true;
                }
                mpv::EVENT_VIDEO_RECONFIG => refit = true,
                mpv::EVENT_PLAYBACK_RESTART => {
                    // Première image du nouveau média : fondu d'ouverture.
                    if self.pending.is_none() {
                        if let Some(secs) = self.fade_in_after.take() {
                            self.anim = Some(FadeAnim { from: self.fade_level, to: 1.0, start: Instant::now(), secs });
                        }
                    }
                }
                mpv::EVENT_END_FILE => {
                    let end = unsafe { &*(ev.data as *const mpv::RawEndFile) };
                    if end.reason == mpv::END_FILE_ERROR {
                        emit(&Event::Error { message: format!("lecture impossible ({})", self.current.clone().unwrap_or_default()) });
                        if let Some(s) = &mut self.show {
                            s.failures += 1;
                            // Un média illisible est sauté, sauf si plus rien ne passe.
                            if s.failures < s.len().max(1) {
                                next = true;
                            }
                        }
                    }
                    if end.reason == mpv::END_FILE_EOF {
                        if self.show.is_some() {
                            next = true;
                        } else {
                            emit(&Event::Ended);
                        }
                    } else if self.show.is_none() {
                        emit(&Event::Idle);
                    }
                }
                mpv::EVENT_PROPERTY_CHANGE => {
                    let p = unsafe { &*(ev.data as *const mpv::RawProperty) };
                    self.property(ev.reply_userdata, p);
                }
                _ => {}
            }
        }
        let paused = m.get_property("pause").as_deref() == Some("yes");
        self.mpv = Some(m);
        if started {
            if let Some(s) = &mut self.show {
                s.started(paused);
            }
        }
        if refit {
            self.apply_fit();
        }
        if next {
            self.show_next();
        }
    }

    fn property(&mut self, id: u64, p: &mpv::RawProperty) {
        let double = || (p.format == mpv::FORMAT_DOUBLE && !p.data.is_null()).then(|| unsafe { *(p.data as *const f64) });
        let int = || (p.format == mpv::FORMAT_INT64 && !p.data.is_null()).then(|| unsafe { *(p.data as *const i64) });
        match id {
            P_TIME => {
                if let Some(t) = double() {
                    // 10 positions par seconde suffisent à la régie.
                    if self.last_pos.elapsed() >= Duration::from_millis(100) {
                        self.last_pos = Instant::now();
                        emit(&Event::Position { time: t, duration: self.duration });
                    }
                }
            }
            P_DURATION => self.duration = double().unwrap_or(0.0),
            P_VFPS | P_DFPS => {
                let v = double().unwrap_or(0.0);
                if id == P_VFPS {
                    self.fps.0 = v;
                } else {
                    self.fps.1 = v;
                }
                // Une mesure par seconde suffit (l'estimation bouge à chaque image).
                if id == P_DFPS || self.last_fps.elapsed() >= Duration::from_secs(1) {
                    self.last_fps = Instant::now();
                    emit(&Event::Fps { video: self.fps.0, display: self.fps.1 });
                }
            }
            P_PAUSE => {
                if p.format == mpv::FORMAT_FLAG && !p.data.is_null() {
                    emit(&Event::Paused { paused: unsafe { *(p.data as *const c_int) } != 0 });
                }
            }
            P_HWDEC => {
                if p.format == mpv::FORMAT_STRING && !p.data.is_null() {
                    let s = unsafe { *(p.data as *const *const c_char) };
                    if !s.is_null() {
                        emit(&Event::Hwdec { name: unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned() });
                    }
                }
            }
            P_DROPS | P_DELAYED => {
                if let Some(v) = int() {
                    if id == P_DROPS {
                        self.drops.0 = v;
                    } else {
                        self.drops.1 = v;
                    }
                    emit(&Event::Drops { dropped: self.drops.0, delayed: self.drops.1 });
                }
            }
            _ => {}
        }
    }
}

impl ApplicationHandler<UserEv> for OutputApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(e) = self.open_window(el) {
                emit(&Event::Error { message: e });
                el.exit();
            }
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEv) {
        match ev {
            UserEv::Cmd(c) => self.command(el, c),
            UserEv::Mpv => self.mpv_events(el),
            UserEv::Detached => el.exit(),
        }
    }

    /// Minutage des photos : on se réveille pile à l'échéance.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let fading = self.step_fade();
        let now = Instant::now();
        if let Some(d) = self.show.as_ref().and_then(|s| s.deadline) {
            if d <= now {
                if let Some(s) = &mut self.show {
                    s.deadline = None;
                }
                self.show_next();
            }
        }
        if let Some(every) = self.monitor_every {
            if self.monitor_next <= now {
                self.capture();
                // Même cadence que la vidéo affichée.
                let _ = every;
                let every = self.monitor_interval();
                self.monitor_every = Some(every);
                self.monitor_next = now + every;
            }
        }
        // Prochain réveil : fin de photo ou prochaine vignette.
        // Pendant un fondu, on se réveille à chaque image (~120 Hz) ; sinon fin de photo ou prochaine vignette.
        let fade_tick = (fading || self.anim.is_some()).then(|| now + Duration::from_millis(8));
        let next = [self.show.as_ref().and_then(|s| s.deadline), self.monitor_every.map(|_| self.monitor_next), fade_tick].into_iter().flatten().min();
        el.set_control_flow(match next {
            Some(d) => ControlFlow::WaitUntil(d),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested | WindowEvent::Resized(_) => self.paint_cpu(),
            _ => {}
        }
    }
}

pub fn run(prefs: MachinePrefs, monitor_port: Option<u16>) {
    let event_loop = match EventLoop::<UserEv>::with_user_event().build() {
        Ok(e) => e,
        Err(e) => {
            emit(&Event::Error { message: format!("boucle d'événements : {e}") });
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();

    // Lecture des commandes de la régie. Fin de flux = régie fermée : on s'arrête aussi.
    let cmd_proxy = event_loop.create_proxy();
    std::thread::Builder::new()
        .name("sortie-commandes".into())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if let Ok(c) = serde_json::from_str::<Command>(&line) {
                    if cmd_proxy.send_event(UserEv::Cmd(c)).is_err() {
                        return;
                    }
                }
            }
            let _ = cmd_proxy.send_event(UserEv::Detached);
        })
        .ok();

    let mut app = OutputApp {
        prefs,
        mpv: None,
        proxy: Box::new(proxy),
        window: None,
        surface: None,
        pattern_on: false,
        show: None,
        shaders: warp::ShaderFiles::new(),
        warp_corners: None,
        fade_ok: false,
        fade_level: 1.0,
        anim: None,
        pending: None,
        fade_in_after: None,
        monitor_port,
        feed: None,
        monitor_every: None,
        monitor_next: Instant::now(),
        duration: 0.0,
        last_pos: Instant::now(),
        drops: (0, 0),
        fps: (0.0, 0.0),
        last_fps: Instant::now(),
        current: None,
    };
    let _ = event_loop.run_app(&mut app);
}
