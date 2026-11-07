//! Liaison minimale avec libmpv, chargée à l'exécution.
//!
//! Charger la bibliothèque au démarrage (plutôt qu'à l'édition de liens) permet de compiler
//! et de lancer CueStar sans mpv installé : la sortie affiche alors la mire et signale
//! l'absence de libmpv à la régie.

use libloading::Library;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::PathBuf;

pub type Handle = *mut c_void;

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_VIDEO_RECONFIG: c_int = 17;
pub const EVENT_PLAYBACK_RESTART: c_int = 21;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;

pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_INT64: c_int = 4;
pub const FORMAT_DOUBLE: c_int = 5;

pub const END_FILE_EOF: c_int = 0;
pub const END_FILE_ERROR: c_int = 4;

pub const FORMAT_NODE_MAP: c_int = 8;
pub const FORMAT_BYTE_ARRAY: c_int = 9;

#[repr(C)]
pub struct Node {
    /// Union C : chaîne, entier, double, liste ou tableau d'octets selon `format`.
    pub u: u64,
    pub format: c_int,
}

#[repr(C)]
struct NodeList {
    num: c_int,
    values: *mut Node,
    keys: *mut *mut c_char,
}

#[repr(C)]
struct ByteArray {
    data: *mut c_void,
    size: usize,
}

#[repr(C)]
pub struct RawEvent {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct RawProperty {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct RawEndFile {
    pub reason: c_int,
    pub error: c_int,
}

struct Api {
    client_api_version: unsafe extern "C" fn() -> u64,
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(Handle),
    set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    get_property_string: unsafe extern "C" fn(Handle, *const c_char) -> *mut c_char,
    command: unsafe extern "C" fn(Handle, *const *const c_char) -> c_int,
    command_ret: unsafe extern "C" fn(Handle, *const *const c_char, *mut Node) -> c_int,
    free_node_contents: unsafe extern "C" fn(*mut Node),
    observe_property: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    wait_event: unsafe extern "C" fn(Handle, f64) -> *mut RawEvent,
    set_wakeup_callback: unsafe extern "C" fn(Handle, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    free: unsafe extern "C" fn(*mut c_void),
    render: Option<RenderApi>,
}

/// Fonctions de l'API de rendu (`render.h`) : CueStar dessine lui-même l'image.
#[derive(Clone, Copy)]
pub struct RenderApi {
    pub create: unsafe extern "C" fn(*mut Handle, Handle, *mut RenderParam) -> c_int,
    pub set_update_callback: unsafe extern "C" fn(Handle, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    pub update: unsafe extern "C" fn(Handle) -> u64,
    pub render: unsafe extern "C" fn(Handle, *mut RenderParam) -> c_int,
    pub get_info: unsafe extern "C" fn(Handle, RenderParam) -> c_int,
    pub report_swap: unsafe extern "C" fn(Handle),
    pub free: unsafe extern "C" fn(Handle),
    /// Horloge de mpv (µs), celle des heures d'affichage des images.
    pub time_us: unsafe extern "C" fn(Handle) -> i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RenderParam {
    pub kind: c_int,
    pub data: *mut c_void,
}

impl RenderParam {
    pub const END: RenderParam = RenderParam { kind: 0, data: std::ptr::null_mut() };
}

pub const RENDER_PARAM_API_TYPE: c_int = 1;
#[allow(dead_code)]
pub const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
#[allow(dead_code)]
pub const RENDER_PARAM_OPENGL_FBO: c_int = 3;
#[allow(dead_code)]
pub const RENDER_PARAM_FLIP_Y: c_int = 4;
#[allow(dead_code)]
pub const RENDER_PARAM_X11_DISPLAY: c_int = 8;
#[allow(dead_code)]
pub const RENDER_PARAM_WL_DISPLAY: c_int = 9;
pub const RENDER_PARAM_ADVANCED_CONTROL: c_int = 10;
pub const RENDER_PARAM_NEXT_FRAME_INFO: c_int = 11;
pub const RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
#[allow(dead_code)]
pub const RENDER_PARAM_D3D11_INIT_PARAMS: c_int = 21;
#[allow(dead_code)]
pub const RENDER_PARAM_D3D11_FBO: c_int = 22;
pub const RENDER_UPDATE_FRAME: u64 = 1;
pub const FRAME_INFO_PRESENT: u64 = 1;

#[repr(C)]
#[derive(Default)]
pub struct FrameInfo {
    pub flags: u64,
    /// Heure d'affichage voulue, en microsecondes (horloge `mpv_get_time_us`).
    pub target_time: i64,
}

pub struct Mpv {
    api: Api,
    handle: Handle,
    _lib: Library,
}

// Le handle mpv est utilisable depuis n'importe quel thread (API client thread-safe).
unsafe impl Send for Mpv {}

/// Emplacements essayés, dans l'ordre : à côté de l'exécutable, dans `mpv/` à côté, puis le système.
fn candidates() -> Vec<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["libmpv-2.dll", "mpv-2.dll", "mpv-1.dll"]
    } else if cfg!(target_os = "macos") {
        &["libmpv.2.dylib", "libmpv.dylib"]
    } else {
        &["libmpv.so.2", "libmpv.so.1", "libmpv.so"]
    };
    library_candidates(names)
}

/// Emplacements essayés pour une bibliothèque livrée avec libmpv (même ordre que libmpv).
pub fn library_candidates(names: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        for n in names {
            out.push(dir.join(n));
            out.push(dir.join("mpv").join(n));
        }
        // En développement : dossier `mpv/` à la racine du projet, quelques niveaux au-dessus de `target/…`.
        for root in dir.ancestors().skip(1).take(4) {
            for n in names {
                out.push(root.join("mpv").join(n));
            }
        }
    }
    if cfg!(target_os = "macos") {
        for prefix in ["/opt/homebrew/lib", "/usr/local/lib"] {
            for n in names {
                out.push(PathBuf::from(prefix).join(n));
            }
        }
    }
    for n in names {
        out.push(PathBuf::from(n));
    }
    out
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

impl Mpv {
    /// Charge libmpv et crée une instance (pas encore initialisée).
    pub fn create() -> Result<Self, String> {
        let mut tried = Vec::new();
        let mut lib = None;
        for p in candidates() {
            match unsafe { Library::new(&p) } {
                Ok(l) => {
                    lib = Some(l);
                    break;
                }
                Err(_) => tried.push(p.display().to_string()),
            }
        }
        let Some(lib) = lib else {
            return Err(format!("libmpv introuvable (essayé : {})", tried.join(", ")));
        };
        macro_rules! sym {
            ($name:literal) => {
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
            };
        }
        let api = Api {
            client_api_version: sym!("mpv_client_api_version"),
            create: sym!("mpv_create"),
            initialize: sym!("mpv_initialize"),
            terminate_destroy: sym!("mpv_terminate_destroy"),
            set_option_string: sym!("mpv_set_option_string"),
            set_property_string: sym!("mpv_set_property_string"),
            get_property_string: sym!("mpv_get_property_string"),
            command: sym!("mpv_command"),
            command_ret: sym!("mpv_command_ret"),
            free_node_contents: sym!("mpv_free_node_contents"),
            observe_property: sym!("mpv_observe_property"),
            wait_event: sym!("mpv_wait_event"),
            set_wakeup_callback: sym!("mpv_set_wakeup_callback"),
            error_string: sym!("mpv_error_string"),
            free: sym!("mpv_free"),
            render: (|| -> Result<RenderApi, String> {
                Ok(RenderApi {
                    create: sym!("mpv_render_context_create"),
                    set_update_callback: sym!("mpv_render_context_set_update_callback"),
                    update: sym!("mpv_render_context_update"),
                    render: sym!("mpv_render_context_render"),
                    get_info: sym!("mpv_render_context_get_info"),
                    report_swap: sym!("mpv_render_context_report_swap"),
                    free: sym!("mpv_render_context_free"),
                    time_us: sym!("mpv_get_time_us"),
                })
            })()
            .ok(),
        };
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("mpv_create a échoué".into());
        }
        Ok(Self { api, handle, _lib: lib })
    }

    fn check(&self, code: c_int, what: &str) -> Result<(), String> {
        if code >= 0 {
            Ok(())
        } else {
            let msg = unsafe { CStr::from_ptr((self.api.error_string)(code)) }.to_string_lossy().into_owned();
            Err(format!("{what} : {msg}"))
        }
    }

    /// Fonctions de l'API de rendu, si la libmpv chargée les propose.
    pub fn render_api(&self) -> Option<RenderApi> {
        self.api.render
    }

    pub fn raw(&self) -> Handle {
        self.handle
    }

    pub fn api_version(&self) -> String {
        let v = unsafe { (self.api.client_api_version)() };
        format!("{}.{}", v >> 16, v & 0xffff)
    }

    pub fn set_option(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (cstr(name), cstr(value));
        self.check(unsafe { (self.api.set_option_string)(self.handle, n.as_ptr(), v.as_ptr()) }, name)
    }

    pub fn initialize(&self) -> Result<(), String> {
        self.check(unsafe { (self.api.initialize)(self.handle) }, "initialisation de mpv")
    }

    pub fn set_property(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (cstr(name), cstr(value));
        self.check(unsafe { (self.api.set_property_string)(self.handle, n.as_ptr(), v.as_ptr()) }, name)
    }

    pub fn get_property(&self, name: &str) -> Option<String> {
        let n = cstr(name);
        let p = unsafe { (self.api.get_property_string)(self.handle, n.as_ptr()) };
        if p.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        unsafe { (self.api.free)(p as *mut c_void) };
        Some(s)
    }

    pub fn command(&self, args: &[&str]) -> Result<(), String> {
        let owned: Vec<CString> = args.iter().map(|a| cstr(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        self.check(unsafe { (self.api.command)(self.handle, ptrs.as_ptr()) }, args.first().copied().unwrap_or("commande"))
    }

    /// Copie RGBA de l'image vidéo en cours (`screenshot-raw`) : (largeur, hauteur, pas, pixels).
    pub fn screenshot_rgba(&self) -> Option<(usize, usize, usize, Vec<u8>)> {
        let owned: Vec<CString> = ["screenshot-raw", "video", "rgba"].iter().map(|a| cstr(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let mut node = Node { u: 0, format: 0 };
        if unsafe { (self.api.command_ret)(self.handle, ptrs.as_ptr(), &mut node) } < 0 {
            return None;
        }
        let mut out = None;
        if node.format == FORMAT_NODE_MAP && node.u != 0 {
            let list = unsafe { &*(node.u as *const NodeList) };
            let (mut w, mut h, mut stride, mut data) = (0i64, 0i64, 0i64, None);
            for i in 0..list.num.max(0) as usize {
                let key = unsafe { CStr::from_ptr(*list.keys.add(i)) }.to_bytes();
                let v = unsafe { &*list.values.add(i) };
                match key {
                    b"w" if v.format == FORMAT_INT64 => w = v.u as i64,
                    b"h" if v.format == FORMAT_INT64 => h = v.u as i64,
                    b"stride" if v.format == FORMAT_INT64 => stride = v.u as i64,
                    b"data" if v.format == FORMAT_BYTE_ARRAY && v.u != 0 => {
                        let ba = unsafe { &*(v.u as *const ByteArray) };
                        data = Some(unsafe { std::slice::from_raw_parts(ba.data as *const u8, ba.size) }.to_vec());
                    }
                    _ => {}
                }
            }
            if let Some(d) = data {
                if w > 0 && h > 0 && stride >= w * 4 && d.len() >= (stride * h) as usize {
                    out = Some((w as usize, h as usize, stride as usize, d));
                }
            }
        }
        unsafe { (self.api.free_node_contents)(&mut node) };
        out
    }

    pub fn observe(&self, id: u64, name: &str, format: c_int) {
        let n = cstr(name);
        unsafe { (self.api.observe_property)(self.handle, id, n.as_ptr(), format) };
    }

    /// Prochain événement en attente (timeout 0 : ne bloque pas). None quand la file est vide.
    pub fn next_event(&self) -> Option<&RawEvent> {
        let ev = unsafe { &*(self.api.wait_event)(self.handle, 0.0) };
        (ev.event_id != EVENT_NONE).then_some(ev)
    }

    /// Attend un événement au plus `timeout` secondes.
    pub fn wait_event(&self, timeout: f64) -> Option<&RawEvent> {
        let ev = unsafe { &*(self.api.wait_event)(self.handle, timeout) };
        (ev.event_id != EVENT_NONE).then_some(ev)
    }

    /// Appelée par mpv (depuis un de ses threads) quand des événements arrivent.
    pub fn set_wakeup(&self, cb: unsafe extern "C" fn(*mut c_void), data: *mut c_void) {
        unsafe { (self.api.set_wakeup_callback)(self.handle, Some(cb), data) };
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { (self.api.terminate_destroy)(self.handle) };
    }
}
