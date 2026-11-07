//! Module Présentation : diffuse une présentation PowerPoint (.pptx) sur la sortie.
//!
//! Les diapositives sont dessinées par la crate star-pptx (Rust pur, sans LibreOffice), dans
//! un fil d'exécution à part, à la résolution de l'écran de sortie. Chaque diapositive devient
//! une image PNG dans un dossier temporaire, envoyée à la sortie comme une photo du Lecteur
//! (`Command::Load`) : le protocole ne change pas. La diapositive suivante est préparée
//! d'avance, pour un passage immédiat.

use crate::link::OutputLink;
use crate::protocol::{Command, Transition, TransitionKind};
use egui::{vec2, Color32, Key, Rect, RichText, Sense, Stroke, StrokeKind};
use star_commun::theme::*;
use star_commun::widgets::{pressed, transport_button, TransportIcon};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

const PPTX_EXT: &[&str] = &["pptx", "ppsx", "potx", "pptm", "ppsm", "potm"];
/// Taille des vignettes de la liste.
const THUMB: (u32, u32) = (320, 180);

struct SlideInfo {
    title: Option<String>,
    hidden: bool,
}

// ------------------------------------------------------------------ rendu en arrière-plan

enum Job {
    Open { gen: u64, path: PathBuf },
    /// Diapositive pour la sortie, dans une boîte `w × h` pixels.
    Slide { gen: u64, index: usize, w: u32, h: u32 },
    Thumb { gen: u64, index: usize },
}

enum Done {
    Opened { gen: u64, slides: Vec<SlideInfo> },
    Failed { gen: u64, message: String },
    Slide { gen: u64, index: usize, w: u32, h: u32, path: PathBuf },
    SlideFailed { gen: u64, index: usize, message: String },
    Thumb { gen: u64, index: usize, image: egui::ColorImage },
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    /// Génération en cours : les travaux d'un fichier précédent sont abandonnés.
    gen: u64,
    quit: bool,
}

struct Worker {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    rx: Receiver<Done>,
}

impl Worker {
    fn start(ctx: egui::Context) -> Worker {
        let queue: Arc<(Mutex<Queue>, Condvar)> = Arc::default();
        let (tx, rx) = channel();
        let q = queue.clone();
        std::thread::Builder::new()
            .name("presentation-rendu".into())
            .spawn(move || worker_loop(q, tx, ctx))
            .ok();
        Worker { queue, rx }
    }

    /// `urgent` : passe devant les vignettes.
    fn push(&self, job: Job, urgent: bool) {
        let (m, cv) = &*self.queue;
        let mut q = m.lock().unwrap_or_else(|e| e.into_inner());
        if let Job::Open { gen, .. } = &job {
            q.gen = *gen;
            q.jobs.clear();
        }
        if urgent {
            q.jobs.push_front(job);
        } else {
            q.jobs.push_back(job);
        }
        cv.notify_one();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (m, cv) = &*self.queue;
        m.lock().unwrap_or_else(|e| e.into_inner()).quit = true;
        cv.notify_one();
    }
}

fn worker_loop(queue: Arc<(Mutex<Queue>, Condvar)>, tx: Sender<Done>, ctx: egui::Context) {
    let mut deck: Option<(u64, Arc<star_pptx::Presentation>, PathBuf)> = None;
    loop {
        let job = {
            let (m, cv) = &*queue;
            let mut q = m.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if q.quit {
                    return;
                }
                if let Some(j) = q.jobs.pop_front() {
                    let gen = match &j {
                        Job::Open { gen, .. } | Job::Slide { gen, .. } | Job::Thumb { gen, .. } => *gen,
                    };
                    if gen == q.gen {
                        break j;
                    }
                    continue;
                }
                q = cv.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let done = match job {
            Job::Open { gen, path } => match star_pptx::Presentation::open(&path) {
                Ok(p) => {
                    let slides = (0..p.slide_count()).map(|i| SlideInfo { title: p.slide_title(i), hidden: p.is_hidden(i) }).collect();
                    deck = Some((gen, Arc::new(p), cache_dir(&path)));
                    Done::Opened { gen, slides }
                }
                Err(e) => {
                    deck = None;
                    Done::Failed { gen, message: e.to_string() }
                }
            },
            Job::Slide { gen, index, w, h } => {
                let Some((g, p, dir)) = &deck else { continue };
                if *g != gen {
                    continue;
                }
                match render_to_file(p, dir, index, w, h) {
                    Ok(path) => Done::Slide { gen, index, w, h, path },
                    Err(message) => Done::SlideFailed { gen, index, message },
                }
            }
            Job::Thumb { gen, index } => {
                let Some((g, p, _)) = &deck else { continue };
                if *g != gen {
                    continue;
                }
                match p.render_slide(index, THUMB.0, THUMB.1) {
                    Ok(img) => {
                        let size = [img.width() as usize, img.height() as usize];
                        Done::Thumb { gen, index, image: egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()) }
                    }
                    Err(e) => Done::SlideFailed { gen, index, message: e.to_string() },
                }
            }
        };
        if tx.send(done).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// Dossier temporaire des images d'un fichier (change si le fichier est modifié).
fn cache_dir(path: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(m) = std::fs::metadata(path) {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    std::env::temp_dir().join("cuestar-presentation").join(format!("{:016x}", h.finish()))
}

fn render_to_file(p: &star_pptx::Presentation, dir: &Path, index: usize, w: u32, h: u32) -> Result<PathBuf, String> {
    let file = dir.join(format!("diapo-{:03}-{w}x{h}.png", index + 1));
    if file.is_file() {
        return Ok(file);
    }
    let img = p.render_slide(index, w, h).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("dossier temporaire : {e}"))?;
    // Écrit à côté puis renomme : la sortie ne lit jamais une image à moitié écrite.
    let tmp = file.with_extension("tmp");
    let out = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let enc = image::codecs::png::PngEncoder::new_with_quality(std::io::BufWriter::new(out), image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Sub);
    image::ImageEncoder::write_image(enc, img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    Ok(file)
}

// ------------------------------------------------------------------ module

pub struct Presentation {
    path: Option<PathBuf>,
    gen: u64,
    loading: bool,
    error: Option<String>,
    slides: Vec<SlideInfo>,
    thumbs: HashMap<usize, egui::TextureHandle>,
    /// Images prêtes pour la sortie : (diapositive, boîte) → fichier.
    rendered: HashMap<(usize, (u32, u32)), PathBuf>,
    requested: HashSet<(usize, (u32, u32))>,
    /// Diapositive demandée, en attente de son image.
    want: Option<usize>,
    /// Dernière diapositive envoyée à la sortie, et son image.
    shown: Option<(usize, PathBuf)>,
    /// Diapositive sélectionnée dans la liste (point de départ de « Lancer »).
    selected: usize,
    transition: Transition,
    worker: Option<Worker>,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            path: None,
            gen: 0,
            loading: false,
            error: None,
            slides: Vec::new(),
            thumbs: HashMap::new(),
            rendered: HashMap::new(),
            requested: HashSet::new(),
            want: None,
            shown: None,
            selected: 0,
            transition: Transition { kind: TransitionKind::Cut, secs: 0.5 },
            worker: None,
        }
    }
}

impl Presentation {
    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        let worker = self.worker.get_or_insert_with(|| Worker::start(ctx.clone()));
        self.gen += 1;
        self.loading = true;
        self.error = None;
        self.slides.clear();
        self.thumbs.clear();
        self.rendered.clear();
        self.requested.clear();
        self.want = None;
        self.shown = None;
        self.selected = 0;
        worker.push(Job::Open { gen: self.gen, path: path.clone() }, true);
        self.path = Some(path);
    }

    /// Boîte de rendu : la taille de l'écran de sortie.
    fn target(out: &OutputLink) -> (u32, u32) {
        match &out.state.monitor {
            Some(m) if m.width >= 16 && m.height >= 16 => (m.width.min(7680), m.height.min(4320)),
            _ => (1920, 1080),
        }
    }

    fn request(&mut self, index: usize, size: (u32, u32), urgent: bool) {
        if index >= self.slides.len() || self.rendered.contains_key(&(index, size)) || !self.requested.insert((index, size)) {
            return;
        }
        if let Some(w) = &self.worker {
            w.push(Job::Slide { gen: self.gen, index, w: size.0, h: size.1 }, urgent);
        }
    }

    fn next_visible(&self, from: Option<usize>) -> Option<usize> {
        let start = from.map(|i| i + 1).unwrap_or(0);
        (start..self.slides.len()).find(|&i| !self.slides[i].hidden)
    }

    fn prev_visible(&self, from: usize) -> Option<usize> {
        (0..from.min(self.slides.len())).rev().find(|&i| !self.slides[i].hidden)
    }

    /// Met la diapositive `index` à l'antenne (dès que son image est prête).
    fn go(&mut self, index: usize, out: &mut OutputLink) {
        if index >= self.slides.len() {
            return;
        }
        let size = Self::target(out);
        self.selected = index;
        if let Some(path) = self.rendered.get(&(index, size)).cloned() {
            self.send(index, path, out);
        } else {
            self.want = Some(index);
            self.request(index, size, true);
        }
        // Les voisines sont préparées d'avance.
        if let Some(n) = self.next_visible(Some(index)) {
            self.request(n, size, true);
        }
        if let Some(p) = self.prev_visible(index) {
            self.request(p, size, false);
        }
    }

    fn send(&mut self, index: usize, path: PathBuf, out: &mut OutputLink) {
        self.want = None;
        let p = path.to_string_lossy().into_owned();
        out.send(Command::Load { path: p, paused: false, looping: false, hold: true, transition: self.transition });
        self.shown = Some((index, path));
    }

    /// La dernière diapositive envoyée est-elle à l'écran ?
    fn on_air(&self, out: &OutputLink) -> Option<usize> {
        let (i, p) = self.shown.as_ref()?;
        (out.state.current.as_deref() == Some(&*p.to_string_lossy())).then_some(*i)
    }

    fn next(&mut self, out: &mut OutputLink) {
        let from = match self.on_air(out).or(self.shown.as_ref().map(|s| s.0)) {
            Some(i) => Some(i),
            // Rien encore à l'écran : on part de la sélection.
            None => {
                if self.selected < self.slides.len() {
                    self.go(self.selected, out);
                    return;
                }
                None
            }
        };
        if let Some(n) = self.next_visible(from) {
            self.go(n, out);
        }
    }

    fn prev(&mut self, out: &mut OutputLink) {
        if let Some((i, _)) = &self.shown {
            if let Some(p) = self.prev_visible(*i) {
                self.go(p, out);
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context, out: &mut OutputLink) {
        let Some(w) = &self.worker else { return };
        let done: Vec<Done> = w.rx.try_iter().collect();
        for d in done {
            match d {
                Done::Opened { gen, slides } if gen == self.gen => {
                    self.loading = false;
                    self.slides = slides;
                    self.selected = self.next_visible(None).unwrap_or(0);
                    if self.slides.is_empty() {
                        self.error = Some("cette présentation ne contient aucune diapositive".into());
                    }
                    // Première diapositive prête tout de suite, puis les vignettes.
                    let size = Self::target(out);
                    let first = self.selected;
                    self.request(first, size, true);
                    if let Some(w) = &self.worker {
                        for i in 0..self.slides.len() {
                            w.push(Job::Thumb { gen, index: i }, false);
                        }
                    }
                }
                Done::Failed { gen, message } if gen == self.gen => {
                    self.loading = false;
                    self.error = Some(message);
                }
                Done::Slide { gen, index, w, h, path } if gen == self.gen => {
                    self.rendered.insert((index, (w, h)), path.clone());
                    if self.want == Some(index) && Self::target(out) == (w, h) {
                        self.send(index, path, out);
                    }
                }
                Done::SlideFailed { gen, index, message } if gen == self.gen => {
                    if self.want == Some(index) {
                        self.want = None;
                    }
                    self.error = Some(format!("diapositive {} : {message}", index + 1));
                }
                Done::Thumb { gen, index, image } if gen == self.gen => {
                    let tex = ctx.load_texture(format!("pptx-{gen}-{index}"), image, egui::TextureOptions::LINEAR);
                    self.thumbs.insert(index, tex);
                }
                _ => {}
            }
        }
    }

    fn pick_file(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new().add_filter("Présentation PowerPoint", PPTX_EXT);
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        }
        if let Some(p) = dlg.pick_file() {
            self.open(ctx, p);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, out: &mut OutputLink, typing: bool) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx, out);
        // Un fichier .pptx glissé sur l'onglet est ouvert.
        let dropped: Option<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| p.extension().map(|e| PPTX_EXT.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false))
        });
        if let Some(p) = dropped {
            self.open(&ctx, p);
        }
        let on_air = self.on_air(out);
        let ready = !self.slides.is_empty();
        if !typing && ready {
            let (next, prev, first, last) = ui.input_mut(|i| {
                let next = i.consume_key(egui::Modifiers::NONE, Key::Space) || i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::PageDown) || i.key_pressed(Key::Enter);
                let prev = i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::PageUp) || i.key_pressed(Key::Backspace);
                (next, prev, i.key_pressed(Key::Home), i.key_pressed(Key::End))
            });
            if next {
                self.next(out);
            }
            if prev {
                self.prev(out);
            }
            if first {
                if let Some(i) = self.next_visible(None) {
                    self.go(i, out);
                }
            }
            if last {
                if let Some(i) = self.prev_visible(self.slides.len()) {
                    self.go(i, out);
                }
            }
        }

        // En cours.
        let width = ui.available_width().min(640.0);
        egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x1c, 0x20)).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.set_width(width - 12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.want.is_some() {
                        ui.label(RichText::new("PRÉPARATION…").strong().small().color(ACCENT));
                    }
                    let count = match (on_air.or(self.shown.as_ref().map(|s| s.0)), self.slides.len()) {
                        (_, 0) => String::new(),
                        (Some(i), n) => format!("{} / {}", i + 1, n),
                        (None, n) => format!("{n} diapositives"),
                    };
                    ui.label(RichText::new(count).monospace().color(TEXT_DIM));
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let (name, col) = match on_air {
                            Some(i) => (self.label(i), GO),
                            None if self.loading => ("Ouverture de la présentation…".into(), TEXT_DIM),
                            None if ready => ("Présentation prête, pas à l'antenne".into(), TEXT_DIM),
                            None => ("Aucune présentation".into(), TEXT_DIM),
                        };
                        ui.add(egui::Label::new(RichText::new(name).strong().size(15.0).color(col)).truncate());
                    });
                });
            });
        });

        // Transport : précédente, lancer / suivante, noir, suivante.
        let (row, _) = ui.allocate_exact_size(vec2(width, 38.0), Sense::hover());
        let gap = 4.0;
        let side = 48.0f32.min(row.width() / 6.0);
        let go_w = row.width() - 3.0 * side - 3.0 * gap;
        let mut x = row.left();
        let mut cell = |w: f32| {
            let r = Rect::from_min_size(egui::pos2(x, row.top()), vec2(w, row.height()));
            x += w + gap;
            r
        };
        let (prev_r, go_r, stop_r, next_r) = (cell(side), cell(go_w), cell(side), cell(side));
        let fg = if ready { TEXT } else { TEXT_DIM };
        if transport_button(ui, prev_r, "pptx-prev", TransportIcon::Prev, None, PANEL_HI, fg).on_hover_text("Diapositive précédente (←)").clicked() && ready {
            self.prev(out);
        }
        let go_label = if on_air.is_some() { "SUIVANTE" } else { "LANCER" };
        let go = transport_button(ui, go_r, "pptx-go", TransportIcon::Play, Some(go_label), GO, Color32::from_rgb(0x0c, 0x1a, 0x0e))
            .on_hover_text(if on_air.is_some() { "Diapositive suivante (Espace, →)" } else { "Afficher la diapositive sélectionnée sur la sortie (Espace)" });
        if pressed(ui, &go) && ready {
            if on_air.is_some() {
                self.next(out);
            } else {
                let i = self.selected;
                self.go(i, out);
            }
        }
        let stop = transport_button(ui, stop_r, "pptx-stop", TransportIcon::Stop, None, STOP.gamma_multiply(0.75), Color32::WHITE).on_hover_text("Noir (Échap) : « Lancer » réaffiche la diapositive");
        if pressed(ui, &stop) {
            out.send(Command::Stop);
        }
        if transport_button(ui, next_r, "pptx-next", TransportIcon::Next, None, PANEL_HI, fg).on_hover_text("Diapositive suivante (→)").clicked() && ready {
            self.next(out);
        }
        ui.add_space(10.0);

        // Fichier et réglages.
        egui::Frame::new().fill(PANEL).corner_radius(4).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("pptx").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Fichier");
                ui.horizontal(|ui| {
                    let txt = self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "aucun".into());
                    ui.add(egui::Label::new(RichText::new(txt).color(if self.path.is_some() { TEXT } else { TEXT_DIM })).truncate());
                    if ui.button("Choisir…").on_hover_text("Ouvrir une présentation PowerPoint (.pptx), ou la glisser ici").clicked() {
                        self.pick_file(&ctx);
                    }
                    if self.path.is_some() && ui.button("Relire").on_hover_text("Relit le fichier (après une modification dans PowerPoint)").clicked() {
                        if let Some(p) = self.path.clone() {
                            self.open(&ctx, p);
                        }
                    }
                });
                ui.end_row();

                ui.label("Aller à");
                ui.horizontal(|ui| {
                    let n = self.slides.len().max(1);
                    let mut num = self.selected.min(n - 1) + 1;
                    if ui.add_enabled(ready, egui::DragValue::new(&mut num).range(1..=n).speed(0.1)).changed() {
                        self.selected = num - 1;
                    }
                    ui.label(RichText::new(format!("/ {}", self.slides.len())).color(TEXT_DIM));
                    if ui.add_enabled(ready, egui::Button::new("Afficher")).on_hover_text("Met cette diapositive à l'antenne (double-clic sur une vignette : pareil)").clicked() {
                        let i = self.selected;
                        self.go(i, out);
                    }
                });
                ui.end_row();

                ui.label("Transition");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.transition.kind, TransitionKind::Cut, "Coupe").on_hover_text("Passage direct à la diapositive suivante");
                    ui.radio_value(&mut self.transition.kind, TransitionKind::Black, "Noir").on_hover_text("Fondu au noir, puis fondu d'ouverture");
                    ui.radio_value(&mut self.transition.kind, TransitionKind::Crossfade, "Fondu enchaîné");
                    if self.transition.kind != TransitionKind::Cut {
                        ui.add(egui::DragValue::new(&mut self.transition.secs).range(0.1..=5.0).speed(0.05).suffix(" s").max_decimals(1));
                    }
                });
                ui.end_row();
            });
            if let Some(e) = &self.error {
                ui.add_space(4.0);
                ui.label(RichText::new(e).color(STOP));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Sans animations ni transitions PowerPoint. Les diapositives masquées sont sautées par « Suivante ».").color(TEXT_DIM));
        });

        ui.add_space(10.0);
        ui.label(RichText::new(format!("Diapositives ({})", self.slides.len())).strong());
        egui::Frame::new().fill(SLOT_EMPTY).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if self.slides.is_empty() {
                ui.centered_and_justified(|ui| {
                    let msg = if self.loading { "Ouverture…" } else { "Choisis une présentation PowerPoint (.pptx), ou glisse-la ici" };
                    ui.label(RichText::new(msg).color(TEXT_DIM));
                });
                return;
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                    for i in 0..self.slides.len() {
                        if self.thumb_cell(ui, i, on_air) {
                            self.go(i, out);
                        }
                    }
                });
            });
        });
    }

    fn label(&self, i: usize) -> String {
        match self.slides.get(i).and_then(|s| s.title.clone()) {
            Some(t) => format!("{} — {t}", i + 1),
            None => format!("Diapositive {}", i + 1),
        }
    }

    /// Une vignette ; vrai si elle doit passer à l'antenne (double-clic).
    fn thumb_cell(&mut self, ui: &mut egui::Ui, i: usize, on_air: Option<usize>) -> bool {
        let tw = 192.0;
        let th = tw * 9.0 / 16.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(tw, th + 20.0), Sense::click());
        if !ui.is_rect_visible(rect) {
            return false;
        }
        let p = ui.painter();
        let img_r = Rect::from_min_size(rect.min, vec2(tw, th));
        p.rect_filled(img_r, 3, Color32::BLACK);
        let hidden = self.slides[i].hidden;
        match self.thumbs.get(&i) {
            Some(tex) => {
                let [w, h] = tex.size();
                let s = (tw / w as f32).min(th / h as f32);
                let r = Rect::from_center_size(img_r.center(), vec2(w as f32 * s, h as f32 * s));
                let tint = if hidden { Color32::from_gray(90) } else { Color32::WHITE };
                p.image(tex.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
            }
            None => {
                p.text(img_r.center(), egui::Align2::CENTER_CENTER, "…", egui::FontId::proportional(18.0), TEXT_DIM);
            }
        }
        let (stroke, width) = if on_air == Some(i) {
            (GO, 3.0)
        } else if self.want == Some(i) {
            (ACCENT, 3.0)
        } else if self.selected == i {
            (EDIT, 2.0)
        } else if resp.hovered() {
            (BORDER, 1.0)
        } else {
            (Color32::TRANSPARENT, 0.0)
        };
        if width > 0.0 {
            p.rect_stroke(img_r, 3, Stroke::new(width, stroke), StrokeKind::Outside);
        }
        let caption = format!("{}{}{}", i + 1, if hidden { " (masquée)" } else { "" }, self.slides[i].title.as_ref().map(|t| format!(" · {t}")).unwrap_or_default());
        let col = if on_air == Some(i) { GO } else if hidden { TEXT_DIM } else { TEXT };
        let galley = ui.painter().layout(caption, egui::FontId::proportional(12.0), col, tw);
        let clip = Rect::from_min_size(egui::pos2(rect.left(), img_r.bottom() + 3.0), vec2(tw, 16.0));
        ui.painter().with_clip_rect(clip).galley(clip.min, galley, col);
        let resp = resp.on_hover_text("Clic : sélectionner · double-clic : afficher sur la sortie");
        if resp.clicked() {
            self.selected = i;
        }
        resp.double_clicked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait(w: &Worker, mut f: impl FnMut(Done) -> bool) {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(60) {
            if let Ok(d) = w.rx.recv_timeout(Duration::from_millis(100)) {
                if f(d) {
                    return;
                }
            }
        }
        panic!("le rendu n'a pas répondu");
    }

    #[test]
    fn worker_opens_and_renders_slides_to_png() {
        let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/star-pptx/tests/fixtures/demo.pptx"));
        let w = Worker::start(egui::Context::default());
        w.push(Job::Open { gen: 1, path }, true);
        let mut count = 0;
        wait(&w, |d| match d {
            Done::Opened { slides, .. } => {
                count = slides.len();
                assert!(slides[6].hidden);
                assert_eq!(slides[0].title.as_deref(), Some("Présentation de démonstration"));
                true
            }
            Done::Failed { message, .. } => panic!("{message}"),
            _ => false,
        });
        assert_eq!(count, 9);
        w.push(Job::Slide { gen: 1, index: 2, w: 640, h: 480 }, true);
        // Un travail d'une ancienne génération est ignoré.
        w.push(Job::Slide { gen: 0, index: 1, w: 640, h: 480 }, true);
        wait(&w, |d| match d {
            Done::Slide { index, path, .. } => {
                assert_eq!(index, 2);
                let img = image::open(&path).unwrap();
                assert_eq!((img.width(), img.height()), (640, 360));
                true
            }
            Done::SlideFailed { message, .. } => panic!("{message}"),
            _ => false,
        });
        w.push(Job::Thumb { gen: 1, index: 0 }, false);
        wait(&w, |d| matches!(d, Done::Thumb { index: 0, .. }));
    }
}
