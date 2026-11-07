//! « Adapter la fréquence de l'écran » : passe l'écran de sortie sur une fréquence qui tombe
//! juste pour la vidéo (50 Hz pour du 25, 60 Hz pour du 30…), et rétablit la fréquence
//! d'origine en quittant. Windows seulement pour l'instant.

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

#[cfg(not(windows))]
mod imp {
    pub fn modes(_device: &str) -> Vec<f64> {
        Vec::new()
    }
    pub fn current(_device: &str) -> Option<f64> {
        None
    }
    pub fn set(_device: &str, _hz: f64) -> Result<(), String> {
        Err("changement de fréquence pas encore disponible sur ce système".into())
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
