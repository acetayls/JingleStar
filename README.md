# MediaStar

Les applications de régie de Pierrick et leurs briques communes, dans un seul dépôt.

| Dossier | Contenu | Releases |
| --- | --- | --- |
| [`apps/jinglestar`](apps/jinglestar) | JingleStar, lecteur de jingles et de playlists multi-sorties | tags `jinglestar-vX.Y.Z` |
| [`apps/cuestar`](apps/cuestar) | CueStar | tags `cuestar-vX.Y.Z` |
| [`crates/star-commun`](crates/star-commun) | Charte egui, OSC, MIDI, télécommandes, cuelist | aucune, suit le dépôt |

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
