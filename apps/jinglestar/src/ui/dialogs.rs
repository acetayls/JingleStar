//! Boîtes de dialogue de fichiers non bloquantes (`star_ui::dialogs`) : ce que JingleStar
//! demande et ce qu'il fait des fichiers choisis.

use star_core::i18n::tr;
use star_ui::dialogs::Filter;

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
    /// Fichier texte à importer dans le module Texte.
    NotesImport(usize),
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
    /// Un fichier texte.
    Text,
}

pub type Dialogs = star_ui::dialogs::Dialogs<Pick>;

impl From<Kind> for star_ui::dialogs::Kind {
    fn from(k: Kind) -> Self {
        use star_ui::dialogs::Kind as K;
        let audio = || Filter::new("Audio", crate::audio::decode::AUDIO_EXTENSIONS);
        let project = |exts: &[&str]| Filter::new(tr("Projet JingleStar", "JingleStar project"), exts);
        match k {
            Kind::Audio => K::Open(audio()),
            Kind::AudioMany => K::OpenMany(audio()),
            Kind::Folder => K::Folder,
            Kind::OpenProject => K::Open(project(&["jinglestar", "json"])),
            Kind::Text => K::Open(Filter::text()),
            Kind::SaveProject => K::Save(project(&["jinglestar"]), "projet.jinglestar".into()),
        }
    }
}
