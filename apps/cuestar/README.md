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
`mpv/` à côté, dans le paquet (`Contents/Frameworks` sur macOS, `usr/lib` de l'AppImage),
dans `mpv/` à la racine du dépôt MediaStar (développement), puis le système.
Les paquets de release embarquent libmpv et MediaInfo : l'utilisateur n'a rien d'autre à installer.

- Windows : `libmpv-2.dll` (build « mpv-dev » de shinchiro/mpv-winbuild-cmake).
- macOS : `brew install mpv`.
- Linux : paquet `libmpv2` / `mpv-libs`.

## Compiler

```
cargo build --release -p cuestar
```

## Paquets de release

Workflow **Build CueStar** (onglet Actions, à la demande), ou à la main avec les scripts de
`packaging/`, après `cargo build --release -p cuestar` :

- Windows : `packaging\windows\package.ps1` → installeur `CueStar-<version>-windows-x64-setup.exe`
  (Inno Setup 6) et zip portable, avec `libmpv-2.dll` et `MediaInfo.dll` pris dans `mpv\`.
- macOS : `packaging/macos/bundle.sh <cible>` → `.dmg` avec `CueStar.app`, libmpv, libmediainfo
  et leurs dépendances Homebrew dans `Contents/Frameworks` (`brew install mpv libmediainfo dylibbundler`). Signé et
  notarisé si les secrets Apple sont fournis, sinon signature ad hoc et un fichier
  « LISEZ-MOI - Ouvrir CueStar.txt » dans le `.dmg`. Ouvrir une version non signée, et créer
  le compte et les secrets pour signer : [packaging/macos/SIGNATURE.md](packaging/macos/SIGNATURE.md).
- Linux : `packaging/linux/appimage.sh` → AppImage avec libmpv, libmediainfo et leurs dépendances.

Les licences (CueStar, mpv, FFmpeg, MediaInfo, bibliothèques Rust) sont dans `packaging/licences` et
copiées dans chaque paquet. Après un changement de dépendances Rust :
`python3 apps/cuestar/packaging/licences-crates.py`.
