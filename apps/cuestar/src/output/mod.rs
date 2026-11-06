//! Moteur de sortie : processus séparé de la régie.
//!
//! Il ouvre la fenêtre de sortie (plein écran sur l'écran choisi) et y fait rendre libmpv
//! directement au GPU. Les commandes arrivent sur l'entrée standard, les événements repartent
//! sur la sortie standard (voir `protocol`). Une régie qui rame ne retarde aucune image.

mod mpv;
mod pattern;

use crate::prefs::MachinePrefs;
use crate::protocol::{Command, Event, Monitor};
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

struct OutputApp {
    prefs: MachinePrefs,
    // Ordre des champs = ordre de destruction : mpv avant le proxy qu'il référence.
    mpv: Option<Mpv>,
    proxy: Box<EventLoopProxy<UserEv>>,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    pattern_on: bool,
    duration: f64,
    last_pos: Instant,
    drops: (i64, i64),
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
        m.set_wakeup(mpv_wakeup, &*self.proxy as *const EventLoopProxy<UserEv> as *mut c_void);
        let version = m.get_property("mpv-version").unwrap_or_else(|| format!("mpv (API {})", m.api_version()));
        self.mpv = Some(m);
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
        if let Command::Pattern { on } = cmd {
            self.pattern_on = on;
            if let Some(m) = &self.mpv {
                let res = if on {
                    let (w, h) = self.window.as_ref().map(|w| (w.inner_size().width, w.inner_size().height)).unwrap_or((1920, 1080));
                    m.command(&["loadfile", &format!("av://lavfi:smptehdbars=size={w}x{h}"), "replace"])
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
        let Some(m) = &self.mpv else {
            emit(&Event::Error { message: "lecture impossible : libmpv n'est pas chargé".into() });
            return;
        };
        let res = match cmd {
            Command::Load { path, paused } => {
                let _ = m.set_property("pause", if paused { "yes" } else { "no" });
                self.current = Some(path.clone());
                m.command(&["loadfile", &path, "replace"])
            }
            Command::Play => m.set_property("pause", "no"),
            Command::Pause => m.set_property("pause", "yes"),
            Command::Stop => m.command(&["stop"]),
            Command::Seek { seconds } => m.command(&["seek", &format!("{seconds:.3}"), "absolute+exact"]),
            Command::Volume { value } => m.set_property("volume", &format!("{value:.1}")),
            Command::Pattern { .. } | Command::Quit => Ok(()),
        };
        if let Err(e) = res {
            emit(&Event::Error { message: e });
        }
    }

    fn mpv_events(&mut self, el: &ActiveEventLoop) {
        // On sort mpv le temps de vider sa file : `property` a besoin de `&mut self`.
        let Some(m) = self.mpv.take() else { return };
        while let Some(ev) = m.next_event() {
            match ev.event_id {
                mpv::EVENT_SHUTDOWN => el.exit(),
                mpv::EVENT_FILE_LOADED => {
                    if !self.pattern_on {
                        emit(&Event::Loaded { path: self.current.clone().unwrap_or_default() });
                    }
                }
                mpv::EVENT_END_FILE => {
                    let end = unsafe { &*(ev.data as *const mpv::RawEndFile) };
                    if end.reason == mpv::END_FILE_ERROR {
                        emit(&Event::Error { message: format!("lecture impossible ({})", self.current.clone().unwrap_or_default()) });
                    }
                    if end.reason == mpv::END_FILE_EOF {
                        emit(&Event::Ended);
                    } else {
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
        self.mpv = Some(m);
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

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested | WindowEvent::Resized(_) => self.paint_cpu(),
            _ => {}
        }
    }
}

pub fn run(prefs: MachinePrefs) {
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
        duration: 0.0,
        last_pos: Instant::now(),
        drops: (0, 0),
        current: None,
    };
    let _ = event_loop.run_app(&mut app);
}
