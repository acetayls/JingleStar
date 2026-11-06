//! Moteur graphique macOS (Intel et Apple Silicon) et Linux : OpenGL 3.2 (profil « core »).
//! Décodage GPU : VideoToolbox sur macOS, VAAPI ou NVDEC sous Linux. Voir `screen`.
//!
//! Écrit et compilé sous Windows mais pas encore essayé sur une vraie machine macOS/Linux :
//! points à vérifier en premier — image à l'endroit, synchro de l'écran, décodage GPU actif.

use super::monitor::MonitorFeed;
use super::mpv::{self, Handle, Mpv, RenderApi, RenderParam};
use super::screen::{on_update, Backend, DeckRc, Params, Screen, Shared, THUMB_W};
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentContext, NotCurrentGlContext, PossiblyCurrentContext, Version};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::window::Window;

/// Ce qui est créé dans le fil principal (fenêtre) puis confié au fil d'affichage.
struct GlParts {
    display: Display,
    context: NotCurrentContext,
    surface: Surface<WindowSurface>,
    /// Display X11 ou Wayland (décodage VAAPI sous Linux).
    native: Option<(c_int, isize)>,
}
// Les objets OpenGL ne sont utilisés que par le fil d'affichage une fois confiés.
unsafe impl Send for GlParts {}

/// Ouvre l'affichage OpenGL dans `window`.
pub fn open(window: &Window, size: (u32, u32), refresh_hz: f64, decks: Vec<Mpv>) -> Result<Screen, String> {
    let raw_display = window.display_handle().map_err(|e| e.to_string())?.as_raw();
    let raw_window = window.window_handle().map_err(|e| e.to_string())?.as_raw();
    #[cfg(target_os = "macos")]
    let pref = DisplayApiPreference::Cgl;
    #[cfg(not(target_os = "macos"))]
    let pref = DisplayApiPreference::Egl;
    let display = unsafe { Display::new(raw_display, pref) }.map_err(|e| format!("OpenGL : {e}"))?;
    let template = ConfigTemplateBuilder::new().compatible_with_native_window(raw_window).build();
    let config = unsafe { display.find_configs(template) }
        .map_err(|e| format!("OpenGL : {e}"))?
        .reduce(|a, b| if b.num_samples() < a.num_samples() { b } else { a })
        .ok_or("OpenGL : aucune configuration")?;
    let attrs = ContextAttributesBuilder::new()
        .with_profile(GlProfile::Core)
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 2))))
        .build(Some(raw_window));
    let context = unsafe { display.create_context(&config, &attrs) }.map_err(|e| format!("OpenGL : {e}"))?;
    let (w, h) = (NonZeroU32::new(size.0.max(1)).unwrap(), NonZeroU32::new(size.1.max(1)).unwrap());
    let sattrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(raw_window, w, h);
    let surface = unsafe { display.create_window_surface(&config, &sattrs) }.map_err(|e| format!("OpenGL : {e}"))?;
    let native = match raw_display {
        RawDisplayHandle::Xlib(x) => x.display.map(|d| (mpv::RENDER_PARAM_X11_DISPLAY, d.as_ptr() as isize)),
        RawDisplayHandle::Wayland(wl) => Some((mpv::RENDER_PARAM_WL_DISPLAY, wl.display.as_ptr() as isize)),
        _ => None,
    };
    let parts = GlParts { display, context, surface, native };
    Screen::start(size, refresh_hz, decks, move |shared, links| {
        let parts = parts;
        unsafe { setup(shared, parts, links) }
    })
}

unsafe extern "C" fn get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let display = unsafe { &*(ctx as *const Display) };
    display.get_proc_address(unsafe { CStr::from_ptr(name) }) as *mut c_void
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

const VERT: &str = "#version 150 core
out vec2 uv;
void main() {
    vec2 t = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(t * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    uv = t;
}
";

const FRAG: &str = "#version 150 core
in vec2 uv;
out vec4 color;
uniform sampler2D tex0;
uniform sampler2D tex1;
uniform vec4 h0;
uniform vec4 h1;
uniform vec4 h2;
uniform vec4 misc;
void main() {
    vec2 p = uv;
    float w = h2.x * p.x + h2.y * p.y + h2.z;
    vec2 q = vec2(h0.x * p.x + h0.y * p.y + h0.z, h1.x * p.x + h1.y * p.y + h1.z) / w;
    if (w <= 0.0 || q.x < 0.0 || q.y < 0.0 || q.x > 1.0 || q.y > 1.0) {
        color = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }
    vec3 a = texture(tex0, q).rgb * misc.z;
    vec3 b = texture(tex1, q).rgb * misc.w;
    color = vec4(mix(a, b, misc.x) * misc.y, 1.0);
}
";

unsafe fn shader(kind: u32, src: &str) -> Result<u32, String> {
    unsafe {
        let s = gl::CreateShader(kind);
        let c = CString::new(src).unwrap();
        gl::ShaderSource(s, 1, &c.as_ptr(), std::ptr::null());
        gl::CompileShader(s);
        let mut ok = 0;
        gl::GetShaderiv(s, gl::COMPILE_STATUS, &mut ok);
        if ok == 0 {
            let mut buf = vec![0u8; 2048];
            let mut n = 0;
            gl::GetShaderInfoLog(s, buf.len() as i32, &mut n, buf.as_mut_ptr() as *mut c_char);
            return Err(format!("shader OpenGL : {}", String::from_utf8_lossy(&buf[..n.max(0) as usize])));
        }
        Ok(s)
    }
}

/// Texture RGBA et son framebuffer.
struct Fb {
    tex: u32,
    fbo: u32,
}

unsafe fn framebuffer(w: u32, h: u32) -> Fb {
    unsafe {
        let mut tex = 0;
        gl::GenTextures(1, &mut tex);
        gl::BindTexture(gl::TEXTURE_2D, tex);
        gl::TexImage2D(gl::TEXTURE_2D, 0, gl::RGBA8 as i32, w as i32, h as i32, 0, gl::RGBA, gl::UNSIGNED_BYTE, std::ptr::null());
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        let mut fbo = 0;
        gl::GenFramebuffers(1, &mut fbo);
        gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
        gl::FramebufferTexture2D(gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, tex, 0);
        gl::ClearColor(0.0, 0.0, 0.0, 1.0);
        gl::Clear(gl::COLOR_BUFFER_BIT);
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        Fb { tex, fbo }
    }
}

unsafe fn free_fb(f: &Fb) {
    unsafe {
        gl::DeleteFramebuffers(1, &f.fbo);
        gl::DeleteTextures(1, &f.tex);
    }
}

struct Gl {
    gpu: String,
    /// Gardé en boîte : mpv s'en sert pour trouver les fonctions OpenGL.
    _display: Box<Display>,
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    size: (u32, u32),
    targets: Vec<Fb>,
    program: u32,
    vao: u32,
    loc: [i32; 6],
    thumb: (Fb, u32, u32),
    /// Lectures de vignettes en cours : (tampon, attente GPU).
    pbos: Vec<(u32, Option<gl::types::GLsync>)>,
    slot: usize,
}

unsafe fn setup(shared: &Arc<Shared>, parts: GlParts, links: &[(Handle, RenderApi)]) -> Result<(Gl, Vec<DeckRc>), String> {
    unsafe {
        let GlParts { display, context, surface, native } = parts;
        let context = context.make_current(&surface).map_err(|e| format!("OpenGL : {e}"))?;
        // Synchro de l'écran : une image par rafraîchissement au plus.
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let display = Box::new(display);
        gl::load_with(|s| {
            let c = CString::new(s).unwrap();
            display.get_proc_address(&c) as *const c_void
        });
        let gpu = {
            let p = gl::GetString(gl::RENDERER);
            if p.is_null() { String::new() } else { CStr::from_ptr(p as *const c_char).to_string_lossy().into_owned() }
        };

        let api_type = CString::new("opengl").unwrap();
        let mut decks: Vec<DeckRc> = Vec::new();
        for (handle, api) in links {
            let mut init = OpenGlInitParams { get_proc_address, get_proc_address_ctx: &*display as *const Display as *mut c_void };
            let mut adv: c_int = 1;
            let mut params = vec![
                RenderParam { kind: mpv::RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
                RenderParam { kind: mpv::RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut OpenGlInitParams as *mut c_void },
                RenderParam { kind: mpv::RENDER_PARAM_ADVANCED_CONTROL, data: &mut adv as *mut c_int as *mut c_void },
            ];
            if let Some((kind, ptr)) = native {
                params.push(RenderParam { kind, data: ptr as *mut c_void });
            }
            params.push(RenderParam::END);
            let mut rc: Handle = std::ptr::null_mut();
            let r = (api.create)(&mut rc, *handle, params.as_mut_ptr());
            if r < 0 || rc.is_null() {
                for d in &decks {
                    (d.api.free)(d.rc);
                }
                return Err(format!("API de rendu OpenGL de mpv indisponible (code {r})"));
            }
            (api.set_update_callback)(rc, Some(on_update), Arc::as_ptr(shared) as *mut c_void);
            decks.push(DeckRc::new(rc, *handle, *api));
        }

        let vs = shader(gl::VERTEX_SHADER, VERT)?;
        let fs = shader(gl::FRAGMENT_SHADER, FRAG)?;
        let program = gl::CreateProgram();
        gl::AttachShader(program, vs);
        gl::AttachShader(program, fs);
        gl::BindFragDataLocation(program, 0, c"color".as_ptr());
        gl::LinkProgram(program);
        gl::DeleteShader(vs);
        gl::DeleteShader(fs);
        let mut ok = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut ok);
        if ok == 0 {
            return Err("programme OpenGL : édition de liens impossible".into());
        }
        let loc = ["tex0", "tex1", "h0", "h1", "h2", "misc"].map(|n| {
            let c = CString::new(n).unwrap();
            gl::GetUniformLocation(program, c.as_ptr())
        });
        let mut vao = 0;
        gl::GenVertexArrays(1, &mut vao);
        let size = shared.st.lock().map(|s| s.size).unwrap_or((1, 1));
        let mut g = Gl {
            gpu,
            _display: display,
            context,
            surface,
            size,
            targets: Vec::new(),
            program,
            vao,
            loc,
            thumb: (Fb { tex: 0, fbo: 0 }, 0, 0),
            pbos: Vec::new(),
            slot: 0,
        };
        g.make_targets(decks.len());
        Ok((g, decks))
    }
}

impl Gl {
    unsafe fn make_targets(&mut self, n: usize) {
        unsafe {
            for t in self.targets.drain(..) {
                free_fb(&t);
            }
            for _ in 0..n {
                self.targets.push(framebuffer(self.size.0, self.size.1));
            }
            if self.thumb.0.fbo != 0 {
                free_fb(&self.thumb.0);
            }
            let tw = THUMB_W.min(self.size.0).max(2);
            let th = ((self.size.1 as u64 * tw as u64 / self.size.0.max(1) as u64) as u32).max(2) & !1;
            self.thumb = (framebuffer(tw, th), tw, th);
            for (b, s) in self.pbos.drain(..) {
                if let Some(s) = s {
                    gl::DeleteSync(s);
                }
                gl::DeleteBuffers(1, &b);
            }
            for _ in 0..3 {
                let mut b = 0;
                gl::GenBuffers(1, &mut b);
                gl::BindBuffer(gl::PIXEL_PACK_BUFFER, b);
                gl::BufferData(gl::PIXEL_PACK_BUFFER, (tw * th * 4) as isize, std::ptr::null(), gl::STREAM_READ);
                self.pbos.push((b, None));
            }
            gl::BindBuffer(gl::PIXEL_PACK_BUFFER, 0);
            self.slot = 0;
        }
    }

    unsafe fn draw(&self, fbo: u32, w: u32, h: u32, p: &Params) {
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
            gl::Viewport(0, 0, w as i32, h as i32);
            gl::Disable(gl::BLEND);
            gl::Disable(gl::SCISSOR_TEST);
            gl::Disable(gl::DEPTH_TEST);
            gl::Disable(gl::CULL_FACE);
            gl::UseProgram(self.program);
            gl::BindVertexArray(self.vao);
            for (i, t) in self.targets.iter().enumerate().take(2) {
                gl::ActiveTexture(gl::TEXTURE0 + i as u32);
                gl::BindTexture(gl::TEXTURE_2D, t.tex);
                gl::Uniform1i(self.loc[i], i as i32);
            }
            for r in 0..3 {
                gl::Uniform4fv(self.loc[2 + r], 1, p.h[r].as_ptr());
            }
            gl::Uniform4fv(self.loc[5], 1, p.misc.as_ptr());
            gl::DrawArrays(gl::TRIANGLES, 0, 3);
            gl::BindVertexArray(0);
            gl::ActiveTexture(gl::TEXTURE0);
        }
    }
}

impl Backend for Gl {
    fn gpu(&self) -> String {
        self.gpu.clone()
    }

    fn resize(&mut self, size: (u32, u32)) -> Result<(), String> {
        let (Some(w), Some(h)) = (NonZeroU32::new(size.0), NonZeroU32::new(size.1)) else { return Ok(()) };
        self.surface.resize(&self.context, w, h);
        self.size = size;
        let n = self.targets.len();
        unsafe { self.make_targets(n) };
        Ok(())
    }

    fn draw_deck(&mut self, i: usize, d: &DeckRc) {
        let mut fbo = OpenGlFbo { fbo: self.targets[i].fbo as c_int, w: self.size.0 as c_int, h: self.size.1 as c_int, internal_format: 0 };
        let mut flip: c_int = 0;
        let mut block: c_int = 0;
        let mut rp = [
            RenderParam { kind: mpv::RENDER_PARAM_OPENGL_FBO, data: &mut fbo as *mut OpenGlFbo as *mut c_void },
            RenderParam { kind: mpv::RENDER_PARAM_FLIP_Y, data: &mut flip as *mut c_int as *mut c_void },
            RenderParam { kind: mpv::RENDER_PARAM_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut c_int as *mut c_void },
            RenderParam::END,
        ];
        unsafe { (d.api.render)(d.rc, rp.as_mut_ptr()) };
    }

    fn compose(&mut self, p: &Params) {
        unsafe { self.draw(0, self.size.0, self.size.1, p) };
    }

    fn compose_thumb(&mut self, p: &Params) {
        unsafe {
            let (fbo, w, h) = (self.thumb.0.fbo, self.thumb.1, self.thumb.2);
            self.draw(fbo, w, h, p);
            let slot = self.slot;
            if self.pbos[slot].1.is_some() {
                // Lecture précédente pas encore terminée : on saute cette vignette.
                return;
            }
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, fbo);
            gl::BindBuffer(gl::PIXEL_PACK_BUFFER, self.pbos[slot].0);
            gl::ReadPixels(0, 0, w as i32, h as i32, gl::RGBA, gl::UNSIGNED_BYTE, std::ptr::null_mut());
            gl::BindBuffer(gl::PIXEL_PACK_BUFFER, 0);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
            self.pbos[slot].1 = Some(gl::FenceSync(gl::SYNC_GPU_COMMANDS_COMPLETE, 0));
            self.slot = (slot + 1) % self.pbos.len();
        }
    }

    fn present(&mut self) {
        let _ = self.surface.swap_buffers(&self.context);
    }

    fn thumbs_pending(&self) -> bool {
        self.pbos.iter().any(|p| p.1.is_some())
    }

    fn read_thumbs(&mut self, feed: &mut MonitorFeed) {
        let (w, h) = (self.thumb.1 as usize, self.thumb.2 as usize);
        let n = self.pbos.len();
        for i in 0..n {
            let k = (self.slot + i) % n;
            let Some(sync) = self.pbos[k].1 else { continue };
            unsafe {
                let r = gl::ClientWaitSync(sync, 0, 0);
                if r != gl::ALREADY_SIGNALED && r != gl::CONDITION_SATISFIED {
                    continue;
                }
                gl::DeleteSync(sync);
                self.pbos[k].1 = None;
                gl::BindBuffer(gl::PIXEL_PACK_BUFFER, self.pbos[k].0);
                let p = gl::MapBufferRange(gl::PIXEL_PACK_BUFFER, 0, (w * h * 4) as isize, gl::MAP_READ_BIT) as *const u8;
                if !p.is_null() {
                    // Simple copie mémoire ici ; retournement (OpenGL lit de bas en haut) dans le fil d'envoi.
                    let data = std::slice::from_raw_parts(p, w * h * 4).to_vec();
                    gl::UnmapBuffer(gl::PIXEL_PACK_BUFFER);
                    feed.send_raw(w, h, w * 4, false, true, data);
                }
                gl::BindBuffer(gl::PIXEL_PACK_BUFFER, 0);
            }
        }
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        unsafe {
            for t in &self.targets {
                free_fb(t);
            }
            free_fb(&self.thumb.0);
            for (b, s) in &self.pbos {
                if let Some(s) = s {
                    gl::DeleteSync(*s);
                }
                gl::DeleteBuffers(1, b);
            }
            gl::DeleteVertexArrays(1, &self.vao);
            gl::DeleteProgram(self.program);
        }
    }
}
