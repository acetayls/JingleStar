//! Affichage de secours : mpv dessine seul dans la fenêtre de sortie (`wid`), comme avant
//! le compositeur. Sert si l'API de rendu n'est pas disponible (vieille libmpv, pilote…).
//! Un seul lecteur : pas de fondu enchaîné ; fondu au noir et mapping par shader mpv
//! (vo gpu-next) ; monitoring par capture `screenshot-raw` (coûteuse, cadence réduite).

use super::monitor::MonitorFeed;
use super::mpv::Mpv;
use super::warp;

pub struct Embedded {
    mpv: Mpv,
    shaders: warp::ShaderFiles,
    corners: Option<[[f32; 2]; 4]>,
    /// Le shader porte le réglage de niveau (fondu au noir possible).
    fade_ok: bool,
    level: f64,
    light: bool,
}

impl Embedded {
    pub fn new(mpv: Mpv, light: bool) -> Self {
        Self { mpv, shaders: warp::ShaderFiles::new(), corners: None, fade_ok: false, level: 1.0, light }
    }

    pub fn deck(&self) -> &Mpv {
        &self.mpv
    }

    pub fn fade_ok(&self) -> bool {
        self.fade_ok
    }

    pub fn set_level(&mut self, v: f64) {
        self.level = v.clamp(0.0, 1.0);
        if self.fade_ok {
            let _ = self.mpv.set_property("glsl-shader-opts", &format!("fade={:.4}", self.level));
        }
    }

    /// Shader de sortie : mapping 4 coins, et réglage de niveau pour les fondus (vo gpu-next).
    pub fn set_warp(&mut self, corners: Option<[[f32; 2]; 4]>) -> Result<(), String> {
        self.corners = corners;
        let with_fade = !self.light;
        let c = if with_fade { Some(corners.unwrap_or(crate::prefs::Warp::IDENTITY)) } else { corners };
        self.fade_ok = false;
        let res = match c.and_then(|c| warp::shader(c, with_fade)) {
            Some(src) => match self.shaders.write(&src) {
                Ok(p) => {
                    let r = self.mpv.set_property("glsl-shaders", &p.to_string_lossy());
                    self.fade_ok = with_fade && r.is_ok();
                    r
                }
                Err(e) => Err(format!("mapping 4 coins : {e}")),
            },
            None => self.mpv.set_property("glsl-shaders", ""),
        };
        if self.fade_ok {
            let v = self.level;
            self.set_level(v);
        }
        res
    }

    /// Envoie une vignette de l'image en cours (copie par mpv : bloque la lecture quelques ms).
    pub fn capture(&self, feed: &mut MonitorFeed) {
        match self.mpv.screenshot_rgba() {
            Some((w, h, stride, px)) => feed.send(w, h, stride, &px),
            None => feed.send_black(),
        }
    }
}
