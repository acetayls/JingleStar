# État du projet — point d'étape du 25 octobre 2026

Ce fichier permet de reprendre le travail après une pause. Les idées à venir sont dans
[`ROADMAP.md`](../ROADMAP.md), le manuel dans [`aide.md`](aide.md) / [`help.md`](help.md).

## Ce qui est sur `main`

**Base**
- Modules dynamiques (1 à 4, largeur réglable par la séparation entre deux modules) en mode **Jingles**, **Playlist**, **Texte** ou **GO**, chacun avec
  son volume, son vumètre, son égaliseur et sa paire de sorties stéréo.
- Préférences audio : pilote, périphérique et buffer, avec ASIO en option. Anti-veille, langue
  FR/EN, Échap qui arrête les jingles (et les playlists en option), anti double-GO.
- Pilotage OSC + OSCQuery + mDNS, MIDI avec apprentissage, et console Yamaha (QL, CL, TF) en
  écoute stricte, sans aucun octet envoyé.

**Jingles**
- 12 boutons, lecture unique ou en boucle, éditeur de forme d'onde façon QLab :
  - poignées début et fin, boucle, et **poignées de fondu** ;
  - **zoom** à la molette, glisser pour se déplacer, double-clic ou « Tout afficher ».
- Égaliseur par jingle. Glisser-déposer de fichiers ou de morceaux de playlist.
- Bouton : **BOUCLE** à la place du temps restant, 🔁 et EQ en haut, cadre vert épais en lecture.
- Fondus : en boucle, le fondu d'entrée joue une fois et l'arrêt applique tout le fondu de sortie ;
  sans boucle, l'arrêt est limité à 1 s. Échap / Tout arrêter / ■ du module : 1 s au plus.
- Éditeur : ascenseur si la fenêtre dépasse l'écran, forme d'onde atténuée dans les fondus,
  barre de défilement sous la forme d'onde zoomée.
- Les jingles **partent à l'appui** du bouton.

**Playlist**
- Dossier ou liste libre (＋ Ajouter : fichiers, dossier, ou dossier qui remplace la liste),
  **Vider** avec confirmation, aléatoire, **tout boucler**, transition fin de fichier ou fondu enchaîné.
- Barre de transport, **pause**, **déplacement dans le morceau** sur la barre de lecture
  (micro-fondu).
- Équilibrage LUFS / crête / RMS, et **gain manuel par morceau** au double-clic.
- **GO immédiat** : le morceau que lancera le GO est préchargé à l'arrêt (il fallait 0,6 à
  1,4 s pour décoder un MP3 de 4 min) et le départ se fait à l'appui.

**Niveaux**
- **Clic sur une valeur en dB** pour la taper : faders, jingles, gains des morceaux.
  Entrée valide, Échap annule. Sans signe, la valeur est négative (`6` → -6 dB).
- Repères +3 / 0 / -6 dB sur les faders.

**Divers**
- Module **Texte** pour la conduite.
- **Renommage des modules** au double-clic sur le nom.
- **Recherche dans l'aide** (Ctrl+F), sans tenir compte des accents.
- **Correspondances MIDI / console modifiables à la main** (bouton ✏).
- **Version de build** (version, commit, date) en bas des Préférences.

## Pull requests ouvertes

- **#3 — CI macOS** : construction de la version Mac par GitHub Actions.

## À vérifier sur le vrai matériel

- **Console QL5** : écoute stricte (aucun octet envoyé), correspondance des touches ON, des
  faders et des rappels de scène.
- **Valeur 0 dB du fader** de la console : qu'un fader à 0 dB donne bien 0 dB dans JingleStar.
- **Rendu ASIO 24 bits** (Windows, `--features asio`) : bit-transparence à 0 dB et absence de
  craquements avec un petit buffer.
- Latence ressentie du GO et des jingles sur une vraie carte son.
- Glisser-déposer sous Windows (position du curseur lue par `GetCursorPos`).

## Compiler et tester

```sh
# Linux : sudo apt install libasound2-dev libxkbcommon-dev libgtk-3-dev
cargo run --release              # lancer
cargo test                       # tests, y compris le banc d'essai audio hors ligne
cargo run --release --features asio   # Windows avec ASIO (CPAL_ASIO_DIR vers le SDK Steinberg)
```

Les exécutables Windows, macOS et Linux sont produits par GitHub Actions à chaque push
(onglet *Actions*, dernier build, *Artifacts*).

Mesure du temps de décodage d'un fichier :

```sh
JS_BENCH=chemin/du/fichier.mp3 cargo test --release bench_decode -- --ignored --nocapture
```
