//! Boîtes de dialogue de fichiers non bloquantes, reprises de JingleStar : elles s'ouvrent
//! dans leur propre thread et la réponse est traitée à l'image suivante. Pendant ce temps
//! l'interface se dessine et le pilotage (MIDI, OSC, enchaînements) continue.

use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

/// Filtre de fichiers : nom affiché et extensions (sans le point).
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub name: String,
    pub exts: Vec<String>,
}

impl Filter {
    pub fn new(name: impl Into<String>, exts: &[&str]) -> Self {
        Self { name: name.into(), exts: exts.iter().map(|e| e.to_string()).collect() }
    }

    /// Fichiers texte (.txt, .md), pour importer une conduite.
    pub fn text() -> Self {
        Self::new(star_core::i18n::tr("Texte", "Text"), &["txt", "md", "text"])
    }
}

/// Boîte à ouvrir.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// Un fichier.
    Open(Filter),
    /// Plusieurs fichiers.
    OpenMany(Filter),
    /// Un dossier.
    Folder,
    /// Enregistrer sous, avec un nom proposé.
    Save(Filter, String),
}

/// Boîtes de l'appli ; `P` dit quoi faire des fichiers choisis.
pub struct Dialogs<P> {
    tx: Sender<(P, Vec<PathBuf>)>,
    rx: Receiver<(P, Vec<PathBuf>)>,
    /// Une boîte est ouverte : on n'en ouvre pas une seconde.
    pub open: Option<P>,
}

impl<P> Default for Dialogs<P> {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, open: None }
    }
}

impl<P: Clone + Send + 'static> Dialogs<P> {
    pub fn show(&mut self, ctx: &egui::Context, pick: P, kind: impl Into<Kind>) {
        if self.open.is_some() {
            return;
        }
        let kind = kind.into();
        self.open = Some(pick.clone());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("dialogue".into()).spawn(move || {
            let with = |f: &Filter| rfd::AsyncFileDialog::new().add_filter(&f.name, &f.exts);
            let one = |f: Option<rfd::FileHandle>| f.map(|f| vec![f.path().to_path_buf()]).unwrap_or_default();
            let files = match &kind {
                Kind::Open(f) => one(block_on(with(f).pick_file())),
                Kind::OpenMany(f) => block_on(with(f).pick_files()).map(|v| v.iter().map(|f| f.path().to_path_buf()).collect()).unwrap_or_default(),
                Kind::Folder => one(block_on(rfd::AsyncFileDialog::new().pick_folder())),
                Kind::Save(f, name) => one(block_on(with(f).set_file_name(name).save_file())),
            };
            let _ = tx.send((pick, files));
            ctx.request_repaint();
        });
        if spawned.is_err() {
            self.open = None;
        }
    }

    /// Réponse d'une boîte fermée (liste vide : annulée).
    pub fn take(&mut self) -> Option<(P, Vec<PathBuf>)> {
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
