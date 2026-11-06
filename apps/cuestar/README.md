# CueStar

Diffusion vidéo sur écran de sortie (Windows, macOS, Linux), dans la charte de JingleStar.
Modules : Lecteur, Diaporama, Cuelist (PowerPoint plus tard).

## Architecture

- **Régie** (`src/ui`) : interface egui. Ne rend aucune image de sortie.
- **Moteur de sortie** (`src/output`) : le même exécutable lancé avec `--output`, dans un
  processus séparé. Il ouvre la fenêtre de sortie (plein écran sur le 2e écran, en fenêtre
  s'il n'y a qu'un écran) et y fait rendre **libmpv** au GPU. Une régie qui rame ne retarde
  aucune image. Dialogue en lignes JSON sur stdin/stdout (`src/protocol.rs`).
- **Préférences machine** (`src/prefs.rs`) : décodage Auto/GPU/CPU, rendu Direct3D 11 /
  Vulkan / OpenGL, rendu léger, écran de sortie, calage sur la fréquence de l'écran.
  Stockées par machine, hors du spectacle.
- **star-commun** (`crates/star-commun`) : charte et widgets partagés avec JingleStar,
  destinés à sortir dans leur propre dépôt.

## libmpv

libmpv est chargé au démarrage, pas à la compilation : sans lui, CueStar se lance et la
sortie affiche la mire. Emplacements cherchés : à côté de l'exécutable, dans un dossier
`mpv/` à côté, dans `mpv/` à la racine du dépôt MediaStar (développement), puis le système.

- Windows : `libmpv-2.dll` (build « mpv-dev » de shinchiro/mpv-winbuild-cmake).
- macOS : `brew install mpv`.
- Linux : paquet `libmpv2` / `mpv-libs`.

## Compiler

```
cargo build --release -p cuestar
```
