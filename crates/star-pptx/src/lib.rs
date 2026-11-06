//! Rendu de présentations PowerPoint (.pptx) simples en images, en Rust pur.
//!
//! Un fichier .pptx est une archive zip de parties XML (Office Open XML). Cette crate les lit
//! et dessine chaque diapositive : fonds, formes, texte, images, groupes, tableaux simples.
//! Pas d'animations ni de transitions. Voir le README pour la liste de ce qui est pris en charge.
//!
//! ```no_run
//! let pres = star_pptx::Presentation::open("exemple.pptx")?;
//! for i in 0..pres.slide_count() {
//!     let img = pres.render_slide(i, 1920, 1080)?;
//!     img.save(format!("diapo-{}.png", i + 1)).ok();
//! }
//! # Ok::<(), star_pptx::Error>(())
//! ```

mod color;
mod effects;
mod error;
mod fill;
mod fonts;
mod geom;
mod package;
mod presentation;
mod render;
mod style;
mod text;
mod theme;
mod xml;

pub use error::{Error, Result};
pub use image;
pub use presentation::Presentation;
