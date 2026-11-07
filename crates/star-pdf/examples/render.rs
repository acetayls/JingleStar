//! Rend chaque page d'un PDF en PNG : `cargo run -p star-pdf --example render -- fichier.pdf [dossier] [largeur] [hauteur]`.

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(file) = args.next() else {
        eprintln!("usage : render fichier.pdf [dossier] [largeur] [hauteur]");
        std::process::exit(2);
    };
    let dir = args.next().unwrap_or_else(|| ".".into());
    let w: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1920);
    let h: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1080);
    let doc = star_pdf::Document::open(&file).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    std::fs::create_dir_all(&dir).ok();
    for i in 0..doc.page_count() {
        let t = std::time::Instant::now();
        match doc.render_page(i, w, h) {
            Ok(img) => {
                let out = std::path::Path::new(&dir).join(format!("page-{:03}.png", i + 1));
                img.save(&out).unwrap_or_else(|e| eprintln!("{}: {e}", out.display()));
                println!("{} ({}×{}, {} ms)", out.display(), img.width(), img.height(), t.elapsed().as_millis());
            }
            Err(e) => eprintln!("page {} : {e}", i + 1),
        }
    }
}
