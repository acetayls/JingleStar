//! Boîtes de dialogue de fichiers non bloquantes : elles s'ouvrent dans leur propre thread,
//! et la réponse est traitée à l'image suivante. Pendant ce temps l'interface se dessine
//! et le pilotage (GO MIDI, OSC, enchaînements) continue.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use crossbeam_channel::{unbounded, Receiver, Sender};

/// Ce qu'il faut faire des fichiers choisis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pick {
    SaveProject,
    OpenProject,
    JingleFile(usize, usize),
    PlaylistFiles(usize),
    PlaylistAddFolder(usize),
    PlaylistSetFolder(usize),
    CueFile(usize, usize),
    CueFiles(usize),
    Relocate,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// Un fichier audio.
    Audio,
    /// Plusieurs fichiers audio.
    AudioMany,
    Folder,
    OpenProject,
    SaveProject,
}

pub struct Dialogs {
    tx: Sender<(Pick, Vec<PathBuf>)>,
    rx: Receiver<(Pick, Vec<PathBuf>)>,
    /// Une boîte est ouverte : on n'en ouvre pas une seconde.
    pub open: Option<Pick>,
}

impl Default for Dialogs {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self { tx, rx, open: None }
    }
}

impl Dialogs {
    pub fn show(&mut self, ctx: &egui::Context, pick: Pick, kind: Kind) {
        if self.open.is_some() {
            return;
        }
        self.open = Some(pick);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("dialogue".into()).spawn(move || {
            let audio = || rfd::AsyncFileDialog::new().add_filter("Audio", crate::audio::decode::AUDIO_EXTENSIONS);
            let project = crate::i18n::tr("Projet JingleStar", "JingleStar project");
            let one = |f: Option<rfd::FileHandle>| f.map(|f| vec![f.path().to_path_buf()]).unwrap_or_default();
            let many = |f: Option<Vec<rfd::FileHandle>>| f.map(|v| v.iter().map(|f| f.path().to_path_buf()).collect()).unwrap_or_default();
            let files = match kind {
                Kind::Audio => one(block_on(audio().pick_file())),
                Kind::AudioMany => many(block_on(audio().pick_files())),
                Kind::Folder => one(block_on(rfd::AsyncFileDialog::new().pick_folder())),
                Kind::OpenProject => one(block_on(rfd::AsyncFileDialog::new().add_filter(project, &["jinglestar", "json"]).pick_file())),
                Kind::SaveProject => one(block_on(
                    rfd::AsyncFileDialog::new().add_filter(project, &["jinglestar"]).set_file_name("projet.jinglestar").save_file(),
                )),
            };
            let _ = tx.send((pick, files));
            ctx.request_repaint();
        });
        if spawned.is_err() {
            self.open = None;
        }
    }

    /// Réponse d'une boîte fermée (liste vide : annulée).
    pub fn take(&mut self) -> Option<(Pick, Vec<PathBuf>)> {
        let r = self.rx.try_recv().ok()?;
        self.open = None;
        Some(r)
    }
}

/// Exécute un futur jusqu'au bout dans le thread courant.
fn block_on<F: Future>(f: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        match f.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::park(),
        }
    }
}
