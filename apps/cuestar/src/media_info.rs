//! Informations détaillées sur un fichier (clic droit « Informations sur le fichier… »).
//!
//! La lecture tourne dans un processus à part (`--info <média> [complet]`), comme les vignettes :
//! un fichier abîmé qui fait planter l'analyseur ne touche pas la régie. Le processus utilise
//! la bibliothèque MediaInfo (MediaInfo.dll, chargée à l'exécution, cherchée aux mêmes endroits
//! que libmpv) et, faute de mieux, ce que libmpv sait du fichier. Il écrit un rapport JSON.

use crate::output::mpv::{self, Mpv};
use libloading::Library;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

/// Une partie du rapport (Général, Vidéo, Audio…) et ses lignes « nom : valeur ».
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// D'où viennent les informations (« MediaInfoLib - v24.01 » ou « libmpv »).
    pub source: String,
    pub sections: Vec<Section>,
}

impl Report {
    /// Texte du rapport, présenté comme celui de MediaInfo (pour le presse-papiers).
    pub fn to_text(&self) -> String {
        let width = self.sections.iter().flat_map(|s| s.fields.iter().map(|(k, _)| k.chars().count())).max().unwrap_or(0).max(32);
        let mut out = String::new();
        for s in &self.sections {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&s.title);
            out.push('\n');
            for (k, v) in &s.fields {
                out.push_str(&format!("{k:<width$} : {v}\n"));
            }
        }
        out
    }
}

/// Découpe la sortie texte de MediaInfo : un titre seul sur sa ligne ouvre une partie,
/// puis des lignes « nom   : valeur », une ligne vide entre deux parties.
pub fn parse_inform(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        match line.split_once(" : ") {
            Some((k, v)) if !out.is_empty() => out.last_mut().unwrap().fields.push((k.trim().to_string(), v.trim().to_string())),
            _ if !line.contains(" : ") => out.push(Section { title: line.trim().to_string(), fields: Vec::new() }),
            _ => {}
        }
    }
    out.retain(|s| !s.fields.is_empty());
    out
}

// ---- MediaInfo ----

/// `wchar_t` du C : UTF-16 sous Windows, UTF-32 ailleurs.
#[cfg(windows)]
type WChar = u16;
#[cfg(not(windows))]
type WChar = u32;

fn wide(s: &str) -> Vec<WChar> {
    #[cfg(windows)]
    let mut v: Vec<WChar> = s.encode_utf16().collect();
    #[cfg(not(windows))]
    let mut v: Vec<WChar> = s.chars().map(|c| c as u32).collect();
    v.push(0);
    v
}

/// Chaîne `wchar_t*` terminée par zéro, prêtée par MediaInfo.
unsafe fn from_wide(p: *const WChar) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    let s = unsafe { std::slice::from_raw_parts(p, len) };
    #[cfg(windows)]
    return String::from_utf16_lossy(s);
    #[cfg(not(windows))]
    return s.iter().map(|&c| char::from_u32(c).unwrap_or('\u{fffd}')).collect();
}

fn mediainfo_candidates() -> Vec<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["MediaInfo.dll"]
    } else if cfg!(target_os = "macos") {
        &["libmediainfo.0.dylib", "libmediainfo.dylib"]
    } else {
        &["libmediainfo.so.0", "libmediainfo.so"]
    };
    mpv::library_candidates(names)
}

/// Rapport de MediaInfo, ou None si la bibliothèque manque ou ne lit pas le fichier.
fn with_mediainfo(path: &Path, full: bool) -> Option<Report> {
    type New = unsafe extern "C" fn() -> *mut c_void;
    type Delete = unsafe extern "C" fn(*mut c_void);
    type Open = unsafe extern "C" fn(*mut c_void, *const WChar) -> usize;
    type Opt = unsafe extern "C" fn(*mut c_void, *const WChar, *const WChar) -> *const WChar;
    type Inform = unsafe extern "C" fn(*mut c_void, usize) -> *const WChar;
    let lib = mediainfo_candidates().into_iter().find_map(|p| unsafe { Library::new(&p) }.ok())?;
    unsafe {
        let new: New = *lib.get(b"MediaInfo_New\0").ok()?;
        let delete: Delete = *lib.get(b"MediaInfo_Delete\0").ok()?;
        let open: Open = *lib.get(b"MediaInfo_Open\0").ok()?;
        let option: Opt = *lib.get(b"MediaInfo_Option\0").ok()?;
        let inform: Inform = *lib.get(b"MediaInfo_Inform\0").ok()?;
        let close: Delete = *lib.get(b"MediaInfo_Close\0").ok()?;
        let h = new();
        if h.is_null() {
            return None;
        }
        let version = from_wide(option(h, wide("Info_Version").as_ptr(), wide("").as_ptr()));
        option(h, wide("Complete").as_ptr(), wide(if full { "1" } else { "" }).as_ptr());
        let opened = open(h, wide(&path.to_string_lossy()).as_ptr());
        let text = if opened != 0 { from_wide(inform(h, 0)) } else { String::new() };
        close(h);
        delete(h);
        let sections = parse_inform(&text);
        if sections.is_empty() {
            return None;
        }
        let source = if version.is_empty() { "MediaInfo".to_string() } else { version };
        Some(Report { source, sections })
    }
}

// ---- Repli : ce que libmpv sait du fichier ----

fn size_text(bytes: u64) -> String {
    let units = ["o", "Kio", "Mio", "Gio", "Tio"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} o")
    } else {
        format!("{v:.2} {} ({bytes} o)", units[u])
    }
}

fn duration_text(secs: f64) -> String {
    let ms = (secs * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn with_mpv(path: &Path) -> Result<Report, String> {
    let m = Mpv::create()?;
    for (k, v) in [("terminal", "no"), ("vo", "null"), ("ao", "null"), ("pause", "yes"), ("hwdec", "no"), ("image-display-duration", "inf"), ("sid", "auto")] {
        let _ = m.set_option(k, v);
    }
    m.initialize()?;
    m.command(&["loadfile", &path.to_string_lossy()])?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut loaded = false;
    while Instant::now() < deadline && !loaded {
        match m.wait_event(0.5).map(|e| e.event_id) {
            Some(mpv::EVENT_FILE_LOADED) => loaded = true,
            Some(mpv::EVENT_END_FILE) | Some(mpv::EVENT_SHUTDOWN) => return Err("fichier illisible".into()),
            _ => {}
        }
    }
    if !loaded {
        return Err("délai dépassé".into());
    }
    // Les paramètres décodés (format des pixels, couleurs) arrivent avec la première image.
    let has_video = m.get_property("vid").is_some_and(|v| v != "no");
    let until = Instant::now() + Duration::from_secs(2);
    while has_video && m.get_property("video-params/pixelformat").is_none() && Instant::now() < until {
        m.wait_event(0.1);
    }
    let get = |p: &str| m.get_property(p).filter(|v| !v.is_empty());
    let mut general = Section { title: "Général".into(), fields: Vec::new() };
    let push = |s: &mut Section, k: &str, v: Option<String>| {
        if let Some(v) = v {
            s.fields.push((k.to_string(), v));
        }
    };
    push(&mut general, "Fichier", Some(path.display().to_string()));
    push(&mut general, "Format", get("file-format"));
    push(&mut general, "Taille", std::fs::metadata(path).ok().map(|md| size_text(md.len())));
    push(&mut general, "Durée", get("duration").and_then(|d| d.parse::<f64>().ok()).map(duration_text));
    push(&mut general, "Chapitres", get("chapters").filter(|c| c != "0"));
    let meta = get("metadata/list/count").and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
    for i in 0..meta {
        if let (Some(k), Some(v)) = (get(&format!("metadata/list/{i}/key")), get(&format!("metadata/list/{i}/value"))) {
            general.fields.push((k, v));
        }
    }
    let mut sections = vec![general];
    let tracks = get("track-list/count").and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
    for i in 0..tracks {
        let t = |k: &str| get(&format!("track-list/{i}/{k}"));
        let kind = t("type").unwrap_or_default();
        let title = match kind.as_str() {
            "video" if t("image").as_deref() == Some("yes") => "Image",
            "video" => "Vidéo",
            "audio" => "Audio",
            "sub" => "Texte",
            _ => "Piste",
        };
        let mut s = Section { title: format!("{title} #{}", t("id").unwrap_or_default()), fields: Vec::new() };
        push(&mut s, "Codec", t("codec"));
        push(&mut s, "Description", t("codec-desc"));
        push(&mut s, "Profil", t("codec-profile"));
        push(&mut s, "Titre", t("title"));
        push(&mut s, "Langue", t("lang"));
        push(&mut s, "Par défaut", t("default"));
        push(&mut s, "Externe", t("external").filter(|v| v == "yes"));
        push(&mut s, "Largeur", t("demux-w").map(|v| format!("{v} pixels")));
        push(&mut s, "Hauteur", t("demux-h").map(|v| format!("{v} pixels")));
        push(&mut s, "Images par seconde", t("demux-fps"));
        push(&mut s, "Rapport des pixels", t("demux-par"));
        push(&mut s, "Rotation", t("demux-rotation").filter(|v| v != "0"));
        push(&mut s, "Canaux", t("demux-channel-count"));
        push(&mut s, "Disposition des canaux", t("demux-channels"));
        push(&mut s, "Échantillonnage", t("demux-samplerate").map(|v| format!("{v} Hz")));
        push(&mut s, "Débit", t("demux-bitrate").and_then(|v| v.parse::<f64>().ok()).map(|b| format!("{:.0} kb/s", b / 1000.0)));
        if kind == "video" && t("selected").as_deref() == Some("yes") {
            for (k, p) in [
                ("Format des pixels", "video-params/pixelformat"),
                ("Profondeur", "video-params/average-bpp"),
                ("Matrice de couleurs", "video-params/colormatrix"),
                ("Niveaux", "video-params/colorlevels"),
                ("Primaires", "video-params/primaries"),
                ("Courbe de transfert", "video-params/gamma"),
                ("Sous-échantillonnage", "video-params/chroma-location"),
            ] {
                push(&mut s, k, get(p));
            }
        }
        if kind == "audio" && t("selected").as_deref() == Some("yes") {
            push(&mut s, "Format des échantillons", get("audio-params/format"));
        }
        sections.push(s);
    }
    Ok(Report { source: format!("libmpv {}", get("mpv-version").unwrap_or_default()).trim().to_string(), sections })
}

/// Mode `--info` : écrit le rapport JSON (ou `{"error": …}`) sur la sortie standard.
pub fn run(path: &str, full: bool) {
    let path = Path::new(path);
    let result = match with_mediainfo(path, full) {
        Some(r) => Ok(r),
        None => with_mpv(path),
    };
    let json = match result {
        Ok(r) => serde_json::to_string(&r),
        Err(e) => serde_json::to_string(&serde_json::json!({ "error": e })),
    };
    if let Ok(j) = json {
        println!("{j}");
    }
}

/// Lance l'analyse en tâche de fond ; le résultat arrive sur le canal rendu.
pub fn fetch(path: &Path, full: bool) -> Receiver<Result<Report, String>> {
    let (tx, rx) = channel();
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_blocking(&path, full));
    });
    rx
}

fn fetch_blocking(path: &Path, full: bool) -> Result<Report, String> {
    use std::io::Read;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--info").arg(path);
    if full {
        cmd.arg("complet");
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().ok_or("pas de sortie")?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("l'analyse du fichier ne répond pas".into());
            }
        }
    }
    let text = reader.join().unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(text.trim()).map_err(|_| "l'analyse du fichier a échoué".to_string())?;
    if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
        return Err(e.to_string());
    }
    serde_json::from_value(v).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mediainfo_text_is_split_into_sections() {
        let text = "General\nComplete name                            : C:\\clip : final.mp4\nFormat                                   : MPEG-4\n\nVideo\nID                                       : 1\nWidth                                    : 1 920 pixels\n\n";
        let s = parse_inform(text);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title, "General");
        assert_eq!(s[0].fields[0], ("Complete name".to_string(), "C:\\clip : final.mp4".to_string()));
        assert_eq!(s[1].fields[1], ("Width".to_string(), "1 920 pixels".to_string()));
        let r = Report { source: "x".into(), sections: s };
        assert!(r.to_text().contains("Format                           : MPEG-4"));
    }

    #[test]
    fn sizes_and_durations_read_well() {
        assert_eq!(size_text(512), "512 o");
        assert_eq!(size_text(1536), "1.50 Kio (1536 o)");
        assert_eq!(duration_text(3725.5), "01:02:05.500");
    }
}

#[cfg(test)]
mod mpv_tests {
    /// `CUESTAR_TEST_MEDIA=… cargo test -p cuestar -- --ignored` (libmpv requise).
    #[test]
    #[ignore]
    fn mpv_fallback_reads_tracks() {
        let Ok(media) = std::env::var("CUESTAR_TEST_MEDIA") else { return };
        let r = super::with_mpv(std::path::Path::new(&media)).expect("rapport");
        println!("{}", r.to_text());
        assert!(r.sections.len() >= 2);
    }
}
