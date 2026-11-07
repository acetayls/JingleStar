//! Erreurs de lecture et de rendu.

use std::fmt;

/// Erreur de lecture ou de rendu d'une présentation.
#[derive(Debug)]
pub enum Error {
    /// Fichier illisible.
    Io(std::io::Error),
    /// Le fichier n'est pas une archive zip valide (pas un .pptx ?).
    Zip(String),
    /// Partie XML mal formée.
    Xml { part: String, message: String },
    /// Partie obligatoire absente de l'archive.
    MissingPart(String),
    /// Le fichier ne ressemble pas à une présentation PowerPoint.
    NotAPresentation(String),
    /// Numéro de diapositive hors limites.
    NoSuchSlide { index: usize, count: usize },
    /// Taille de rendu impossible.
    InvalidSize { width: u32, height: u32 },
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "lecture impossible : {e}"),
            Error::Zip(e) => write!(f, "archive illisible (fichier .pptx endommagé ?) : {e}"),
            Error::Xml { part, message } => write!(f, "XML invalide dans {part} : {message}"),
            Error::MissingPart(p) => write!(f, "partie absente de la présentation : {p}"),
            Error::NotAPresentation(why) => write!(f, "ce fichier n'est pas une présentation PowerPoint : {why}"),
            Error::NoSuchSlide { index, count } => write!(f, "diapositive {} inexistante ({count} au total)", index + 1),
            Error::InvalidSize { width, height } => write!(f, "taille de rendu invalide : {width}×{height}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(e) => Error::Io(e),
            e => Error::Zip(e.to_string()),
        }
    }
}
