//! Moteur de sortie : processus séparé de la régie.
//!
//! Il ouvre la fenêtre de sortie (plein écran sur l'écran choisi) et y affiche l'image au GPU.
//! Les commandes arrivent sur l'entrée standard, les événements repartent sur la sortie
//! standard (voir `protocol`). Une régie qui rame ne retarde aucune image.
//!
//! Affichage normal : CueStar compose lui-même l'image (`screen`) à partir de deux lecteurs
//! mpv, ce qui permet le fondu enchaîné, le fondu au noir, le mapping et un monitoring sans
//! perte. Si l'API de rendu manque, affichage de secours (`embed`) : mpv seul dans la fenêtre.

mod audio_filter;
mod embed;
mod monitor;
mod on_top;
pub mod mpv;
mod pattern;
mod refresh;
mod screen;
#[cfg(windows)]
mod screen_d3d11;
#[cfg(not(windows))]
mod screen_gl;
mod slideshow;
pub mod warp;

use crate::prefs::{Cadence, MachinePrefs};
use crate::protocol::{Clip, Command, Event, Fit, Monitor, TransitionKind};
use mpv::Mpv;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slideshow::Show;
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
    /// Le lecteur n° … a des événements à lire.
    Mpv(usize),
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

/// Réveil d'un lecteur : mpv nous prévient depuis un de ses fils.
struct Wake {
    proxy: EventLoopProxy<UserEv>,
    deck: usize,
}

unsafe extern "C" fn mpv_wakeup(data: *mut c_void) {
    let w = unsafe { &*(data as *const Wake) };
    let _ = w.proxy.send_event(UserEv::Mpv(w.deck));
}

/// Silence sans fin : garde la sortie son d'un lecteur en attente ouverte.
const SILENCE: &str = "av://lavfi:anullsrc=r=48000:cl=stereo";

// Identifiants des propriétés observées.
const P_TIME: u64 = 1;
const P_DURATION: u64 = 2;
const P_PAUSE: u64 = 3;
const P_HWDEC: u64 = 4;
const P_DROPS: u64 = 5;
const P_DELAYED: u64 = 6;
const P_VFPS: u64 = 7;
const P_DFPS: u64 = 8;
const P_EOF: u64 = 9;

/// Affichage de la sortie.
enum Out {
    /// CueStar compose l'image (deux lecteurs).
    Composite(screen::Screen),
    /// Secours : mpv seul dans la fenêtre (un lecteur).
    Embedded(embed::Embedded),
}

impl Out {
    fn deck(&self, i: usize) -> &Mpv {
        match self {
            Out::Composite(s) => s.deck(i.min(s.deck_count() - 1)),
            Out::Embedded(e) => e.deck(),
        }
    }
    fn decks(&self) -> usize {
        match self {
            Out::Composite(s) => s.deck_count(),
            Out::Embedded(_) => 1,
        }
    }
    fn can_fade(&self) -> bool {
        match self {
            Out::Composite(_) => true,
            Out::Embedded(e) => e.fade_ok(),
        }
    }
    fn set_level(&mut self, v: f64) {
        match self {
            Out::Composite(s) => s.set_level(v),
            Out::Embedded(e) => e.set_level(v),
        }
    }
    fn set_mix(&self, v: f64) {
        if let Out::Composite(s) = self {
            s.set_mix(v);
        }
    }
    fn show(&self, deck: usize, on: bool) {
        if let Out::Composite(s) = self {
            s.show(deck, on);
        }
    }
    fn reveal(&self, deck: usize) {
        if let Out::Composite(s) = self {
            s.reveal(deck);
        }
    }
}

struct FadeAnim {
    from: f64,
    to: f64,
    start: Instant,
    secs: f64,
    /// Le son suit l'image (fondus d'un élément édité ; la transition « Noir » ne touche qu'à l'image).
    audio: bool,
}

/// Fondu enchaîné en cours : de `from` vers `to` (lecteurs).
struct Xfade {
    from: usize,
    to: usize,
    start: Instant,
    secs: f64,
}

struct PendingLoad {
    path: String,
    paused: bool,
    looping: bool,
    hold: bool,
    clip: Clip,
    signal_end: bool,
}

/// Média demandé par `Prepare`.
struct PrepareReq {
    path: String,
    looping: bool,
    clip: Clip,
}

/// Média préparé sur le lecteur en attente : ouvert, arrêté sur sa première image, masqué.
struct Prepared {
    deck: usize,
    req: PrepareReq,
    filters: Vec<audio_filter::Stage>,
    /// Première image prête (PLAYBACK_RESTART reçu).
    ready: bool,
}

/// Points, boucle, durée d'image et filtres son d'un élément, posés sur le lecteur juste
/// avant `loadfile`. Sans réglage (`None`), le lecteur revient à la lecture brute.
/// Renvoie les filtres son posés (pour les changer ensuite à chaud).
fn apply_clip(m: &Mpv, clip: Option<&Clip>, looping: bool) -> Vec<audio_filter::Stage> {
    let secs = |v: f64| format!("{:.3}", v.max(0.0));
    let none = "none".to_string();
    let (start, end, ab, image, stages) = match clip {
        None => (none.clone(), none.clone(), None, "inf".to_string(), Vec::new()),
        Some(c) => {
            let s = &c.sound;
            let start = if s.start > 0.0 { secs(s.start) } else { none.clone() };
            let end_opt = s.end.filter(|e| *e > s.start);
            // En boucle avec des traits de boucle : mpv saute de la fin de boucle à son début.
            let ab = (looping && (s.loop_start.is_some() || s.loop_end.is_some()))
                .then(|| (s.loop_start.unwrap_or(s.start), s.loop_end.or(end_opt)))
                .and_then(|(a, b)| b.filter(|b| *b > a).map(|b| (a, b)));
            // La fin coupe le média ; en boucle, c'est la fin de boucle qui compte.
            let end = if looping { none.clone() } else { end_opt.map(secs).unwrap_or_else(|| none.clone()) };
            let image = c.image_secs.map(|d| format!("{d:.3}")).unwrap_or_else(|| "inf".into());
            (start, end, ab, image, audio_filter::stages(s))
        }
    };
    let _ = m.set_property("start", &start);
    let _ = m.set_property("end", &end);
    match ab {
        Some((a, b)) => {
            let _ = m.set_property("loop-file", "no");
            let _ = m.set_property("ab-loop-a", &secs(a));
            let _ = m.set_property("ab-loop-b", &secs(b));
            let _ = m.set_property("ab-loop-count", "inf");
        }
        None => {
            let _ = m.set_property("ab-loop-a", "no");
            let _ = m.set_property("ab-loop-b", "no");
        }
    }
    let _ = m.set_property("image-display-duration", &image);
    let _ = m.set_property("af", &audio_filter::af_value(&stages));
    stages
}

struct OutputApp {
    prefs: MachinePrefs,
    // Ordre des champs = ordre de destruction : l'affichage (et ses mpv) avant les réveils
    // qu'ils référencent, et avant la fenêtre.
    out: Option<Out>,
    wakes: Vec<Box<Wake>>,
    proxy: EventLoopProxy<UserEv>,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    pattern_on: bool,
    show: Option<Show>,
    warp_corners: Option<[[f32; 2]; 4]>,
    /// Lecteur à l'antenne.
    active: usize,
    /// Lecteur qui sort pendant un fondu enchaîné (ses événements sont ignorés).
    outgoing: Option<usize>,
    /// Fondu enchaîné à lancer dès la première image du lecteur actif.
    xfade_after: Option<f64>,
    xfade: Option<Xfade>,
    /// Lecteurs en cours de préchauffage (événements ignorés).
    warming: Vec<u8>,
    /// Fins de média à taire (préchauffage interrompu par un vrai chargement).
    skip_end: Vec<bool>,
    volume: f64,
    fade_level: f64,
    anim: Option<FadeAnim>,
    /// Média à charger une fois le fondu sortant terminé.
    pending: Option<PendingLoad>,
    /// Durée du fondu d'ouverture à lancer dès la première image du média chargé.
    fade_in_after: Option<f64>,
    /// Le fondu d'ouverture à venir fait aussi monter le son (élément édité).
    fade_in_audio: bool,
    /// Réglages d'édition du média de chaque lecteur, et les filtres son posés.
    clips: [Clip; 2],
    filters: [Vec<audio_filter::Stage>; 2],
    /// Fondu de sortie de l'élément déjà lancé (une fois par lecture).
    out_fade_started: bool,
    /// Image fixe en cours : début de son fondu de sortie.
    image_fade_at: Option<Instant>,
    monitor_port: Option<u16>,
    feed: Option<monitor::MonitorFeed>,
    /// Monitoring par capture (affichage de secours seulement).
    monitor_every: Option<Duration>,
    monitor_next: Instant,
    duration: f64,
    last_pos: Instant,
    drops: (i64, i64),
    fps: (f64, f64),
    last_fps: Instant,
    current: Option<String>,
    /// Écran de sortie : nom système, fréquence actuelle, fréquences possibles.
    device: Option<String>,
    refresh_hz: f64,
    modes: Vec<f64>,
    /// Fréquence changée par CueStar (à rétablir en quittant).
    refresh_changed: bool,
    /// Média prêt sur le lecteur en attente, et demande à servir dès qu'il se libère.
    prepared: Option<Prepared>,
    prepare_after: Option<PrepareReq>,
    /// Le média à l'antenne signale sa fin même s'il garde sa dernière image (et l'a fait).
    signal_end: bool,
    end_signaled: bool,
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

/// Dossier du cache des shaders compilés (premier chargement plus rapide).
fn shader_cache() -> Option<String> {
    let d = directories::ProjectDirs::from("com", "acetayls", "CueStar")?.cache_dir().join("shaders");
    std::fs::create_dir_all(&d).ok()?;
    Some(d.to_string_lossy().into_owned())
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
        // Premier plan imposé seulement s'il reste un écran pour la régie.
        let on_top = fullscreen && !single;
        if on_top {
            attrs = on_top::attributes(attrs);
        }
        if fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(chosen.clone())));
        } else {
            attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(960.0, 540.0));
        }
        let window = Arc::new(el.create_window(attrs).map_err(|e| format!("fenêtre de sortie : {e}"))?);
        if on_top {
            on_top::install(&window);
        } else if fullscreen {
            window.set_cursor_visible(false);
        }
        self.window = Some(window.clone());
        self.refresh_hz = chosen.as_ref().and_then(|m| m.refresh_rate_millihertz()).map(|r| r as f64 / 1000.0).unwrap_or(60.0);
        self.device = chosen.as_ref().and_then(refresh::device_id);
        self.modes = self.device.as_deref().map(refresh::modes).unwrap_or_default();
        self.force_refresh();

        let mpv_version = match self.start_output(&window, chosen.as_ref()) {
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

    /// Crée un lecteur mpv avec les réglages communs (pas encore initialisé).
    fn new_deck(&self, composite: bool) -> Result<Mpv, String> {
        let m = Mpv::create()?;
        let p = &self.prefs;
        let hwdec = if composite {
            // Décodeurs qui livrent directement au moteur graphique, sans copie.
            match p.decode {
                crate::prefs::Decode::Auto => "auto-safe",
                crate::prefs::Decode::Cpu => "no",
                crate::prefs::Decode::Gpu => {
                    if cfg!(windows) {
                        "d3d11va"
                    } else if cfg!(target_os = "macos") {
                        "videotoolbox"
                    } else {
                        "vaapi,nvdec"
                    }
                }
            }
        } else {
            p.decode.mpv_hwdec()
        };
        m.set_option("hwdec", hwdec)?;
        if let Some(dev) = &p.audio_device {
            let _ = m.set_option("audio-device", dev);
        }
        let _ = m.set_option("volume", &format!("{:.1}", p.volume.clamp(0.0, 100.0)));
        let _ = m.set_option("audio-delay", &format!("{:.4}", p.audio_delay_ms / 1000.0));
        // Réglages « régie » : pas d'interface mpv, pas de clavier.
        for (k, v) in [
            ("terminal", "no"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("osc", "no"),
            ("osd-level", "0"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("input-cursor", "no"),
            ("image-display-duration", "inf"),
            ("audio-display", "no"),
            ("hr-seek", "yes"),
            ("stop-screensaver", "yes"),
            ("background-color", "#000000"),
            // Les vidéos de téléphone (H.264 « Baseline ») passent aussi par le GPU.
            ("vd-lavc-check-hw-profile", "no"),
            ("video-sync", "audio"),
            // Format de sortie son fixe : le son ouvert sert à tous les médias, sans réouverture.
            ("audio-samplerate", "48000"),
            ("audio-channels", "stereo"),
        ] {
            let _ = m.set_option(k, v);
        }
        // CUESTAR_MPV_LOG=<dossier> : journal détaillé de chaque lecteur (diagnostic).
        if let Some(dir) = std::env::var_os("CUESTAR_MPV_LOG") {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let _ = m.set_option("log-file", &std::path::Path::new(&dir).join(format!("mpv-{n}.log")).to_string_lossy());
        }
        // CUESTAR_MPV_OPTS="nom=valeur,…" : options mpv en plus, pour les essais au banc (tools/bench-sortie).
        if let Ok(opts) = std::env::var("CUESTAR_MPV_OPTS") {
            for kv in opts.split(',').filter(|s| !s.is_empty()) {
                if let Some((k, v)) = kv.split_once('=') {
                    let _ = m.set_option(k.trim(), v.trim());
                }
            }
        }
        if let Some(dir) = shader_cache() {
            let _ = m.set_option("gpu-shader-cache-dir", &dir);
        }
        if p.light {
            let _ = m.set_option("profile", "fast");
        }
        if composite {
            m.set_option("vo", "libmpv")?;
            // mpv ne voit pas l'écran : on lui donne sa fréquence (calage des images).
            let _ = m.set_option("display-fps-override", &format!("{:.3}", self.refresh_hz));
        }
        Ok(m)
    }

    /// Branche les observations et le réveil d'un lecteur initialisé.
    fn watch(&mut self, m: &Mpv, deck: usize) {
        let base = deck as u64 * 100;
        m.observe(base + P_TIME, "time-pos", mpv::FORMAT_DOUBLE);
        m.observe(base + P_DURATION, "duration", mpv::FORMAT_DOUBLE);
        m.observe(base + P_PAUSE, "pause", mpv::FORMAT_FLAG);
        m.observe(base + P_HWDEC, "hwdec-current", mpv::FORMAT_STRING);
        m.observe(base + P_DROPS, "frame-drop-count", mpv::FORMAT_INT64);
        m.observe(base + P_DELAYED, "vo-delayed-frame-count", mpv::FORMAT_INT64);
        m.observe(base + P_VFPS, "estimated-vf-fps", mpv::FORMAT_DOUBLE);
        m.observe(base + P_DFPS, "display-fps", mpv::FORMAT_DOUBLE);
        m.observe(base + P_EOF, "eof-reached", mpv::FORMAT_FLAG);
        let w = Box::new(Wake { proxy: self.proxy.clone(), deck });
        m.set_wakeup(mpv_wakeup, &*w as *const Wake as *mut c_void);
        self.wakes.push(w);
    }

    fn start_output(&mut self, window: &Arc<Window>, chosen: Option<&MonitorHandle>) -> Result<String, String> {
        let size = window.inner_size();
        let size = (size.width.max(1), size.height.max(1));
        // 1. Affichage composé par CueStar.
        let composite = (|| -> Result<screen::Screen, String> {
            let mut decks = Vec::new();
            for _ in 0..screen::DECKS {
                let m = self.new_deck(true)?;
                m.initialize()?;
                decks.push(m);
            }
            #[cfg(windows)]
            {
                let hwnd = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
                    RawWindowHandle::Win32(h) => h.hwnd.get(),
                    other => return Err(format!("fenêtre non gérée : {other:?}")),
                };
                use winit::platform::windows::MonitorHandleExtWindows;
                let hmon = chosen.map(|m| m.hmonitor()).unwrap_or(0);
                screen_d3d11::open(hwnd, hmon, size, self.refresh_hz, decks)
            }
            #[cfg(not(windows))]
            {
                let _ = chosen;
                screen_gl::open(window, size, self.refresh_hz, decks)
            }
        })();
        let out = match composite {
            Ok(s) => Out::Composite(s),
            Err(e) => {
                emit(&Event::Error { message: format!("affichage composé indisponible ({e}) : affichage de secours") });
                // 2. Secours : mpv dessine seul dans la fenêtre.
                let m = self.new_deck(false)?;
                let wid: i64 = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
                    RawWindowHandle::Win32(h) => h.hwnd.get() as i64,
                    RawWindowHandle::Xlib(h) => h.window as i64,
                    RawWindowHandle::Xcb(h) => h.window.get() as i64,
                    RawWindowHandle::AppKit(h) => h.ns_view.as_ptr() as i64,
                    other => return Err(format!("type de fenêtre non géré pour l'incrustation mpv : {other:?}")),
                };
                m.set_option("wid", &wid.to_string())?;
                m.set_option("vo", if self.prefs.light { "gpu" } else { "gpu-next" })?;
                if let Some(api) = self.prefs.render.mpv_gpu_api() {
                    m.set_option("gpu-api", api)?;
                }
                let _ = m.set_option("force-window", "yes");
                let _ = m.set_option("cursor-autohide", "always");
                m.initialize()?;
                Out::Embedded(embed::Embedded::new(m, self.prefs.light))
            }
        };
        let version = {
            let m = out.deck(0);
            m.get_property("mpv-version").unwrap_or_else(|| format!("mpv (API {})", m.api_version()))
        };
        let n = out.decks();
        // Les réveils pointent vers des boîtes gardées dans `self.wakes`.
        let raw: Vec<*const Mpv> = (0..n).map(|i| out.deck(i) as *const Mpv).collect();
        self.out = Some(out);
        for (i, m) in raw.into_iter().enumerate() {
            self.watch(unsafe { &*m }, i);
        }
        self.warming = vec![0; n];
        self.skip_end = vec![false; n];
        self.warp_corners = self.prefs.warp.active();
        self.apply_warp();
        self.prewarm();
        Ok(version)
    }

    /// Préchauffage : ouvre le son et prépare le rendu de chaque lecteur avant le premier GO.
    /// Le rendu est préparé par une image noire, puis le lecteur se met en attente.
    fn prewarm(&mut self) {
        let Some(out) = &self.out else { return };
        for i in 0..out.decks() {
            let m = out.deck(i);
            let _ = m.set_property("keep-open", "no");
            let _ = m.set_property("loop-file", "no");
            apply_clip(m, None, false);
            let ok = m.command(&["loadfile", "av://lavfi:color=c=black:s=64x36:r=25:d=0.1", "replace"]).is_ok()
                && m.command(&["loadfile", SILENCE, "append-play"]).is_ok();
            // En attente tant qu'un vrai média ne remplace pas le silence.
            self.warming[i] = if ok { 100 } else { 0 };
        }
    }

    /// Lecteur en attente : il joue du silence pour garder la sortie son ouverte. Ouvrir le son
    /// prend jusqu'à 1,6 s sous Windows ; un son déjà ouvert repart en quelques ms.
    fn park(&mut self, deck: usize) {
        if self.prepared.as_ref().is_some_and(|p| p.deck == deck) {
            self.prepared = None;
        }
        let Some(out) = &self.out else { return };
        let m = out.deck(deck);
        let _ = m.set_property("keep-open", "no");
        let _ = m.set_property("loop-file", "no");
        let _ = m.set_property("pause", "no");
        let _ = m.set_property("volume", &format!("{:.1}", self.volume));
        apply_clip(m, None, false);
        out.show(deck, false);
        // En attente : ses événements (fin du média remplacé, silence) ne remontent pas à la régie.
        if m.command(&["loadfile", SILENCE, "replace"]).is_ok() {
            self.warming[deck] = 100;
        }
    }

    /// Charge un média tout de suite : sur le lecteur en attente quand il y en a deux (son déjà
    /// ouvert, image précédente gardée jusqu'à la première image du nouveau média).
    fn load_now(&mut self, l: PendingLoad) -> Result<(), String> {
        if self.out().decks() > 1 {
            self.switch_load(l, 0.0)
        } else {
            self.do_load(l, true)
        }
    }

    /// Charge sur l'autre lecteur ; à sa première image, passage en `secs` secondes (0 : coupe).
    fn switch_load(&mut self, l: PendingLoad, secs: f64) -> Result<(), String> {
        self.finish_xfade();
        let from = self.active;
        let to = (from + 1) % self.out().decks();
        self.outgoing = Some(from);
        self.active = to;
        self.xfade_after = Some(secs);
        if secs > 0.0 {
            let _ = self.out().deck(to).set_property("volume", "0");
        }
        self.do_load(l, secs <= 0.0)
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
            self.set_monitor(on);
            return;
        }
        if let Command::Warp { corners } = cmd {
            self.warp_corners = corners;
            self.apply_warp();
            return;
        }
        if let Command::Pattern { kind } = cmd {
            self.end_show();
            self.cancel_transitions();
            self.pattern_on = kind.is_some();
            if self.out.is_some() {
                let (w, h) = self.window.as_ref().map(|w| (w.inner_size().width, w.inner_size().height)).unwrap_or((1920, 1080));
                let a = self.active;
                self.stop_warming(a);
                let m = self.m();
                let res = if let Some(k) = kind {
                    let _ = m.set_property("panscan", "0");
                    let _ = m.set_property("loop-file", "no");
                    let _ = m.set_property("keep-open", "yes");
                    let _ = m.set_property("pause", "no");
                    apply_clip(m, None, false);
                    let r = m.command(&["loadfile", &k.lavfi(w, h), "replace"]);
                    self.out().reveal(a);
                    r
                } else {
                    let r = m.command(&["stop"]);
                    self.out().show(a, false);
                    r
                };
                if let Err(e) = res {
                    emit(&Event::Error { message: e });
                }
            } else if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }
        if self.out.is_none() {
            emit(&Event::Error { message: "lecture impossible : libmpv n'est pas chargé".into() });
            return;
        }
        let res = match cmd {
            Command::Prepare { path, looping, clip } => {
                self.prepare(PrepareReq { path, looping, clip });
                Ok(())
            }
            Command::Load { path, paused, looping, hold, transition, clip, signal_end } => {
                self.end_show();
                self.pattern_on = false;
                // Un élément avec un fondu d'entrée part du noir, quelle que soit la transition.
                let item_in = (clip.sound.fade_in > 0.0).then_some(clip.sound.fade_in).filter(|_| self.out().can_fade());
                let load = PendingLoad { path, paused, looping, hold, clip, signal_end };
                let on_air = self.current.is_some();
                let cross = item_in.is_none() && transition.kind == TransitionKind::Crossfade && transition.secs > 0.0 && self.out().decks() > 1 && on_air;
                let black = (item_in.is_some() || transition.kind != TransitionKind::Cut && transition.secs > 0.0) && self.out().can_fade() && !cross;
                self.fade_in_audio = item_in.is_some();
                // Média déjà préparé : coupe ou fondu enchaîné sans attendre l'ouverture du fichier.
                let quick = !black && {
                    self.anim = None;
                    self.pending = None;
                    self.fade_in_after = None;
                    self.set_fade(1.0);
                    self.go_prepared(&load, cross.then_some(transition.secs))
                };
                if quick {
                    Ok(())
                } else if cross {
                    // Fondu enchaîné : le média part sur l'autre lecteur, le mélange commence à sa première image.
                    self.anim = None;
                    self.pending = None;
                    self.fade_in_after = None;
                    self.set_fade(1.0);
                    self.switch_load(load, transition.secs)
                } else if !black {
                    self.cancel_transitions();
                    self.set_fade(1.0);
                    self.load_now(load)
                } else {
                    self.cancel_transitions();
                    self.fade_in_audio = item_in.is_some();
                    let half = if transition.kind != TransitionKind::Cut { transition.secs / 2.0 } else { 0.0 };
                    let fade_in = item_in.unwrap_or(half);
                    if on_air && self.fade_level > 0.0 && half > 0.0 {
                        // Fondu sortant, puis chargement, puis fondu d'ouverture.
                        self.anim = Some(FadeAnim { from: self.fade_level, to: 0.0, start: Instant::now(), secs: half, audio: false });
                        self.pending = Some(load);
                        self.fade_in_after = Some(fade_in);
                        Ok(())
                    } else {
                        self.set_fade(0.0);
                        self.fade_in_after = Some(fade_in);
                        self.load_now(load)
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
                self.prepare_after = None;
                self.end_show();
                self.cancel_transitions();
                self.set_fade(1.0);
                self.current = None;
                let n = self.out().decks();
                if matches!(self.out(), Out::Composite(_)) {
                    for i in 0..n {
                        self.park(i);
                    }
                    emit(&Event::Idle);
                    Ok(())
                } else {
                    self.stop_warming(0);
                    self.m().command(&["stop"])
                }
            }
            Command::Seek { seconds } => {
                // Retour avant le fondu de sortie : l'image et le son reviennent.
                if self.out_fade_started {
                    self.out_fade_started = false;
                    if self.xfade.is_none() {
                        self.anim = None;
                        self.set_fade(1.0);
                        let _ = self.m().set_property("volume", &format!("{:.1}", self.volume));
                    }
                }
                self.m().command(&["seek", &format!("{seconds:.3}"), "absolute+exact"])
            }
            Command::SetClip { clip } => {
                // Volume et EQ à chaud, sans couper le son quand les filtres restent les mêmes.
                let a = self.active.min(1);
                let stages = audio_filter::stages(&clip.sound);
                let m = self.m();
                let live = audio_filter::live_commands(&self.filters[a], &stages);
                let r = match live {
                    Some(cmds) => cmds.iter().try_for_each(|[l, k, v, t]| m.command(&["af-command", l, k, v, t])),
                    None => Err(String::new()),
                };
                let res = if r.is_err() { m.set_property("af", &audio_filter::af_value(&stages)) } else { Ok(()) };
                self.filters[a] = stages;
                self.clips[a] = clip;
                res
            }
            Command::Volume { value } => {
                self.volume = value;
                if self.xfade.is_some() || self.xfade_after.is_some() {
                    // Le fondu enchaîné reprend la main sur les volumes.
                    Ok(())
                } else {
                    self.m().set_property("volume", &format!("{value:.1}"))
                }
            }
            Command::Slideshow { items, photo_seconds, looping, shuffle, fit, video_sound } => {
                if items.is_empty() {
                    Err("diaporama vide : aucun média dans le dossier".into())
                } else {
                    self.cancel_transitions();
                    self.set_fade(1.0);
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
            Command::AudioDevice { name } => {
                // Les deux lecteurs changent de sortie son (le fondu enchaîné passe de l'un à l'autre).
                let dev = name.as_deref().unwrap_or("auto").to_string();
                let out = self.out();
                (0..out.decks()).map(|i| out.deck(i).set_property("audio-device", &dev)).collect::<Result<Vec<_>, _>>().map(|_| ())
            }
            Command::AudioDelay { ms } => {
                let secs = format!("{:.4}", ms / 1000.0);
                let out = self.out();
                (0..out.decks()).map(|i| out.deck(i).set_property("audio-delay", &secs)).collect::<Result<Vec<_>, _>>().map(|_| ())
            }
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

    fn out(&self) -> &Out {
        self.out.as_ref().expect("sortie ouverte")
    }

    /// Un vrai média remplace le préchauffage : ses dernières fins de média sont tues.
    fn stop_warming(&mut self, deck: usize) {
        if self.warming.get(deck).copied().unwrap_or(0) > 0 {
            self.warming[deck] = 0;
            self.skip_end[deck] = true;
            if let Some(out) = &self.out {
                let _ = out.deck(deck).set_property("mute", "no");
            }
        }
    }

    /// Lecteur à l'antenne.
    fn m(&self) -> &Mpv {
        self.out().deck(self.active)
    }

    /// Charge un média sur le lecteur actif. `set_volume` : volume normal tout de suite
    /// (faux pendant un fondu enchaîné, qui monte le son lui-même).
    fn do_load(&mut self, l: PendingLoad, set_volume: bool) -> Result<(), String> {
        self.current = Some(l.path.clone());
        let a = self.active;
        if self.prepared.as_ref().is_some_and(|p| p.deck == a) {
            self.prepared = None;
        }
        self.signal_end = l.hold && l.signal_end;
        self.end_signaled = false;
        self.stop_warming(a);
        let vol = format!("{:.1}", self.volume);
        let m = self.m();
        // Lecteur : la dernière image reste à l'écran en fin de média.
        let _ = m.set_property("keep-open", if l.hold { "yes" } else { "no" });
        let _ = m.set_property("loop-file", if l.looping { "inf" } else { "no" });
        let _ = m.set_property("panscan", "0");
        let _ = m.set_property("mute", "no");
        if set_volume {
            let _ = m.set_property("volume", &vol);
        }
        let _ = m.set_property("pause", if l.paused { "yes" } else { "no" });
        let filters = apply_clip(m, Some(&l.clip), l.looping);
        // Fondu d'entrée de l'élément : le son part du silence avec l'image.
        if self.fade_in_audio {
            let _ = m.set_property("volume", "0");
        }
        let r = m.command(&["loadfile", &l.path, "replace"]);
        let a = a.min(1);
        self.clips[a] = l.clip;
        self.filters[a] = filters;
        self.out_fade_started = false;
        self.image_fade_at = None;
        // L'image précédente reste jusqu'à la première image du nouveau média (pas de noir).
        self.out().reveal(a);
        r
    }

    /// Arrête net les fondus en cours (le lecteur actif reste seul à l'antenne).
    fn cancel_transitions(&mut self) {
        self.anim = None;
        self.pending = None;
        self.fade_in_after = None;
        self.xfade_after = None;
        if let Some(x) = self.xfade.take() {
            self.end_outgoing(x.from);
        } else if let Some(o) = self.outgoing.take() {
            self.end_outgoing(o);
        }
        self.outgoing = None;
        let a = self.active as f64;
        if let Some(out) = &self.out {
            out.set_mix(a);
            let _ = out.deck(self.active).set_property("volume", &format!("{:.1}", self.volume));
        }
    }

    /// Termine tout de suite un fondu enchaîné en cours (avant d'en lancer un autre).
    fn finish_xfade(&mut self) {
        if self.xfade.is_some() || self.outgoing.is_some() {
            self.cancel_transitions();
        }
    }

    /// Le lecteur sorti s'arrête et redevient disponible.
    fn end_outgoing(&mut self, deck: usize) {
        if deck != self.active {
            self.park(deck);
            // Le lecteur est libre : la préparation demandée pendant la transition peut partir.
            if let Some(r) = self.prepare_after.take() {
                self.prepare(r);
            }
        }
    }

    /// Ouvre un média sur le lecteur en attente, en pause sur sa première image, masqué.
    fn prepare(&mut self, r: PrepareReq) {
        if !matches!(self.out, Some(Out::Composite(_))) || self.pattern_on {
            return;
        }
        // Lecteur en attente encore occupé (transition, diaporama) : la préparation attendra.
        if self.outgoing.is_some() || self.xfade.is_some() || self.xfade_after.is_some() || self.pending.is_some() || self.show.is_some() {
            self.prepare_after = Some(r);
            return;
        }
        let deck = (self.active + 1) % self.out().decks();
        if self.prepared.as_ref().is_some_and(|p| p.deck == deck && p.req.path == r.path && p.req.looping == r.looping && p.req.clip == r.clip) {
            return;
        }
        self.stop_warming(deck);
        let out = self.out();
        let m = out.deck(deck);
        out.show(deck, false);
        let _ = m.set_property("pause", "yes");
        let _ = m.set_property("keep-open", "yes");
        let _ = m.set_property("loop-file", if r.looping { "inf" } else { "no" });
        let _ = m.set_property("panscan", "0");
        let _ = m.set_property("mute", "no");
        let filters = apply_clip(m, Some(&r.clip), r.looping);
        match m.command(&["loadfile", &r.path, "replace"]) {
            Ok(()) => self.prepared = Some(Prepared { deck, req: r, filters, ready: false }),
            Err(e) => {
                self.prepared = None;
                emit(&Event::Error { message: e });
            }
        }
    }

    /// GO sur un média préparé : il passe à l'antenne tout de suite (coupe), ou par un fondu
    /// enchaîné de `xfade` secondes. Faux si le média préparé ne correspond pas (chargement normal).
    fn go_prepared(&mut self, l: &PendingLoad, xfade: Option<f64>) -> bool {
        let fits = self.prepared.as_ref().is_some_and(|p| p.ready && p.deck != self.active && p.req.path == l.path && p.req.looping == l.looping && p.req.clip == l.clip);
        // Une transition encore en cours occupe les deux lecteurs : chargement normal.
        let busy = self.outgoing.is_some() || self.xfade.is_some() || self.xfade_after.is_some();
        if !fits || busy || self.out().decks() < 2 {
            return false;
        }
        let p = self.prepared.take().expect("préparé");
        let (from, to) = (self.active, p.deck);
        self.active = to;
        self.current = Some(l.path.clone());
        self.skip_end[to] = false;
        self.signal_end = l.hold && l.signal_end;
        self.end_signaled = false;
        let vol = format!("{:.1}", self.volume);
        {
            let m = self.out().deck(to);
            let _ = m.set_property("keep-open", if l.hold { "yes" } else { "no" });
            let _ = m.set_property("volume", if xfade.is_some() { "0" } else { &vol });
        }
        let i = to.min(1);
        self.clips[i] = l.clip.clone();
        self.filters[i] = p.filters;
        self.out_fade_started = false;
        self.image_fade_at = None;
        // L'image préparée est déjà sur la carte : elle s'affiche au prochain rafraîchissement.
        self.out().show(to, true);
        match xfade {
            Some(secs) => {
                self.outgoing = Some(from);
                self.xfade = Some(Xfade { from, to, start: Instant::now(), secs });
            }
            None => {
                self.out().set_mix(to as f64);
                self.end_outgoing(from);
            }
        }
        let m = self.out().deck(to);
        let _ = m.set_property("pause", if l.paused { "yes" } else { "no" });
        // Le lecteur préparé se taisait : la régie reçoit ce qu'elle aurait appris au chargement.
        let duration = m.get_property("duration").and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let hwdec = m.get_property("hwdec-current");
        let image = m.get_property("current-tracks/video/image").as_deref() == Some("yes");
        let audio_only = m.get_property("current-tracks/video/id").is_none();
        self.duration = duration;
        emit(&Event::Loaded { path: l.path.clone() });
        if let Some(name) = hwdec {
            emit(&Event::Hwdec { name });
        }
        if audio_only {
            self.out().show(to, false);
        }
        if let Some(d) = l.clip.image_secs.filter(|_| image && l.clip.sound.fade_out > 0.0) {
            self.image_fade_at = Some(Instant::now() + Duration::from_secs_f64((d - l.clip.sound.fade_out).max(0.0)));
        }
        self.apply_cadence(to);
        true
    }

    /// Niveau de l'image (1 = normal, 0 = noir).
    fn set_fade(&mut self, v: f64) {
        self.fade_level = v.clamp(0.0, 1.0);
        let v = self.fade_level;
        if let Some(out) = &mut self.out {
            out.set_level(v);
        }
    }

    /// Avance les fondus en cours. Vrai tant qu'il faut continuer à se réveiller.
    fn step_anims(&mut self) -> bool {
        let mut busy = false;
        if let Some(a) = &self.anim {
            let k = if a.secs <= 0.0 { 1.0 } else { (a.start.elapsed().as_secs_f64() / a.secs).min(1.0) };
            let v = a.from + (a.to - a.from) * k;
            let audio = a.audio;
            self.set_fade(v);
            if audio && self.xfade.is_none() {
                let _ = self.m().set_property("volume", &format!("{:.1}", self.volume * v));
            }
            if k < 1.0 {
                busy = true;
            } else {
                self.anim = None;
                if let Some(l) = self.pending.take() {
                    if let Err(e) = self.load_now(l) {
                        emit(&Event::Error { message: e });
                    }
                }
            }
        }
        if let Some(x) = &self.xfade {
            let k = if x.secs <= 0.0 { 1.0 } else { (x.start.elapsed().as_secs_f64() / x.secs).min(1.0) };
            // Courbe douce au départ et à l'arrivée.
            let s = k * k * (3.0 - 2.0 * k);
            let (from, to) = (x.from, x.to);
            if let Some(out) = &self.out {
                out.set_mix(from as f64 + (to as f64 - from as f64) * s);
                // Son : puissance constante, pas de creux au milieu.
                let a = (s * std::f64::consts::FRAC_PI_2).cos();
                let b = (s * std::f64::consts::FRAC_PI_2).sin();
                let _ = out.deck(from).set_property("volume", &format!("{:.1}", self.volume * a));
                let _ = out.deck(to).set_property("volume", &format!("{:.1}", self.volume * b));
            }
            if k < 1.0 {
                busy = true;
            } else {
                self.xfade = None;
                self.outgoing = None;
                self.end_outgoing(from);
            }
        }
        busy
    }

    fn apply_warp(&mut self) {
        let corners = self.warp_corners;
        match &mut self.out {
            Some(Out::Composite(s)) => s.set_warp(corners),
            Some(Out::Embedded(e)) => {
                if let Err(e) = e.set_warp(corners) {
                    emit(&Event::Error { message: e });
                }
            }
            None => {}
        }
    }

    fn set_monitor(&mut self, on: bool) {
        if on && !self.feed.as_ref().is_some_and(|f| f.alive()) {
            self.feed = self.monitor_port.map(monitor::MonitorFeed::connect);
        }
        match &self.out {
            Some(Out::Composite(s)) => {
                // Le compositeur fabrique les vignettes lui-même (60 par seconde au plus).
                let feed = if on { self.feed.take() } else { None };
                s.monitor(feed.map(|f| (f, Duration::from_secs_f64(1.0 / 60.0))));
                if !on {
                    self.feed = None;
                }
            }
            _ => {
                // Secours : capture par mpv, cadence réduite (elle bloque la lecture).
                self.monitor_every = on.then(|| Duration::from_millis(200));
                self.monitor_next = Instant::now();
            }
        }
    }

    /// FPS choisi dans les Préférences : l'écran de sortie passe sur cette fréquence (ou un
    /// multiple qu'il propose), avant l'ouverture des lecteurs.
    fn force_refresh(&mut self) {
        let (Some(fps), Some(dev)) = (self.prefs.fps, self.device.clone()) else { return };
        if (fps - self.refresh_hz).abs() < 0.5 {
            return;
        }
        // Fréquence exacte si l'écran la propose, sinon un multiple (50 Hz pour 25 i/s…).
        let exact = self.modes.is_empty() || self.modes.iter().any(|m| (m - fps).abs() < 0.5);
        let Some(hz) = (if exact { Some(fps) } else { refresh::best(fps, &self.modes) }) else {
            emit(&Event::Error { message: format!("l'écran de sortie ne propose pas {fps:.0} Hz") });
            return;
        };
        if (hz - self.refresh_hz).abs() < 0.5 {
            return;
        }
        match refresh::set(&dev, hz) {
            Ok(()) => {
                self.refresh_changed = true;
                self.refresh_hz = refresh::current(&dev).unwrap_or(hz);
            }
            Err(e) => emit(&Event::Error { message: format!("FPS {fps:.0} : {e}") }),
        }
    }

    /// Calage des images sur l'écran, pour le fichier qui vient d'être chargé sur `deck`.
    fn apply_cadence(&mut self, deck: usize) {
        let Some(out) = &self.out else { return };
        let m = out.deck(deck);
        let fps = m.get_property("container-fps").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        // Fréquence de l'écran adaptée à la vidéo (si demandé et possible).
        if self.prefs.fps.is_none() && self.prefs.adapt_refresh && fps > 1.0 && deck == self.active {
            if let (Some(dev), Some(hz)) = (self.device.clone(), refresh::best(fps, &self.modes)) {
                if (hz - self.refresh_hz).abs() > 0.5 && !refresh::fits(fps, self.refresh_hz) {
                    match refresh::set(&dev, hz) {
                        Ok(()) => {
                            self.refresh_changed = true;
                            self.refresh_hz = refresh::current(&dev).unwrap_or(hz);
                            if let Some(Out::Composite(s)) = &self.out {
                                s.set_refresh(self.refresh_hz);
                                for i in 0..s.deck_count() {
                                    let _ = s.deck(i).set_property("display-fps-override", &format!("{:.3}", self.refresh_hz));
                                }
                            }
                        }
                        Err(e) => emit(&Event::Error { message: e }),
                    }
                }
            }
        }
        let Some(out) = &self.out else { return };
        let m = out.deck(deck);
        let interp = self.prefs.interpolation;
        let resample = match self.prefs.cadence {
            Cadence::Audio => false,
            Cadence::Resample => true,
            // Mesuré au banc (tools/bench-sortie) : quand l'écran tombe juste (30 i/s sur 60 Hz),
            // la synchro audio donne la même cadence sans risque ; le recalage ne sert qu'aux
            // cadences presque justes (29,97 sur 60 Hz). Au-delà de la 1440p, il perdait jusqu'à
            // 4 images par seconde en HEVC et en ProRes : synchro audio là aussi.
            Cadence::Auto => {
                let height = m.get_property("height").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                refresh::fits(fps, self.refresh_hz) && !refresh::exact(fps, self.refresh_hz) && height <= 1440
            }
        };
        // L'interpolation demande le calage sur l'écran.
        let sync = if resample || interp { "display-resample" } else { "audio" };
        let _ = m.set_property("video-sync", sync);
        let _ = m.set_property("interpolation", if interp { "yes" } else { "no" });
        if interp {
            let _ = m.set_property("tscale", "oversample");
        }
    }

    /// Sorties audio vues par mpv, pour le choix dans la régie.
    fn emit_audio_devices(&self) {
        let Some(out) = &self.out else { return };
        let list = out
            .deck(0)
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

    /// Envoie au lecteur actif le média courant du diaporama.
    fn show_current(&mut self) {
        let Some(path) = self.show.as_mut().and_then(|s| {
            s.deadline = None;
            s.current().map(str::to_string)
        }) else {
            return;
        };
        let video = slideshow::is_video(&path);
        let sound = self.show.as_ref().map(|s| s.video_sound).unwrap_or(false);
        let (pos, len) = self.show.as_ref().map(|s| (s.pos, s.len())).unwrap_or((0, 0));
        let a = self.active;
        self.stop_warming(a);
        let m = self.m();
        // Diaporama : une vidéo finie passe la main au média suivant.
        let _ = m.set_property("keep-open", "no");
        let _ = m.set_property("loop-file", "no");
        let _ = m.set_property("mute", if video && !sound { "yes" } else { "no" });
        apply_clip(m, None, false);
        let r = m.command(&["loadfile", &path, "replace"]);
        self.out().reveal(a);
        self.current = Some(path.clone());
        emit(&Event::Slide { index: pos, count: len, path });
        if let Err(e) = r {
            emit(&Event::Error { message: e });
        }
    }

    /// Ajustement de l'image courante (panscan 1 = remplir en recadrant).
    fn apply_fit(&self) {
        let (Some(_), Some(s)) = (&self.out, &self.show) else { return };
        let m = self.m();
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

    fn mpv_events(&mut self, el: &ActiveEventLoop, deck: usize) {
        let Some(out) = self.out.take() else { return };
        // Seul le lecteur actif parle à la régie ; le lecteur qui sort et le préchauffage se taisent.
        let mine = deck == self.active;
        let mut next = false;
        let mut started = false;
        let mut refit = false;
        let mut first_frame = false;
        let mut loaded = false;
        let mut audio_only = false;
        let mut idle = false;
        let mut park_after = false;
        let mut failed_prep = false;
        {
            let m = out.deck(deck);
            while let Some(ev) = m.next_event() {
                if self.warming.get(deck).copied().unwrap_or(0) > 0 {
                    // Chaque fin de média du préchauffage le rapproche de la fin.
                    if ev.event_id == mpv::EVENT_END_FILE {
                        self.warming[deck] -= 1;
                        if self.warming[deck] == 0 {
                            idle = true;
                        }
                    }
                    continue;
                }
                let prep = self.prepared.as_mut().filter(|p| p.deck == deck && !mine);
                if let Some(p) = prep {
                    // Lecteur en préparation : il se tait, on note seulement qu'il est prêt.
                    match ev.event_id {
                        mpv::EVENT_FILE_LOADED => self.skip_end[deck] = false,
                        mpv::EVENT_PLAYBACK_RESTART => p.ready = true,
                        mpv::EVENT_END_FILE if !self.skip_end[deck] => {
                            let end = unsafe { &*(ev.data as *const mpv::RawEndFile) };
                            if end.reason == mpv::END_FILE_ERROR {
                                failed_prep = true;
                            }
                        }
                        mpv::EVENT_END_FILE => self.skip_end[deck] = false,
                        _ => {}
                    }
                    continue;
                }
                match ev.event_id {
                    mpv::EVENT_SHUTDOWN => el.exit(),
                    mpv::EVENT_END_FILE if self.skip_end[deck] => {}
                    mpv::EVENT_FILE_LOADED if mine => {
                        loaded = true;
                        self.skip_end[deck] = false;
                        audio_only = m.get_property("current-tracks/video/id").is_none();
                        started = true;
                    }
                    mpv::EVENT_VIDEO_RECONFIG if mine => refit = true,
                    mpv::EVENT_PLAYBACK_RESTART if mine => first_frame = true,
                    mpv::EVENT_END_FILE if mine => {
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
                                // Sans « garder la dernière image », l'écran passe au noir.
                                if m.get_property("keep-open").as_deref() != Some("yes") {
                                    out.show(deck, false);
                                    park_after = true;
                                }
                                emit(&Event::Ended);
                            }
                        } else if self.show.is_none() {
                            emit(&Event::Idle);
                        }
                    }
                    mpv::EVENT_PROPERTY_CHANGE if mine => {
                        let p = unsafe { &*(ev.data as *const mpv::RawProperty) };
                        self.property(ev.reply_userdata % 100, p);
                    }
                    _ => {}
                }
            }
        }
        if idle {
            // Préchauffage terminé : son ouvert, rendu prêt.
            let m = out.deck(deck);
            let _ = m.set_property("mute", "no");
            out.show(deck, false);
        }
        let paused = mine && out.deck(deck).get_property("pause").as_deref() == Some("yes");
        self.out = Some(out);
        if failed_prep {
            // Média illisible : le GO le chargera normalement (et signalera l'erreur).
            self.prepared = None;
            self.park(deck);
        }
        if park_after && matches!(self.out, Some(Out::Composite(_))) {
            self.current = None;
            self.park(deck);
        }
        if loaded {
            if !self.pattern_on {
                emit(&Event::Loaded { path: self.current.clone().unwrap_or_default() });
            }
            if audio_only {
                self.out().show(deck, false);
            }
            self.apply_cadence(deck);
        }
        if first_frame && mine {
            // Image fixe à durée réglée : son fondu de sortie part à l'heure (sa position ne bouge pas).
            let c = &self.clips[deck.min(1)];
            if let Some(d) = c.image_secs.filter(|_| c.sound.fade_out > 0.0) {
                if self.out().deck(deck).get_property("current-tracks/video/image").as_deref() == Some("yes") {
                    self.image_fade_at = Some(Instant::now() + Duration::from_secs_f64((d - c.sound.fade_out).max(0.0)));
                }
            }
        }
        if first_frame {
            // Première image du nouveau média : fondu d'ouverture, ou fondu enchaîné.
            if self.pending.is_none() {
                if let Some(secs) = self.fade_in_after.take() {
                    self.anim = Some(FadeAnim { from: self.fade_level, to: 1.0, start: Instant::now(), secs, audio: std::mem::take(&mut self.fade_in_audio) });
                }
            }
            if let (Some(secs), Some(from)) = (self.xfade_after.take(), self.outgoing) {
                self.xfade = Some(Xfade { from, to: deck, start: Instant::now(), secs });
            }
        }
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

    /// Fondu de sortie d'un élément édité : il démarre quand la lecture atteint
    /// « fin − durée du fondu », et l'image (et le son) arrivent au noir pile à la fin.
    fn check_out_fade(&mut self, t: f64) {
        let c = &self.clips[self.active.min(1)].sound;
        if self.out_fade_started || c.fade_out <= 0.0 || c.looping || self.anim.is_some() || self.xfade.is_some() || self.pending.is_some() {
            return;
        }
        let end = c.end.filter(|e| *e > c.start).unwrap_or(self.duration);
        if end <= 0.0 || t < end - c.fade_out || t >= end {
            return;
        }
        if !self.out.as_ref().is_some_and(Out::can_fade) {
            return;
        }
        self.out_fade_started = true;
        self.anim = Some(FadeAnim { from: self.fade_level, to: 0.0, start: Instant::now(), secs: (end - t).max(0.05), audio: true });
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
                    self.check_out_fade(t);
                }
            }
            P_DURATION => self.duration = double().unwrap_or(0.0),
            P_EOF => {
                // Média gardé à l'écran en fin de lecture : la fin est signalée si on l'a demandé.
                let eof = p.format == mpv::FORMAT_FLAG && !p.data.is_null() && unsafe { *(p.data as *const c_int) } != 0;
                if eof && self.signal_end && !self.end_signaled && self.show.is_none() {
                    self.end_signaled = true;
                    emit(&Event::Ended);
                }
            }
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

impl Drop for OutputApp {
    fn drop(&mut self) {
        // L'écran retrouve sa fréquence d'origine.
        if self.refresh_changed {
            if let Some(d) = &self.device {
                refresh::restore(d);
            }
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
            UserEv::Mpv(deck) => self.mpv_events(el, deck),
            UserEv::Detached => el.exit(),
        }
    }

    /// Minutage des photos et des fondus : on se réveille pile à l'échéance.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.image_fade_at.is_some_and(|at| at <= Instant::now()) {
            self.image_fade_at = None;
            let secs = self.clips[self.active.min(1)].sound.fade_out;
            if self.anim.is_none() && self.xfade.is_none() && !self.out_fade_started {
                self.out_fade_started = true;
                self.anim = Some(FadeAnim { from: self.fade_level, to: 0.0, start: Instant::now(), secs, audio: true });
            }
        }
        let fading = self.step_anims();
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
                if let (Some(Out::Embedded(e)), Some(feed)) = (&self.out, &mut self.feed) {
                    e.capture(feed);
                }
                self.monitor_next = now + every;
            }
        }
        // Pendant un fondu, on se réveille à chaque image (~120 Hz) ; sinon fin de photo ou prochaine vignette.
        let fade_tick = fading.then(|| now + Duration::from_millis(8));
        let next = [self.show.as_ref().and_then(|s| s.deadline), self.monitor_every.map(|_| self.monitor_next), fade_tick, self.image_fade_at].into_iter().flatten().min();
        el.set_control_flow(match next {
            Some(d) => ControlFlow::WaitUntil(d),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            // Échap n'arrive ici que si on a cliqué sur la sortie (focus clavier) : elle se ferme.
            // Partout ailleurs, Échap reste à la régie (noir).
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed() && event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) =>
            {
                el.exit()
            }
            WindowEvent::Resized(s) => {
                if let Some(Out::Composite(scr)) = &self.out {
                    scr.resize(s.width, s.height);
                }
                self.paint_cpu();
            }
            WindowEvent::RedrawRequested => self.paint_cpu(),
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

    let volume = prefs.volume.clamp(0.0, 100.0);
    let mut app = OutputApp {
        prefs,
        out: None,
        wakes: Vec::new(),
        proxy,
        window: None,
        surface: None,
        pattern_on: false,
        show: None,
        warp_corners: None,
        active: 0,
        outgoing: None,
        xfade_after: None,
        xfade: None,
        warming: Vec::new(),
        skip_end: Vec::new(),
        volume,
        fade_level: 1.0,
        anim: None,
        pending: None,
        fade_in_after: None,
        fade_in_audio: false,
        clips: Default::default(),
        filters: Default::default(),
        out_fade_started: false,
        image_fade_at: None,
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
        device: None,
        refresh_hz: 60.0,
        modes: Vec::new(),
        refresh_changed: false,
        prepared: None,
        prepare_after: None,
        signal_end: false,
        end_signaled: false,
    };
    let _ = event_loop.run_app(&mut app);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Essai avec une vraie libmpv : `CUESTAR_TEST_MEDIA=<vidéo avec son> cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn clip_options_play_on_real_mpv() {
        let media = std::env::var("CUESTAR_TEST_MEDIA").expect("CUESTAR_TEST_MEDIA");
        let m = Mpv::create().unwrap();
        for (k, v) in [("vo", "null"), ("ao", "null"), ("untimed", "yes"), ("idle", "yes"), ("keep-open", "no"), ("terminal", "no")] {
            m.set_option(k, v).unwrap();
        }
        m.initialize().unwrap();
        let mut clip = Clip::default();
        clip.sound.start = 2.0;
        clip.sound.end = Some(5.0);
        clip.sound.volume_db = -6.0;
        clip.sound.eq.low_cut.on = true;
        clip.sound.eq.low_cut_slope = star_core::eq::CutSlope::Db24;
        clip.sound.eq.peak1.on = true;
        clip.sound.eq.peak1.gain_db = 4.0;
        clip.sound.eq.low_shelf.on = true;
        clip.sound.eq.low_shelf.gain_db = -2.0;
        clip.sound.eq.high_shelf.on = true;
        clip.sound.eq.high_shelf.gain_db = 3.0;
        let stages = apply_clip(&m, Some(&clip), false);
        m.command(&["loadfile", &media, "replace"]).unwrap();
        let (mut first, mut last, mut eof, mut err) = (None, 0.0f64, false, false);
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(20) && !eof {
            if let Some(ev) = m.wait_event(0.05) {
                if ev.event_id == mpv::EVENT_PLAYBACK_RESTART && first.is_none() {
                    first = m.get_property("time-pos").and_then(|s| s.parse::<f64>().ok());
                    // Changement à chaud d'un gain : sans reconstruire la chaîne.
                    let mut c2 = clip.clone();
                    c2.sound.eq.peak1.gain_db = -5.0;
                    c2.sound.volume_db = -3.0;
                    for [l, k, v, t] in audio_filter::live_commands(&stages, &audio_filter::stages(&c2.sound)).unwrap() {
                        m.command(&["af-command", &l, &k, &v, &t]).unwrap();
                    }
                }
                if ev.event_id == mpv::EVENT_END_FILE {
                    let end = unsafe { &*(ev.data as *const mpv::RawEndFile) };
                    eof = true;
                    err = end.reason == mpv::END_FILE_ERROR;
                }
            }
            if let Some(t) = m.get_property("time-pos").and_then(|s| s.parse::<f64>().ok()) {
                last = last.max(t);
            }
        }
        let af = m.get_property("af").unwrap_or_default();
        assert!(eof && !err, "lecture : eof={eof} erreur={err}");
        assert!((first.unwrap() - 2.0).abs() < 0.1, "début {first:?}");
        assert!(last <= 5.05 && last > 4.5, "fin {last}");
        assert!(af.contains("cs_p1"), "af = {af}");
        // Boucle entre les traits : la lecture repasse par le début de boucle.
        let mut clip = Clip::default();
        clip.sound.looping = true;
        clip.sound.loop_start = Some(1.0);
        clip.sound.loop_end = Some(2.0);
        apply_clip(&m, Some(&clip), true);
        m.command(&["loadfile", &media, "replace"]).unwrap();
        let (mut max, mut wrapped, mut prev) = (0.0f64, false, 0.0f64);
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(3) {
            let _ = m.wait_event(0.01);
            if let Some(t) = m.get_property("time-pos").and_then(|s| s.parse::<f64>().ok()) {
                max = max.max(t);
                if t + 0.5 < prev {
                    wrapped = true;
                }
                prev = t;
            }
        }
        assert!(wrapped && max < 2.2, "boucle : max {max}, retour {wrapped}");
    }
}
