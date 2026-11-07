//! Boîtes de dialogue de fichiers non bloquantes (`star_ui::dialogs`, comme JingleStar) :
//! ce que CueStar demande et ce qu'il fait des fichiers choisis.

use star_ui::dialogs::Filter;

/// Ce qu'il faut faire des fichiers choisis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pick {
    SaveProject,
    OpenProject,
    Relocate,
    NotesImport,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Folder,
    Text,
    OpenProject,
    SaveProject,
}

pub type Dialogs = star_ui::dialogs::Dialogs<Pick>;

impl From<Kind> for star_ui::dialogs::Kind {
    fn from(k: Kind) -> Self {
        use star_ui::dialogs::Kind as K;
        let ext = crate::project::EXTENSION;
        let project = |exts: &[&str]| Filter::new(star_core::i18n::tr("Projet CueStar", "CueStar project"), exts);
        match k {
            Kind::Folder => K::Folder,
            Kind::Text => K::Open(Filter::text()),
            Kind::OpenProject => K::Open(project(&[ext, "json"])),
            Kind::SaveProject => K::Save(project(&[ext]), format!("projet.{ext}")),
        }
    }
}
