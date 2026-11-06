//! Liaison minimale avec libmpv, chargée à l'exécution.
//!
//! Charger la bibliothèque au démarrage (plutôt qu'à l'édition de liens) permet de compiler
//! et de lancer CueStar sans mpv installé : la sortie affiche alors la mire et signale
//! l'absence de libmpv à la régie.

use libloading::Library;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::PathBuf;

type Handle = *mut c_void;

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;

pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_INT64: c_int = 4;
pub const FORMAT_DOUBLE: c_int = 5;

pub const END_FILE_EOF: c_int = 0;
pub const END_FILE_ERROR: c_int = 4;

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
    observe_property: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    wait_event: unsafe extern "C" fn(Handle, f64) -> *mut RawEvent,
    set_wakeup_callback: unsafe extern "C" fn(Handle, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    free: unsafe extern "C" fn(*mut c_void),
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
    let mut out = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        for n in names {
            out.push(dir.join(n));
            out.push(dir.join("mpv").join(n));
        }
        // En développement : `target/debug/` → racine du projet.
        if let Some(root) = dir.parent().and_then(|p| p.parent()) {
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
            observe_property: sym!("mpv_observe_property"),
            wait_event: sym!("mpv_wait_event"),
            set_wakeup_callback: sym!("mpv_set_wakeup_callback"),
            error_string: sym!("mpv_error_string"),
            free: sym!("mpv_free"),
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

    pub fn observe(&self, id: u64, name: &str, format: c_int) {
        let n = cstr(name);
        unsafe { (self.api.observe_property)(self.handle, id, n.as_ptr(), format) };
    }

    /// Prochain événement en attente (timeout 0 : ne bloque pas). None quand la file est vide.
    pub fn next_event(&self) -> Option<&RawEvent> {
        let ev = unsafe { &*(self.api.wait_event)(self.handle, 0.0) };
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
