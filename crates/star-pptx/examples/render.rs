//! Rend chaque diapositive d'une présentation en PNG.
//!
//! `cargo run -p star-pptx --release --example render -- fichier.pptx [dossier] [largeur] [hauteur]`

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(input) = args.get(1) else {
        eprintln!("usage : render <fichier.pptx> [dossier de sortie] [largeur] [hauteur]");
        std::process::exit(2);
    };
    let out = PathBuf::from(args.get(2).cloned().unwrap_or_else(|| ".".into()));
    let w: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1920);
    let h: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1080);
    let t = Instant::now();
    let pres = match star_pptx::Presentation::open(input) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{input} : {e}");
            std::process::exit(1);
        }
    };
    println!("{} diapositives, {:?} EMU, ouvert en {:?}", pres.slide_count(), pres.slide_size_emu(), t.elapsed());
    std::fs::create_dir_all(&out).ok();
    let stem = std::path::Path::new(input).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "diapo".into());
    for i in 0..pres.slide_count() {
        let t = Instant::now();
        match pres.render_slide(i, w, h) {
            Ok(img) => {
                let path = out.join(format!("{stem}-{:02}.png", i + 1));
                if let Err(e) = img.save(&path) {
                    eprintln!("{} : {e}", path.display());
                }
                println!(
                    "{:>3} {}{} {:?} — {}",
                    i + 1,
                    if pres.is_hidden(i) { "[masquée] " } else { "" },
                    pres.slide_title(i).unwrap_or_default(),
                    t.elapsed(),
                    path.display()
                );
            }
            Err(e) => eprintln!("diapositive {} : {e}", i + 1),
        }
    }
}
