# JingleStar

Lecteur de jingles et de playlists multi-sorties pour Windows, macOS et Linux,
inspiré de QLab et de Jingle Palette. Écrit entièrement en Rust (interface egui,
audio cpal, décodage symphonia).

## Fonctionnalités

- **Modules dynamiques** : on démarre avec un seul, on en ajoute jusqu'à 8 (« ＋ Ajouter un module »)
  et on les supprime en mode édition (🗑). Chacun en mode **Jingles** ou **Playlist**, avec son propre volume et son vumètre.
- **Routage** : chaque module sort sur une paire stéréo (1-2, 3-4, 5-6…). Multipiste avec une carte
  multicanal, ou tous les modules sur 1-2.
- **Jingles** : 12 boutons par module. Clic = lecture, second clic = arrêt.
  - Mode édition (✏) : choisir le son, nom, couleur, volume.
  - Lecture **une fois** ou **en boucle**, avec un éditeur de forme d'onde façon QLab :
    poignée de début (verte), traits de début et de fin de boucle (orange), poignée de fin (rouge).
  - **Fondus** d'entrée et de sortie.
  - **Égaliseur** par jingle (dans l'éditeur du jingle).
  - Glisser-déposer de fichiers sur les boutons (plusieurs fichiers remplissent les boutons suivants).
- **Égaliseur** sur chaque jingle et sur chaque module (bouton « EQ » en mode édition) :
  coupe-bas 12 ou 24 dB/oct, shelf grave, deux bandes paramétriques, shelf aigu. Courbe éditable
  à la souris (glisser = fréquence et gain, molette = Q, double-clic = activer/désactiver).
- **Playlist** : un dossier, enchaînement automatique, **aléatoire**, **boucle**, transition
  **fin de fichier** (sans blanc) ou **fondu enchaîné** réglable.
- **Équilibrage du niveau** des morceaux de la playlist (l'équivalent de « Normalize items » de REAPER),
  en mode édition : « Équilibrer le niveau ». Chaque morceau est mesuré en arrière-plan puis lu avec
  le gain qui l'amène à la cible ; les fichiers ne sont jamais modifiés.
  - Cible au choix : **sonie intégrée** en LUFS (ITU-R BS.1770-4 / EBU R128 : pondération K, blocs
    de 400 ms, portes -70 LUFS et -10 LU), **crête**, **crête vraie** (dBTP) ou **RMS**.
    Repères : -23 LUFS (EBU R128, radio/TV), -14 à -16 LUFS (streaming, sonorisation).
  - **Max** (activé par défaut, -1 dBTP) : le gain d'un morceau est réduit si sa crête vraie dépasserait
    ce plafond, pour ne jamais saturer.
  - Le gain appliqué s'affiche sur chaque morceau (orange s'il a été réduit par le plafond) ; au survol,
    les mesures détaillées. Les mesures sont gardées en cache et refaites si le fichier change.
- **Volumes en dB** affichés en permanence : faders des modules, volume de chaque jingle, gain des morceaux.
- **Préférences** : pilote audio (WASAPI, CoreAudio, ALSA, ASIO en option), périphérique,
  taille de buffer, et **anti-veille** pendant la lecture.
- **Échap** : tout arrêter.
- **Pilotage OSC** avec description **OSCQuery** et annonce mDNS (voir plus bas).
- **Langue** : français ou anglais (Préférences → Système).
- **Aide intégrée** (bouton « ? Aide » ou **F1**), dans la langue choisie. Le manuel est aussi dans
  [`docs/aide.md`](docs/aide.md) (français) et [`docs/help.md`](docs/help.md) (anglais).
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

### Icônes

L'icône est intégrée à l'exécutable Windows à la compilation (`build.rs`) et appliquée à la
fenêtre sur tous les systèmes. Pour une application macOS avec son icône :

```sh
cargo install cargo-bundle
cargo bundle --release
```

## Pilotage OSC et OSCQuery

À activer dans **Préférences → OSC** puis « Appliquer ». Réglages : port UDP de réception
(8000 par défaut), préfixe des adresses (`/jinglestar` par défaut, peut être vide), serveur
OSCQuery et son port HTTP (8001), annonce mDNS / Bonjour et nom annoncé. Les préférences
affichent l'adresse IP et les ports en écoute.

Les numéros de module (1 à 8), de jingle (1 à 12) et de morceau commencent à 1. Les adresses
ci-dessous sont données avec le préfixe par défaut.

| Adresse | Argument | Effet |
|---|---|---|
| `/jinglestar/stop_all` | — | Arrête tout |
| `/jinglestar/module/<m>/stop` | — | Arrête le module |
| `/jinglestar/module/<m>/volume` | float, dB (-60 à +10) | Volume du module |
| `/jinglestar/module/<m>/mode` | `jingle` ou `playlist` | Change le mode du module |
| `/jinglestar/module/<m>/jingle/<j>/play` | — | Lance le jingle depuis le début (le relance s'il joue) |
| `/jinglestar/module/<m>/jingle/<j>/stop` | — | Arrête le jingle |
| `/jinglestar/module/<m>/jingle/<j>/toggle` | — | Lance ou arrête, comme un clic |
| `/jinglestar/module/<m>/jingle/<j>/volume` | float, dB (-40 à +10) | Volume du jingle |
| `/jinglestar/module/<m>/playlist/play` | — ou int | Lecture (au morceau sélectionné, ou au morceau n°) |
| `/jinglestar/module/<m>/playlist/track` | int | Lance le morceau n° |
| `/jinglestar/module/<m>/playlist/stop` | — | Arrête la playlist |
| `/jinglestar/module/<m>/playlist/next` | — | Morceau suivant |
| `/jinglestar/module/<m>/playlist/previous` | — | Morceau précédent |
| `/jinglestar/module/<m>/playlist/shuffle` | bool | Aléatoire |
| `/jinglestar/module/<m>/playlist/loop` | bool | Boucle sur la liste |
| `/jinglestar/module/<m>/playlist/auto_advance` | bool | Enchaînement automatique |

- Les déclencheurs (`play`, `stop`, `toggle`, `next`…) partent sans argument ou avec une valeur
  non nulle : un bouton qui envoie 1 à l'appui puis 0 au relâché ne déclenche qu'une fois.
- Les booléens acceptent `T`/`F`, un entier ou un float (≥ 0,5 = vrai), ou `true`/`false`/`on`/`off`.
- Les commandes de jingle n'agissent que sur un module en mode Jingles, celles de playlist
  sur un module en mode Playlist. Les paquets groupés (bundles) sont acceptés.

**OSCQuery** : `http://<ip>:8001/` renvoie l'arborescence en JSON (types, accès, plages et
valeurs courantes), `/?HOST_INFO` les informations du serveur, et `<chemin>?VALUE` la valeur
d'un nœud. L'arborescence suit l'état du projet : nombre de modules, mode de chacun, nom des
jingles. En lecture seule on y trouve aussi `name`, `jingle/<j>/name`, `jingle/<j>/playing`,
`playlist/playing` et `playlist/tracks`. Le service est annoncé en `_oscjson._tcp` (et
`_osc._udp`), ce qui permet à Chataigne, TouchOSC, Vezér ou Ossia Score de le trouver seuls.
Les notifications en direct (WebSocket `LISTEN`) ne sont pas proposées : les clients relisent
les valeurs en HTTP.

Exemple avec `oscsend` (liblo) :

```sh
oscsend localhost 8000 /jinglestar/module/1/jingle/3/play
oscsend localhost 8000 /jinglestar/module/2/volume f -6
curl http://localhost:8001/jinglestar/module/2/volume?VALUE
```

## Architecture

```
src/
  main.rs            fenêtre eframe
  controller.rs      logique : projet, chargement des sons, jingles, playlists, anti-veille
  project.rs         modèle sauvegardé (JSON)
  osc.rs             commandes OSC (UDP), serveur OSCQuery (HTTP), annonce mDNS
  audio/
    engine.rs        périphérique cpal (thread dédié), choix de la config multicanal
    mixer.rs         mixeur temps réel sans allocation : voix, boucles, fondus, EQ, routage par paire
    eq.rs            égaliseur : réglages, coefficients biquad, filtrage, changements en fondu
    limiter.rs       limiteur de sécurité des sorties (transparent sous 0 dBFS)
    loudness.rs      mesure LUFS / crête / crête vraie / RMS, gain de normalisation, cache
    quality_tests.rs banc d'essai hors ligne de la qualité audio
    decode.rs        décodage symphonia + rééchantillonnage rubato
    buffer.rs        son en mémoire + aperçu de forme d'onde
  ui/
    mod.rs           interface (modules, éditeur de jingle, préférences)
    widgets.rs       boutons, vumètres, éditeur de forme d'onde
    eq_view.rs       courbe d'égaliseur éditable
    help.rs          fenêtre d'aide (affiche docs/aide.md ou docs/help.md)
    theme.rs         thème sombre façon QLab, police du logo
assets/
  icons/             icônes d'application (.ico Windows, .icns macOS, PNG)
  logo/              logo et symbole en SVG, versions claire et sombre
  fonts/             Barlow Condensed (logotype, licence OFL)
build.rs             icône de l'exécutable Windows
```

## Qualité audio

Le chemin audio est vérifié hors ligne par `cargo test` (rendu du mixeur dans des
tampons, sans carte son) :

- à 0 dB sans EQ, la sortie est le fichier au bit près (sortie 16, 24, 32 bits ou flottante) ;
- aucun changement en marche d'escalier : volumes lissés, EQ et changement de sortie en fondu
  de 20 et 10 ms, relance d'un jingle avec fondu court de l'ancien son, rampe de 3 ms quand le
  point de début ou de fin tombe au milieu du fichier, fondu de 4 ms aux points de boucle ;
- limiteur de sécurité par paire de sorties (prévision 1,5 ms) : la somme de plusieurs sons
  n'écrête jamais, et le signal est intact tant qu'il reste sous 0 dBFS ;
- aucune allocation ni verrou dans le callback, fichiers vides ou abîmés refusés sans blocage.

