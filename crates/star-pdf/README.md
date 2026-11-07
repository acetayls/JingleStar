# star-pdf

Rendu des pages d'un fichier PDF en images RGBA, pour le module Présentation de CueStar.

Le rendu passe par [hayro](https://github.com/LaurenzV/hayro), un moteur PDF en Rust pur
(licence MIT ou Apache-2.0) : aucune DLL à livrer avec CueStar.

```rust
let doc = star_pdf::Document::open("diapos.pdf")?;
for i in 0..doc.page_count() {
    let img = doc.render_page(i, 1920, 1080)?; // tient dans 1920 × 1080, proportions gardées
}
```

Exemple : `cargo run --release -p star-pdf --example render -- fichier.pdf dossier 1920 1080`.

## Limites (celles de hayro)

- Pas de PDF chiffrés ou protégés par mot de passe.
- Quelques effets de transparence avancés (groupes isolés, knockout) approximés.
- Pas de formulaires interactifs, de vidéo ni de son.
