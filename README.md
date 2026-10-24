# JingleStar

Lecteur de jingles et de playlists multi-sorties pour Windows, macOS et Linux,
inspiré de QLab et de Jingle Palette. Écrit entièrement en Rust (interface egui,
audio cpal, décodage symphonia).

## Fonctionnalités

- **1 à 8 modules**, chacun en mode **Jingles** ou **Playlist**, avec son propre volume et son vumètre.
- **Routage** : chaque module sort sur une paire stéréo (1-2, 3-4, 5-6…). Multipiste avec une carte
  multicanal, ou tous les modules sur 1-2.
- **Jingles** : 12 boutons par module. Clic = lecture, second clic = arrêt.
  - Mode édition (✏) : choisir le son, nom, couleur, volume.
  - Lecture **une fois** ou **en boucle**, avec un éditeur de forme d'onde façon QLab :
    poignée de début (verte), traits de début et de fin de boucle (orange), poignée de fin (rouge).
  - **Fondus** d'entrée et de sortie.
  - Glisser-déposer de fichiers sur les boutons (plusieurs fichiers remplissent les boutons suivants).
- **Playlist** : un dossier, enchaînement automatique, **aléatoire**, **boucle**, transition
  **fin de fichier** (sans blanc) ou **fondu enchaîné** réglable.
- **Préférences** : pilote audio (WASAPI, CoreAudio, ALSA, ASIO en option), périphérique,
  taille de buffer, et **anti-veille** pendant la lecture.
- **Échap** : tout arrêter.
- Session sauvegardée automatiquement ; projets enregistrables (`.jinglestar`).

Formats lus : WAV, AIFF, MP3, FLAC, OGG/Vorbis, AAC/M4A.

## Lancer

1. Installer Rust : <https://rustup.rs>
   - Windows : installer aussi les « Build Tools for Visual Studio » (C++), proposés par rustup.
   - Linux : `sudo apt install libasound2-dev libxkbcommon-dev libgtk-3-dev`
2. Dans le dossier du projet :

```sh
cargo run --release
```

Les exécutables Windows, macOS et Linux sont aussi produits à chaque push par GitHub Actions
(onglet *Actions* → dernier build → *Artifacts*).

### ASIO (Windows, optionnel)

Télécharger le SDK ASIO de Steinberg, définir `CPAL_ASIO_DIR` vers son dossier, puis :

```sh
cargo run --release --features asio
```

## Architecture

```
src/
  main.rs            fenêtre eframe
  controller.rs      logique : projet, chargement des sons, jingles, playlists, anti-veille
  project.rs         modèle sauvegardé (JSON)
  audio/
    engine.rs        périphérique cpal (thread dédié), choix de la config multicanal
    mixer.rs         mixeur temps réel sans allocation : voix, boucles, fondus, routage par paire
    decode.rs        décodage symphonia + rééchantillonnage rubato
    buffer.rs        son en mémoire + aperçu de forme d'onde
  ui/
    mod.rs           interface (modules, éditeur de jingle, préférences)
    widgets.rs       boutons, vumètres, éditeur de forme d'onde
    theme.rs         thème sombre façon QLab
```

À venir : équilibrage du volume entre les morceaux de la playlist.
