//! Socle commun aux applis Star (JingleStar, CueStar), sans interface graphique : il sert
//! aussi bien à la régie qu'aux processus sans fenêtre (sortie vidéo de CueStar).
//!
//! Extrait du code de JingleStar. Aucune crate commune ne connaît les applis elles-mêmes.

pub mod app;
pub mod document;
pub mod eq;
pub mod i18n;
pub mod log;
pub mod num;
pub mod prefs;
pub mod session;
pub mod sound;
pub mod time;
