//! « Adapter la fréquence de l'écran » : passe l'écran de sortie sur une fréquence qui tombe
//! juste pour la vidéo (50 Hz pour du 25, 60 Hz pour du 30…), et rétablit la fréquence
//! d'origine en quittant. Windows (GDI), macOS (CoreGraphics) et Linux sous X11 (xrandr) ;
//! sous Wayland, le système ne laisse pas une application changer la fréquence.

/// Fréquence la plus adaptée à `fps` parmi `modes` : un multiple entier (à 0,5 % près),
/// la plus haute possible. None si aucune ne tombe juste.
pub fn best(fps: f64, modes: &[f64]) -> Option<f64> {
    if fps <= 1.0 {
        return None;
    }
    modes
        .iter()
        .copied()
        .filter(|hz| {
            let r = hz / fps;
            r >= 0.99 && ((r - r.round()).abs() / r) < 0.005
        })
        .fold(None, |best: Option<f64>, hz| Some(best.map_or(hz, |b| b.max(hz))))
}

/// Chaque image tient un nombre entier de rafraîchissements, sans recalage (30 i/s sur 60 Hz).
pub fn exact(fps: f64, hz: f64) -> bool {
    if fps <= 1.0 || hz <= 1.0 {
        return false;
    }
    let r = hz / fps;
    r >= 0.999 && ((r - r.round()).abs() / r) < 0.0005
}

/// L'écran tombe-t-il juste pour cette cadence (à 1 % près, rattrapable par recalage de vitesse) ?
pub fn fits(fps: f64, hz: f64) -> bool {
    if fps <= 1.0 || hz <= 1.0 {
        return false;
    }
    let r = hz / fps;
    r >= 0.99 && ((r - r.round()).abs() / r) < 0.01
}

#[cfg(windows)]
mod imp {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_FULLSCREEN, DEVMODEW, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn mode(dev: &[u16], i: ENUM_DISPLAY_SETTINGS_MODE) -> Option<DEVMODEW> {
        let mut m = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        unsafe { EnumDisplaySettingsW(PCWSTR(dev.as_ptr()), i, &mut m) }.as_bool().then_some(m)
    }

    /// Fréquences proposées par l'écran `device` (ex. `\\.\DISPLAY1`) à sa résolution actuelle.
    pub fn modes(device: &str) -> Vec<f64> {
        let dev = wide(device);
        let Some(cur) = mode(&dev, ENUM_CURRENT_SETTINGS) else { return Vec::new() };
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(m) = mode(&dev, ENUM_DISPLAY_SETTINGS_MODE(i)) {
            if m.dmPelsWidth == cur.dmPelsWidth && m.dmPelsHeight == cur.dmPelsHeight && m.dmBitsPerPel == cur.dmBitsPerPel {
                let hz = m.dmDisplayFrequency as f64;
                if hz > 1.0 && !out.contains(&hz) {
                    out.push(hz);
                }
            }
            i += 1;
        }
        out
    }

    pub fn current(device: &str) -> Option<f64> {
        mode(&wide(device), ENUM_CURRENT_SETTINGS).map(|m| m.dmDisplayFrequency as f64)
    }

    /// Change la fréquence (mode temporaire : Windows le rétablit aussi si CueStar s'arrête net).
    pub fn set(device: &str, hz: f64) -> Result<(), String> {
        let dev = wide(device);
        let mut m = mode(&dev, ENUM_CURRENT_SETTINGS).ok_or("écran introuvable")?;
        m.dmDisplayFrequency = hz.round() as u32;
        m.dmFields = DM_DISPLAYFREQUENCY;
        let r = unsafe { ChangeDisplaySettingsExW(PCWSTR(dev.as_ptr()), Some(&m), None, CDS_FULLSCREEN, None) };
        if r == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(format!("changement de fréquence refusé ({})", r.0))
        }
    }

    /// Revient au réglage enregistré de l'écran.
    pub fn restore(device: &str) {
        let dev = wide(device);
        let _ = unsafe { ChangeDisplaySettingsExW(PCWSTR(dev.as_ptr()), None, None, Default::default(), None) };
    }
}

/// Identifiant de l'écran pour ce module : nom système sous Windows (`\\.\DISPLAY1`) et
/// X11 (sortie RandR, `HDMI-1`), numéro CGDirectDisplayID sous macOS.
pub fn device_id(m: &winit::monitor::MonitorHandle) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::MonitorHandleExtMacOS;
        Some(m.native_id().to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        m.name()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;

    type DisplayId = u32;
    type ModeRef = *mut c_void;
    type ArrayRef = *const c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDisplayCopyAllDisplayModes(display: DisplayId, options: *const c_void) -> ArrayRef;
        fn CGDisplayCopyDisplayMode(display: DisplayId) -> ModeRef;
        fn CGDisplayModeRelease(mode: ModeRef);
        fn CGDisplayModeGetWidth(mode: ModeRef) -> usize;
        fn CGDisplayModeGetHeight(mode: ModeRef) -> usize;
        fn CGDisplayModeGetPixelWidth(mode: ModeRef) -> usize;
        fn CGDisplayModeGetPixelHeight(mode: ModeRef) -> usize;
        fn CGDisplayModeGetRefreshRate(mode: ModeRef) -> f64;
        fn CGDisplaySetDisplayMode(display: DisplayId, mode: ModeRef, options: *const c_void) -> i32;
        fn CGRestorePermanentDisplayConfiguration();
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: ArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: ArrayRef, i: isize) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    fn id(device: &str) -> Option<DisplayId> {
        device.parse().ok()
    }

    type Size = (usize, usize, usize, usize);

    unsafe fn size(m: ModeRef) -> Size {
        (CGDisplayModeGetWidth(m), CGDisplayModeGetHeight(m), CGDisplayModeGetPixelWidth(m), CGDisplayModeGetPixelHeight(m))
    }

    /// Modes de l'écran à la définition actuelle : appelle `f` sur chacun avec sa fréquence.
    fn each_mode(d: DisplayId, mut f: impl FnMut(ModeRef, f64) -> bool) {
        unsafe {
            let cur = CGDisplayCopyDisplayMode(d);
            if cur.is_null() {
                return;
            }
            let want = size(cur);
            CGDisplayModeRelease(cur);
            let all = CGDisplayCopyAllDisplayModes(d, std::ptr::null());
            if all.is_null() {
                return;
            }
            for i in 0..CFArrayGetCount(all) {
                let m = CFArrayGetValueAtIndex(all, i) as ModeRef;
                let hz = CGDisplayModeGetRefreshRate(m);
                if hz > 1.0 && size(m) == want && !f(m, hz) {
                    break;
                }
            }
            CFRelease(all);
        }
    }

    pub fn modes(device: &str) -> Vec<f64> {
        let mut out = Vec::new();
        if let Some(d) = id(device) {
            each_mode(d, |_, hz| {
                if !out.iter().any(|h: &f64| (h - hz).abs() < 0.01) {
                    out.push(hz);
                }
                true
            });
        }
        out
    }

    /// Fréquence actuelle ; None pour un écran qui n'en annonce pas (écran intégré).
    pub fn current(device: &str) -> Option<f64> {
        let d = id(device)?;
        unsafe {
            let m = CGDisplayCopyDisplayMode(d);
            if m.is_null() {
                return None;
            }
            let hz = CGDisplayModeGetRefreshRate(m);
            CGDisplayModeRelease(m);
            (hz > 1.0).then_some(hz)
        }
    }

    pub fn set(device: &str, hz: f64) -> Result<(), String> {
        let d = id(device).ok_or("écran introuvable")?;
        let mut err = Some("l'écran ne propose pas cette fréquence".to_string());
        each_mode(d, |m, rate| {
            if (rate - hz).abs() < 0.5 {
                let r = unsafe { CGDisplaySetDisplayMode(d, m, std::ptr::null()) };
                err = (r != 0).then(|| format!("changement de fréquence refusé ({r})"));
                return false;
            }
            true
        });
        err.map_or(Ok(()), Err)
    }

    /// Revient au réglage enregistré des écrans.
    pub fn restore(_device: &str) {
        unsafe { CGRestorePermanentDisplayConfiguration() };
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::Mutex;

    /// Réglage d'origine (définition, fréquence) de chaque sortie changée, pour le retour.
    static ORIGINAL: Mutex<Option<HashMap<String, (String, f64)>>> = Mutex::new(None);

    pub const WAYLAND: &str = "sous Wayland, CueStar ne peut pas changer la fréquence de l'écran : \
                               réglez-la dans les paramètres d'affichage du système";

    fn wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
    }

    /// Définition actuelle de la sortie et ses fréquences (actuelle en premier), lues dans
    /// `xrandr --query`.
    fn query(output: &str) -> Result<(String, Vec<f64>, Option<f64>), String> {
        let out = Command::new("xrandr").arg("--query").output().map_err(|_| "xrandr introuvable (paquet x11-xserver-utils)".to_string())?;
        if !out.status.success() {
            return Err("xrandr : pas d'affichage X11".into());
        }
        Ok(parse(&String::from_utf8_lossy(&out.stdout), output).ok_or_else(|| format!("sortie {output} introuvable dans xrandr"))?)
    }

    /// Lignes de mode : « 1920x1080     60.00*+  59.94    50.00 » ; `*` = fréquence en cours.
    pub(super) fn parse(text: &str, output: &str) -> Option<(String, Vec<f64>, Option<f64>)> {
        let mut lines = text.lines().skip_while(|l| !(l.starts_with(output) && l[output.len()..].starts_with(' ')));
        lines.next()?;
        for l in lines.take_while(|l| l.starts_with(' ')) {
            let mut words = l.split_whitespace();
            let Some(mode) = words.next() else { continue };
            if !l.contains('*') {
                continue;
            }
            let mut rates = Vec::new();
            let mut current = None;
            for w in words {
                let Ok(hz) = w.trim_end_matches(['*', '+']).parse::<f64>() else { continue };
                if w.contains('*') {
                    current = Some(hz);
                }
                rates.push(hz);
            }
            return Some((mode.to_string(), rates, current));
        }
        None
    }

    pub fn modes(output: &str) -> Vec<f64> {
        if wayland() {
            return Vec::new();
        }
        query(output).map(|q| q.1).unwrap_or_default()
    }

    pub fn current(output: &str) -> Option<f64> {
        if wayland() {
            return None;
        }
        query(output).ok()?.2
    }

    fn xrandr(output: &str, mode: &str, hz: f64) -> Result<(), String> {
        let st = Command::new("xrandr")
            .args(["--output", output, "--mode", mode, "--rate", &format!("{hz:.2}")])
            .status()
            .map_err(|_| "xrandr introuvable".to_string())?;
        st.success().then_some(()).ok_or_else(|| "changement de fréquence refusé par xrandr".into())
    }

    pub fn set(output: &str, hz: f64) -> Result<(), String> {
        if wayland() {
            return Err(WAYLAND.into());
        }
        let (mode, rates, current) = query(output)?;
        let rate = rates.iter().copied().filter(|r| (r - hz).abs() < 0.5).min_by(|a, b| (a - hz).abs().total_cmp(&(b - hz).abs())).ok_or("l'écran ne propose pas cette fréquence")?;
        xrandr(output, &mode, rate)?;
        let mut orig = ORIGINAL.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cur) = current {
            orig.get_or_insert_with(HashMap::new).entry(output.to_string()).or_insert((mode, cur));
        }
        Ok(())
    }

    pub fn restore(output: &str) {
        let back = ORIGINAL.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(output));
        if let Some((mode, hz)) = back {
            let _ = xrandr(output, &mode, hz);
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn modes(_device: &str) -> Vec<f64> {
        Vec::new()
    }
    pub fn current(_device: &str) -> Option<f64> {
        None
    }
    pub fn set(_device: &str, _hz: f64) -> Result<(), String> {
        Err("changement de fréquence pas disponible sur ce système".into())
    }
    pub fn restore(_device: &str) {}
}

pub use imp::{current, modes, restore, set};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choix() {
        assert_eq!(best(25.0, &[60.0, 144.0, 50.0]), Some(50.0));
        assert_eq!(best(23.976, &[60.0, 144.0]), Some(144.0));
        assert_eq!(best(30.0, &[60.0, 144.0, 120.0]), Some(120.0));
        assert_eq!(best(25.0, &[60.0, 144.0]), None);
        assert!(fits(23.976, 144.0));
        assert!(!fits(25.0, 144.0));
        assert!(fits(29.97, 60.0));
        assert!(exact(30.0, 60.0));
        assert!(exact(25.0, 50.0));
        assert!(!exact(29.97, 60.0));
        assert!(!exact(23.976, 24.0));
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests_xrandr {
    #[test]
    fn parse_xrandr() {
        let text = "Screen 0: minimum 8 x 8, current 3840 x 1080, maximum 32767 x 32767
eDP-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 309mm x 174mm
   1920x1080     60.02*+  48.00
HDMI-1 connected 1920x1080+1920+0 (normal left inverted right x axis y axis) 527mm x 296mm
   3840x2160     30.00    25.00
   1920x1080     60.00    59.94*   50.00    24.00
   1280x720      60.00
HDMI-10 disconnected (normal left inverted right x axis y axis)
";
        let (mode, rates, cur) = super::imp::parse(text, "HDMI-1").unwrap();
        assert_eq!(mode, "1920x1080");
        assert_eq!(rates, vec![60.0, 59.94, 50.0, 24.0]);
        assert_eq!(cur, Some(59.94));
        assert!(super::imp::parse(text, "HDMI-10").is_none());
    }
}
