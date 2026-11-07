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
- **Crates communes** (`crates/star-*`), partagées avec JingleStar : fichiers et session
  (`star-core`), thème, widgets, panneau Texte et langue français / anglais (`star-ui`),
  télécommandes MIDI / consoles et serveur OSC avec OSCQuery et mDNS (`star-remote`).
- **OSC** (`src/osc.rs`) : adresses de CueStar (préfixe `/cuestar`, `/diaporama/…` accepté
  aussi). En option, OSCQuery décrit les adresses et l'état courant, et `<préfixe>/subscribe`
  abonne pendant 5 s au retour d'état (média, temps restant, cue en attente…).

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
