# MediaStar

Les applications de régie de Pierrick et leurs briques communes, dans un seul dépôt.

| Dossier | Contenu | Releases |
| --- | --- | --- |
| [`apps/jinglestar`](apps/jinglestar) | JingleStar, lecteur de jingles et de playlists multi-sorties | tags `jinglestar-vX.Y.Z` |
| [`apps/cuestar`](apps/cuestar) | CueStar | tags `cuestar-vX.Y.Z` |
| [`crates/star-core`](crates/star-core) | Sans interface : chemins de l'appli, journal, enregistrement sûr des projets et de la session, préférences, langue, EQ, réglages de son | aucune, suit le dépôt |
| [`crates/star-ui`](crates/star-ui) | Interface egui : thème, widgets, aide, EQ, éditeur de son, panneau Texte, boîtes de fichiers | aucune, suit le dépôt |
| [`crates/star-remote`](crates/star-remote) | Télécommandes : MIDI, consoles Yamaha et Midas, correspondances et mode apprendre, serveur OSC / OSCQuery / mDNS ; option `ui` pour la fenêtre Télécommandes | aucune, suit le dépôt |
| [`crates/star-build`](crates/star-build) | `build.rs` commun : version git, date, icône Windows | aucune, suit le dépôt |
| [`crates/star-pptx`](crates/star-pptx) | Rendu des présentations PowerPoint (.pptx) en images, en Rust pur ; utilisé par le module Présentation de CueStar | aucune, suit le dépôt |

Les crates communes sont extraites de JingleStar, l'appli la plus aboutie, puis utilisées par
les deux applis. Elles ne connaissent pas les applis : chaque appli garde ses adresses OSC, ses
actions et le format de ses fichiers, que des tests de format (`tests/fixtures`) protègent.

Les releases de JingleStar antérieures à la fusion portent les tags `v0.1.x`.

## Construire

```sh
cargo run --release -p jinglestar
cargo run --release -p cuestar
cargo test --workspace
```

Toutes les applications partagent la même version d'egui, fixée dans le `Cargo.toml` racine
(`[workspace.dependencies]`).

## Publier une version

Chaque application a sa propre version et son propre calendrier. Avec
[cargo-release](https://github.com/crate-ci/cargo-release) (`cargo install cargo-release`) :

```sh
cargo release -p jinglestar patch            # simulation
cargo release -p jinglestar patch --execute  # bump, commit, tag jinglestar-vX.Y.Z, push
```

Puis lancer le workflow de build de l'application (onglet Actions) avec ce tag pour remplir la
release GitHub.

## Conventions

- Messages de commit préfixés par la zone : `jinglestar: …`, `cuestar: …`, `commun: …`.
- Labels d'issues : `jinglestar`, `cuestar`, `commun`.
