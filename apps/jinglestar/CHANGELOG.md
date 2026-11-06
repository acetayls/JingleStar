# Journal des modifications

## 0.1.3 — 3 novembre 2026

### Téléchargements

- Version publiée sur GitHub (Releases) pour **Mac Intel**, **Mac Apple Silicon** (fichiers .dmg)
  et **Windows 64 bits** (fichier .exe).

### Interface

- Le crayon (mode édition) de chaque module est placé juste à droite de son type (JINGLES,
  PLAYLIST, CUELIST, TEXTE), au lieu du bord droit de l'en-tête.
- **Nouvelle barre du haut** : le nom du projet en grand à gauche, sans mention « Projet » (avec ● s'il y a des
  modifications non enregistrées, le chemin du fichier au survol), à côté du logo réduit à son
  icône. Le menu **▾** à côté du nom regroupe Nouveau, Ouvrir et Enregistrer sous. Le bouton
  **+ Module** passe à droite, avec les Préférences. Le nom du projet est aussi dans le titre de
  la fenêtre.
- **Nom du projet modifiable** : double-clic sur le nom (ou ▾ → Renommer le projet), comme pour
  un module. Il est enregistré dans le projet, sans renommer le fichier ; vide, c'est le nom du
  fichier qui s'affiche.
- La barre d'espace ne lance plus de GO pendant qu'on écrit dans un module Texte (ni dans aucun
  autre champ de saisie) : elle tape un espace.
- **Raccourcis de fichier habituels** : Ctrl+O (ouvrir), Ctrl+Maj+S (enregistrer sous),
  Ctrl+W ou Ctrl+N (fermer le projet : projet vierge, après confirmation), en plus de Ctrl+S.
- **Télécommandes en onglets** : OSC, MIDI, Yamaha, Midas M32 / X32, Correspondances et
  **Raccourcis** (la liste de toutes les touches), au lieu d'une longue page à faire défiler.
- **Raccourci clavier par jingle** : dans l'éditeur d'un jingle, ligne « Raccourci » →
  **Choisir…** puis la touche voulue (avec Ctrl, Alt, Maj si on veut). Si la touche est déjà prise
  par JingleStar ou par un autre jingle, un message le dit et elle n'est pas attribuée. Le
  raccourci lance le jingle (ou l'arrête), sauf pendant la saisie d'un texte. La liste est dans
  Télécommandes → Raccourcis.
- **Faders** : après un clic sur un fader, **+** et **−** montent ou baissent son volume de 1 dB.
  Les flèches ne bougent plus le volume (elles servent à la Cuelist).

### Playlist

- **Suivant en aléatoire** : le bouton (et la télécommande) lance un morceau au hasard, celui que
  la fin du morceau aurait enchaîné, au lieu du suivant dans l'ordre. **Précédent** revient au
  morceau joué juste avant.
- La barre de lecture n'est plus coupée dans un module étroit : un titre long est raccourci
  (complet au survol), et le gain s'efface s'il manque la place pour le temps restant.

### Télécommandes

- **Correspondances plus claires** : à côté du numéro de module s'affiche son nom, ou un ⚠ si
  la correspondance ne fera rien (module absent du projet, ou dans un autre mode que l'action,
  par exemple un jingle visé dans un module passé en Playlist). Un jingle visé sur un
  emplacement vide est signalé aussi.
- Le volume d'un jingle et les options aléatoire / boucle d'une playlist ne changent plus à
  distance quand le module n'est pas dans le bon mode, comme les autres actions.
- Raccourcis clavier : la liste indique les jingles **inactifs** (module hors mode Jingles), et
  le refus d'une touche déjà prise par un tel jingle dit où l'effacer.

### Module Texte

- **Mise en forme** : gras, italique, souligné, barré et surlignage en 4 couleurs (rouge, bleu,
  vert, rose), depuis une barre d'outils au-dessus du texte ou avec Ctrl+B / Ctrl+I / Ctrl+U.
  Un second clic retire le style. Sans sélection, le style s'applique au mot sous le curseur.
  Les boutons des styles du texte sélectionné (ou sous le curseur) s'allument. On ne voit que
  le texte mis en forme, jamais de balise.
- **Zoom du texte** : boutons **−** et **+** dans la barre d'outils (de 60 % à 300 %).
- **Importer…** : ajoute un fichier texte (.txt, .md) à la suite de la conduite.

### Cuelist

- **Pause** (bouton entre Précédent et Arrêter, ou touche **P**) : suspend les cues qui sonnent
  sans perdre leur place ; « EN PAUSE » s'affiche. P ou GO reprend (GO reprend puis lance le cue
  en attente). Un enchaînement attend la reprise.
- **Flèches Haut / Bas** : choisissent le cue en attente sans le lancer ; la liste défile pour
  le garder visible.
- **Glisser-déposer fiable** pour changer l'ordre, vers le haut comme vers le bas, y compris en
  mode édition : on attrape le cue par son numéro (poignée), une ligne montre où il va, et la
  liste défile toute seule près des bords.
- P et les flèches sont inactifs pendant la saisie d'un texte.

### Mémoire

- Un son retiré d'un module (ou remplacé par un autre fichier) libère sa mémoire dans les
  5 secondes, au lieu d'y rester jusqu'à la réouverture du projet. Un son en train de jouer va
  jusqu'au bout.
- **Sons longs lus depuis le disque** : au-delà de 30 secondes, un son n'est plus gardé en
  entier en mémoire. Ses 10 premières secondes, son point d'entrée et son début de boucle restent
  prêts pour un départ instantané ; la suite est lue depuis le disque pendant la lecture, avec
  5 secondes d'avance. La mémoire occupée baisse fortement (environ 4 Mo par morceau au lieu de
  23 Mo par minute). Le son, les boucles, les fondus et les déplacements ne changent pas.
- Si le disque ne suit pas (disque lent, clé USB débranchée), la barre d'état indique
  « Lecture disque en retard » avec la durée de silence. Préférez un SSD interne pour les shows.
- Un point d'entrée ou un début de boucle déplacé (éditeur, OSC, télécommande) est prêt en
  mémoire tout de suite, au lieu d'attendre jusqu'à 5 secondes.

### Fichiers audio

- **Plus de silence d'encodage** au début et à la fin des MP3 (étiquette LAME), des Ogg Vorbis et
  des AAC/M4A (étiquette iTunes) : le son part dès la première image utile, et les morceaux
  s'enchaînent sans blanc. Un en-tête de MP3 incohérent est ignoré pour ne jamais couper de son.

### Companion (Stream Deck)

- **Module Bitfocus Companion** (dossier `companion/`) : pilote JingleStar depuis un Stream Deck
  ou toute surface Companion. Actions pour les jingles, playlists, cuelist, volumes (fixe ou
  +/−) et « Tout arrêter » ; touches qui s'allument selon l'état (jingle qui joue, playlist en
  lecture ou en pause, aléatoire, boucle, cue qui sonne) ; variables (noms, cue en attente,
  numéro du morceau) ; presets prêts à glisser pour chaque module du projet ouvert.
  Il suffit d'activer OSC et OSCQuery dans les préférences de JingleStar.
- Les touches reprennent la **couleur des jingles** et affichent un **compte à rebours** (jingle,
  morceau, cue qui sonne), avec le titre du morceau et le nom du cue en cours. Elles passent à
  l'orange dans les dernières secondes.
- **Pause de la Cuelist à distance** : nouvelles commandes OSC `cue/pause` et `cue/paused`, et
  action « Conduite : pause / reprise » pour les télécommandes MIDI et les consoles.
- **Barre de lecture** sur les touches : elle avance en bas de la touche du jingle, du morceau
  ou du cue qui sonne. L'état est rafraîchi 10 fois par seconde.
- **Vumètres sur le Stream Deck** : une touche par module affiche son niveau gauche et droite
  (vert, jaune, rouge), en entier ou en bande sur le bord d'une touche. JingleStar envoie les
  niveaux environ 20 fois par seconde au module Companion, sans réglage à faire.

## 0.1.2 — 26 octobre 2026

Version de fiabilité : corrections des audits de code (Sonnet et Opus) du 26 octobre 2026.
Le détail technique, et ce qui reste volontairement de côté, est dans
[`docs/audit-notes.md`](docs/audit-notes.md).

### Carte son

- **Reconnexion automatique** : si la carte son est débranchée ou si le pilote s'arrête, un
  bandeau rouge l'annonce en haut de la fenêtre et JingleStar retente toutes les 2 s, sur le même
  périphérique seulement (jamais de bascule vers les haut-parleurs de l'ordinateur). Le bouton
  **Reconnecter** force un essai. Les sons en cours au moment de la coupure sont arrêtés.
- **Interface absente au lancement** : un bandeau orange indique la sortie de secours utilisée,
  et JingleStar repasse sur l'interface dès qu'elle est branchée et que rien ne joue.
- **« Appliquer »** dans les Préférences rouvre une sortie en panne, même sans changer de réglage.
- L'ouverture de la carte son ne fige plus l'interface (pilotes ASIO ou WASAPI lents).
- Les commandes audio perdues pendant une panne sont signalées.

### Session et projets

- **Session jamais perdue** : une session illisible n'est plus remplacée par un projet vierge.
  Elle est mise de côté (`session-illisible-<date>.json`), la copie de secours
  (`session.json.bak`) est chargée et un bandeau le signale. Une session contenant des valeurs
  invalides (écrites par la 0.1.1) est réparée.
- Chaque sauvegarde est forcée sur le disque avant de remplacer la précédente (plus de fichier
  vide après une coupure de courant). Une copie datée est gardée à chaque lancement (les 5
  dernières, dossier `sauvegardes`).
- **Bouton « Enregistrer »** (Ctrl+S) : enregistre dans le projet ouvert ; un ● signale des
  modifications non enregistrées. Le projet ouvert est retenu d'un lancement à l'autre.
- **Préférences de la machine à part** (`machine.json`) : ouvrir un projet venu d'un autre
  ordinateur ne change plus la carte son, les préférences ni les télécommandes.
- **Sons introuvables** :
  - à l'ouverture d'un projet copié ailleurs, les sons sont cherchés à la même place par
    rapport au fichier projet ;
  - bouton **Rechercher…** dans la barre du bas : cherche les sons manquants dans un dossier
    et ses sous-dossiers, par nom de fichier ;
  - un disque rebranché est vu tout seul, et un fichier remplacé sur le disque est relu.

### Lecture

- **Cues « Enchaîner »** : le cue suivant part à l'échantillon près, sans blanc, même quand
  l'interface est occupée.
- **Pilotage indépendant de l'affichage** : GO MIDI, OSC, consoles et enchaînements continuent
  quand une fenêtre « Ouvrir… » est ouverte, ou quand on déplace ou réduit la fenêtre.
  Les boîtes de dialogue de fichiers ne bloquent plus rien.
- **Playlist** : une liste dont aucun morceau n'est lisible (clé USB retirée, boucle activée)
  s'arrête avec un message au lieu de réessayer sans fin. Retirer, ajouter un morceau ou relire
  le dossier pendant la lecture garde le bon morceau suivant.
- Les sons attendus tout de suite (clic, GO, départ de playlist) passent devant les
  préchargements.

### Son

- Fichiers **multicanaux** (5.1…) : tous les canaux sont mixés en stéréo (centre et arrières
  compris) au lieu de garder seulement les canaux 1 et 2.
- Fichiers **abîmés ou tronqués** : signalés par un ⚠ orange dans la barre du bas.
- Sortie **16 bits** : dither ajouté (fins de fondu plus propres).
- Mesure de niveau identique quel que soit le chemin de chargement.

### Robustesse

- Un volume non numérique (« nan » reçu par OSC, fichier modifié à la main) est refusé : il ne
  peut plus rendre une sortie muette ni rendre la session illisible. Les volumes sont bornés.
- Un fichier qui fait planter le décodeur donne une simple erreur ; les autres sons continuent
  de se charger.
- **Journal** `jinglestar.log` (erreurs, plantages) dans le dossier de configuration :
  `%APPDATA%\acetayls\JingleStar\config` sous Windows.
- Moins de mémoire au chargement des sons ; seuil d'**alerte mémoire** réglable dans les
  Préférences ; un jingle ou un cue de plus de 20 minutes est signalé.
- Changer les réglages d'une console Yamaha ou M32, ou quitter, ne fige plus l'interface.

### Divers

- La compilation GitHub Actions ne part plus qu'à la demande (builds faits en local).
- 112 tests automatiques (16 nouveaux : contrôleur, projet, mixeur, décodeur).
