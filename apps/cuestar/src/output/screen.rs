//! Affichage de la sortie : CueStar dessine lui-même chaque image (API de rendu de mpv).
//!
//! Chaque lecteur (« deck ») mpv dessine dans sa propre texture ; un compositeur les mélange
//! (fondu enchaîné), règle le niveau (fondu au noir), applique le mapping 4 coins et affiche
//! le tout au rythme de l'écran. Le monitoring est une copie réduite faite par le GPU et lue
//! sans attendre : il ne ralentit jamais la sortie.
//!
//! Ce fichier contient la partie commune ; le dessin lui-même dépend du système :
//! - Windows : Direct3D 11 (`screen_d3d11`) ;
//! - macOS (Intel et Apple Silicon) et Linux : OpenGL (`screen_gl`).
//!
//! Tout le travail graphique se fait dans un fil dédié ; le reste du moteur ne voit que
//! `Screen` (réglages) et les instances mpv (lecture).

use super::monitor::MonitorFeed;
use super::mpv::{self, FrameInfo, Handle, Mpv, RenderApi, RenderParam};
use std::ffi::c_void;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Nombre de lecteurs mpv dans la sortie (deux : le fondu enchaîné passe de l'un à l'autre).
pub const DECKS: usize = 2;
/// Largeur des vignettes de monitoring.
pub const THUMB_W: u32 = 640;

/// Réglages partagés entre le moteur (fil principal) et le fil d'affichage.
pub(super) struct State {
    /// Niveau de l'image : 1 normal, 0 noir.
    level: f32,
    /// Mélange des lecteurs : 0 = lecteur 0, 1 = lecteur 1.
    mix: f32,
    /// Homographie écran → image (mapping 4 coins).
    warp: [[f32; 3]; 3],
    pub(super) size: (u32, u32),
    /// Durée d'un rafraîchissement de l'écran (µs).
    vsync_us: i64,
    /// Lecteur visible (faux : noir à sa place).
    shown: [bool; DECKS],
    /// Rendre le lecteur visible dès sa prochaine image (média qui vient d'être chargé).
    reveal: [bool; DECKS],
    /// Signaler à la régie la première image affichée de ce lecteur (média qui vient d'être lancé).
    announce: [bool; DECKS],
    /// Nouveau réglage du monitoring à prendre en compte : None = arrêt.
    monitor: Option<Option<(MonitorFeed, Duration)>>,
    /// Réglage changé : il faut recomposer l'image.
    dirty: bool,
    /// Un lecteur a signalé du nouveau.
    wake: bool,
    stop: bool,
}

pub(super) struct Shared {
    pub(super) st: Mutex<State>,
    cv: Condvar,
}

/// Appelée par mpv (depuis un de ses fils) quand un lecteur a du nouveau à dessiner.
pub(super) unsafe extern "C" fn on_update(p: *mut c_void) {
    let s = unsafe { &*(p as *const Shared) };
    if let Ok(mut g) = s.st.lock() {
        g.wake = true;
    }
    s.cv.notify_one();
}

/// Paramètres du shader de composition (même disposition en HLSL et en GLSL).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Params {
    /// Homographie écran → image, une ligne par vec4.
    pub h: [[f32; 4]; 3],
    /// x = mélange (0 : lecteur 0), y = niveau, z = lecteur 0 visible, w = lecteur 1 visible.
    pub misc: [f32; 4],
}

/// Contexte de rendu mpv d'un lecteur, tel que le voit le fil d'affichage.
pub(super) struct DeckRc {
    pub rc: Handle,
    /// Instance mpv du lecteur (pour son horloge).
    pub mpv: Handle,
    pub api: RenderApi,
    /// Une image attend d'être dessinée.
    has_frame: bool,
    /// Heure d'affichage voulue de cette image (µs, horloge mpv).
    target: Option<i64>,
    /// Le lecteur a déjà dessiné au moins une image (à redessiner après un changement de taille).
    drawn: bool,
}

impl DeckRc {
    pub fn new(rc: Handle, mpv: Handle, api: RenderApi) -> Self {
        Self { rc, mpv, api, has_frame: false, target: None, drawn: false }
    }
}

/// Ce que chaque moteur graphique (Direct3D 11, OpenGL) sait faire.
pub(super) trait Backend {
    /// Nom de la carte graphique.
    fn gpu(&self) -> String;
    fn resize(&mut self, size: (u32, u32)) -> Result<(), String>;
    /// Attend que l'écran soit prêt à recevoir une image (Direct3D : objet d'attente de la chaîne).
    fn wait_ready(&mut self) {}
    /// Le lecteur `i` dessine son image courante dans sa texture.
    fn draw_deck(&mut self, i: usize, d: &DeckRc);
    fn compose(&mut self, p: &Params);
    /// Même composition, en petit, mise de côté pour être lue plus tard.
    fn compose_thumb(&mut self, p: &Params);
    fn present(&mut self);
    /// Envoie les vignettes prêtes, sans jamais attendre la carte graphique.
    fn read_thumbs(&mut self, feed: &mut MonitorFeed);
    /// Des vignettes attendent encore d'être lues.
    fn thumbs_pending(&self) -> bool;
}

/// Pointeur passé au fil d'affichage (les objets qu'il désigne restent valides jusqu'à son arrêt).
pub(super) struct SendPtr(pub isize);
unsafe impl Send for SendPtr {}

/// Façade de l'affichage, vue du moteur de sortie.
pub struct Screen {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    /// Lecteurs mpv. Détruits après l'arrêt du fil d'affichage (qui libère leurs contextes de rendu).
    decks: Vec<Mpv>,
    /// Nom de la carte graphique utilisée.
    pub gpu: String,
}

impl Screen {
    /// Démarre le fil d'affichage. `setup` y crée le moteur graphique et les contextes de rendu mpv.
    pub(super) fn start<B, F>(size: (u32, u32), refresh_hz: f64, decks: Vec<Mpv>, setup: F) -> Result<Self, String>
    where
        B: Backend,
        F: FnOnce(&Arc<Shared>, &[(Handle, RenderApi)]) -> Result<(B, Vec<DeckRc>), String> + Send + 'static,
    {
        let mut links = Vec::new();
        for d in &decks {
            let api = d.render_api().ok_or("cette libmpv ne propose pas l'API de rendu")?;
            links.push((SendPtr(d.raw() as isize), api));
        }
        let shared = Arc::new(Shared {
            st: Mutex::new(State {
                level: 1.0,
                mix: 0.0,
                warp: super::warp::inverse(None),
                size: (size.0.max(1), size.1.max(1)),
                vsync_us: vsync_us(refresh_hz),
                shown: [false; DECKS],
                reveal: [false; DECKS],
                announce: [false; DECKS],
                monitor: None,
                dirty: true,
                wake: false,
                stop: false,
            }),
            cv: Condvar::new(),
        });
        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name("sortie-affichage".into())
            .spawn(move || {
                let links: Vec<(Handle, RenderApi)> = links.into_iter().map(|(p, a)| (p.0 as Handle, a)).collect();
                match setup(&sh, &links) {
                    Ok((backend, decks)) => {
                        let _ = tx.send(Ok(backend.gpu()));
                        run(sh, backend, decks);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let mut screen = Self { shared, thread: Some(thread), decks, gpu: String::new() };
        match rx.recv() {
            Ok(Ok(gpu)) => {
                screen.gpu = gpu;
                Ok(screen)
            }
            Ok(Err(e)) => {
                if let Some(t) = screen.thread.take() {
                    let _ = t.join();
                }
                Err(e)
            }
            Err(_) => Err("le fil d'affichage s'est arrêté au démarrage".into()),
        }
    }

    pub fn deck(&self, i: usize) -> &Mpv {
        &self.decks[i]
    }

    pub fn deck_count(&self) -> usize {
        self.decks.len()
    }

    fn with(&self, f: impl FnOnce(&mut State)) {
        if let Ok(mut g) = self.shared.st.lock() {
            f(&mut g);
            g.dirty = true;
        }
        self.shared.cv.notify_one();
    }

    pub fn set_level(&self, v: f64) {
        self.with(|s| s.level = v.clamp(0.0, 1.0) as f32);
    }

    pub fn set_mix(&self, v: f64) {
        self.with(|s| s.mix = v.clamp(0.0, 1.0) as f32);
    }

    pub fn set_warp(&self, corners: Option<[[f32; 2]; 4]>) {
        let m = super::warp::inverse(corners);
        self.with(|s| s.warp = m);
    }

    /// Fréquence de l'écran de sortie (après un changement de mode, par exemple).
    pub fn set_refresh(&self, hz: f64) {
        self.with(|s| s.vsync_us = vsync_us(hz));
    }

    pub fn resize(&self, w: u32, h: u32) {
        if w > 0 && h > 0 {
            self.with(|s| s.size = (w, h));
        }
    }

    /// Lecteur visible ou masqué (noir).
    pub fn show(&self, deck: usize, on: bool) {
        self.with(|s| {
            s.shown[deck] = on;
            s.reveal[deck] = false;
            s.announce[deck] = on;
        });
    }

    /// Le lecteur s'affichera dès sa prochaine image (l'image précédente reste jusque-là).
    pub fn reveal(&self, deck: usize) {
        self.with(|s| {
            s.reveal[deck] = true;
            s.announce[deck] = true;
        });
    }

    pub fn monitor(&self, feed: Option<(MonitorFeed, Duration)>) {
        self.with(|s| s.monitor = Some(feed));
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        self.with(|s| s.stop = true);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // Les instances mpv sont détruites ensuite (champ `decks`).
    }
}

/// Boucle du fil d'affichage : dessine les lecteurs quand leur image est due, compose, affiche.
fn vsync_us(hz: f64) -> i64 {
    (1_000_000.0 / hz.clamp(24.0, 500.0)) as i64
}

fn run<B: Backend>(shared: Arc<Shared>, mut g: B, mut decks: Vec<DeckRc>) {
    // CUESTAR_DEBUG : une ligne de statistiques par seconde sur la sortie d'erreur.
    let debug = std::env::var_os("CUESTAR_DEBUG").is_some();
    let mut stat = (Instant::now(), [0u32; DECKS], [0u32; DECKS], 0u32, [0i64; DECKS]);
    let mut monitor: Option<(MonitorFeed, Duration)> = None;
    let mut next_thumb = Instant::now();
    let mut size = shared.st.lock().map(|s| s.size).unwrap_or((1, 1));

    loop {
        // Réglages du moment.
        let (mut params, new_size, reveal, stop, mon, dirty, vsync_us) = {
            let Ok(mut s) = shared.st.lock() else { break };
            let w = s.warp;
            let p = Params {
                h: [[w[0][0], w[0][1], w[0][2], 0.0], [w[1][0], w[1][1], w[1][2], 0.0], [w[2][0], w[2][1], w[2][2], 0.0]],
                misc: [s.mix, s.level, s.shown[0] as u8 as f32, s.shown[1] as u8 as f32],
            };
            let r = (p, s.size, s.reveal, s.stop, s.monitor.take(), s.dirty, s.vsync_us);
            s.dirty = false;
            s.wake = false;
            r
        };
        if stop {
            break;
        }
        let mut dirty = dirty;
        if let Some(m) = mon {
            monitor = m;
            next_thumb = Instant::now();
            dirty = true;
        }
        let resized = new_size != size;
        if resized {
            match g.resize(new_size) {
                Ok(()) => size = new_size,
                Err(e) => eprintln!("{e}"),
            }
        }

        // Nouvelles images des lecteurs.
        let now = decks.first().map(|d| unsafe { (d.api.time_us)(d.mpv) }).unwrap_or(0);
        for (di, d) in decks.iter_mut().enumerate() {
            let flags = unsafe { (d.api.update)(d.rc) };
            if flags & mpv::RENDER_UPDATE_FRAME != 0 {
                stat.2[di] += 1;
            }
            if flags & mpv::RENDER_UPDATE_FRAME != 0 {

                d.has_frame = true;
                let mut info = FrameInfo::default();
                let ok = unsafe { (d.api.get_info)(d.rc, RenderParam { kind: mpv::RENDER_PARAM_NEXT_FRAME_INFO, data: &mut info as *mut FrameInfo as *mut c_void }) } >= 0;
                // L'en-tête de mpv annonce des µs, mais les versions récentes donnent des ns :
                // on garde l'unité qui tombe le plus près de l'horloge.
                let t = info.target_time;
                let t = if (t / 1000 - now).abs() < (t - now).abs() { t / 1000 } else { t };
                d.target = (ok && info.flags & mpv::FRAME_INFO_PRESENT != 0 && t > 0).then_some(t);
            }
        }
        // Une image est due si son heure tombe avant le prochain rafraîchissement.
        let due: Vec<bool> = decks.iter().map(|d| d.has_frame && d.target.map(|t| t <= now + vsync_us).unwrap_or(true)).collect();
        if !due.iter().any(|x| *x) && !dirty && !resized {
            // Rien à faire : on attend un lecteur, un réglage, ou l'heure de la prochaine image.
            let next = decks.iter().filter(|d| d.has_frame).filter_map(|d| d.target).min();
            let mut wait = next.map(|t| Duration::from_micros((t - vsync_us - now).clamp(0, 100_000) as u64)).unwrap_or(Duration::from_millis(100));
            // Vignettes encore sur la carte : on repasse vite les chercher.
            if let Some((feed, _)) = &mut monitor {
                g.read_thumbs(feed);
                if g.thumbs_pending() {
                    wait = wait.min(Duration::from_millis(4));
                }
            }
            if let Ok(s) = shared.st.lock() {
                let _ = shared.cv.wait_timeout_while(s, wait, |s| !s.wake && !s.dirty && !s.stop);
            }
            continue;
        }

        g.wait_ready();
        let mut revealed = [false; DECKS];
        for (i, d) in decks.iter_mut().enumerate() {
            if due[i] || (resized && d.drawn) {
                g.draw_deck(i, d);
                stat.1[i] += 1;
                if let Some(t) = d.target {
                    stat.4[i] = stat.4[i].max(now - t);
                }
                d.has_frame = false;
                d.target = None;
                d.drawn = true;
                revealed[i] = reveal[i];
            }
        }
        if revealed.iter().any(|x| *x) {
            if let Ok(mut s) = shared.st.lock() {
                for i in 0..DECKS {
                    if revealed[i] && s.reveal[i] {
                        s.reveal[i] = false;
                        s.shown[i] = true;
                        params.misc[2 + i] = 1.0;
                    }
                }
            }
        }
        g.compose(&params);
        if let Some((_, every)) = &monitor {
            if Instant::now() >= next_thumb {
                g.compose_thumb(&params);
                next_thumb = Instant::now() + *every;
            }
        }
        g.present();
        stat.3 += 1;
        // Première image d'un média lancé : elle est partie vers l'écran (lecteur visible et mélangé).
        if let Ok(mut s) = shared.st.lock() {
            for i in 0..DECKS {
                let weight = if i == 0 { 1.0 - params.misc[0] } else { params.misc[0] };
                if s.announce[i] && params.misc[2 + i] > 0.5 && weight > 0.01 && decks.get(i).is_some_and(|d| d.drawn) {
                    s.announce[i] = false;
                    super::emit(&crate::protocol::Event::OnScreen);
                }
            }
        }
        if debug && stat.0.elapsed() >= Duration::from_secs(1) {
            eprintln!("rendus {:?} images reçues {:?} affichages {} avance max µs {:?}", stat.1, stat.2, stat.3, stat.4);
            stat = (Instant::now(), [0; DECKS], [0; DECKS], 0, [i64::MIN / 2; DECKS]);
        }
        for d in &decks {
            unsafe { (d.api.report_swap)(d.rc) };
        }
        if let Some((feed, _)) = &mut monitor {
            g.read_thumbs(feed);
        }
    }
    // Les contextes de rendu sont libérés ici, tant que le moteur graphique existe encore.
    for d in &decks {
        unsafe { (d.api.free)(d.rc) };
    }
    drop(g);
}
