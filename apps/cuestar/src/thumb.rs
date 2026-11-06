//! Vignettes des médias (aperçu « Suivant » du Lecteur).
//!
//! L'extraction tourne dans un processus à part (`--thumb <média> <dossier>`) avec sa propre
//! instance de libmpv : un fichier qui fait planter le décodeur ne touche ni la régie ni la sortie.

use crate::output::mpv::{self, Mpv};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const THUMB_W: u32 = 320;

/// Mode `--thumb` : écrit une image JPEG du média dans `out_dir`.
pub fn extract(media: &str, out_dir: &str) {
    let Ok(m) = Mpv::create() else { return };
    for (k, v) in [
        ("terminal", "no"),
        ("vo", "image"),
        ("vo-image-format", "jpg"),
        ("vo-image-outdir", out_dir),
        ("frames", "1"),
        ("start", "10%"),
        ("aid", "no"),
        ("sid", "no"),
        ("hwdec", "no"),
        ("image-display-duration", "0"),
        ("vf", &format!("scale={THUMB_W}:-2")),
    ] {
        let _ = m.set_option(k, v);
    }
    if m.initialize().is_err() || m.command(&["loadfile", media]).is_err() {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match m.wait_event(0.5).map(|e| e.event_id) {
            Some(mpv::EVENT_END_FILE) | Some(mpv::EVENT_SHUTDOWN) => break,
            _ => {}
        }
    }
}

enum Slot {
    Pending,
    Ready(egui::TextureHandle),
    Failed,
}

/// Cache des vignettes côté régie, remplies en tâche de fond.
pub struct Thumbs {
    slots: HashMap<PathBuf, Slot>,
    tx: Sender<(PathBuf, Option<egui::ColorImage>)>,
    rx: Receiver<(PathBuf, Option<egui::ColorImage>)>,
}

impl Default for Thumbs {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { slots: HashMap::new(), tx, rx }
    }
}

fn is_image(p: &Path) -> bool {
    p.extension()
        .map(|e| ["jpg", "jpeg", "png", "bmp", "gif", "tif", "tiff", "webp"].contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

fn decode(file: &Path) -> Option<egui::ColorImage> {
    let img = image::open(file).ok()?;
    let img = if img.width() > THUMB_W { img.resize(THUMB_W, THUMB_W * 4, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

fn make(media: &Path) -> Option<egui::ColorImage> {
    if is_image(media) {
        if let Some(img) = decode(media) {
            return Some(img);
        }
    }
    let dir = std::env::temp_dir().join(format!("cuestar-thumb-{}-{}", std::process::id(), {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        media.hash(&mut h);
        h.finish()
    }));
    let _ = std::fs::create_dir_all(&dir);
    let mut cmd = std::process::Command::new(std::env::current_exe().ok()?);
    cmd.arg("--thumb").arg(media).arg(&dir);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let file = std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| is_image(p));
    let img = file.as_deref().and_then(decode);
    let _ = std::fs::remove_dir_all(&dir);
    img
}

impl Thumbs {
    /// Vignette du média, demandée en tâche de fond si besoin (None tant qu'elle n'est pas prête).
    pub fn get(&mut self, ctx: &egui::Context, media: &Path) -> Option<&egui::TextureHandle> {
        while let Ok((p, img)) = self.rx.try_recv() {
            let slot = match img {
                Some(img) => Slot::Ready(ctx.load_texture(format!("thumb:{}", p.display()), img, egui::TextureOptions::LINEAR)),
                None => Slot::Failed,
            };
            self.slots.insert(p, slot);
        }
        if !self.slots.contains_key(media) {
            self.slots.insert(media.to_path_buf(), Slot::Pending);
            let (tx, p, ctx) = (self.tx.clone(), media.to_path_buf(), ctx.clone());
            std::thread::Builder::new()
                .name("vignette".into())
                .spawn(move || {
                    let img = make(&p);
                    let _ = tx.send((p, img));
                    ctx.request_repaint();
                })
                .ok();
        }
        match self.slots.get(media) {
            Some(Slot::Ready(t)) => Some(t),
            _ => None,
        }
    }

    pub fn failed(&self, media: &Path) -> bool {
        matches!(self.slots.get(media), Some(Slot::Failed))
    }
}
