# Aide de CueStar

CueStar envoie des vidéos, des images et des sons sur un écran de sortie (vidéoprojecteur,
écran de scène, régie vidéo). La régie, où vous travaillez, et la **sortie**, qui affiche
l'image, tournent dans deux programmes séparés : une régie qui rame ne retarde aucune image.

Trois modules se partagent l'écran : le **Lecteur** (une playlist), le **Diaporama** (un
dossier de photos et de vidéos) et la **Cuelist** (une conduite de cues lancés au GO). Le
panneau **Texte** s'affiche à côté pour garder la conduite écrite sous les yeux.

## Prise en main

1. Au lancement, la sortie s'ouvre en plein écran sur le deuxième écran (dans une fenêtre s'il
   n'y en a qu'un). La barre du bas indique **Sortie : 1920×1080 @ 60.00 Hz** quand elle est prête.
2. Dans le **Lecteur**, cliquez sur **Ajouter** ou glissez des fichiers sur la liste.
3. Double-cliquez sur un élément, ou sélectionnez-le et appuyez sur **Espace** (GO).
4. **Échap** ou le bouton rouge **NOIR** coupe l'image à tout moment.
5. Clic droit sur un élément → **Éditer…** pour régler son entrée, sa sortie, ses fondus au
   noir, son volume et son égaliseur.

Tout est gardé automatiquement : en relançant CueStar, vous retrouvez la session telle que vous
l'avez laissée. **Enregistrer** écrit le projet dans un fichier `.cuestar`.

Formats lus : vidéos (MP4, MOV, MKV, AVI, WebM, MXF, MPEG, TS, WMV…), images (JPEG, PNG, BMP,
GIF, TIFF, WebP) et sons (MP3, WAV, FLAC, AAC/M4A, OGG). Le décodage passe par libmpv.

## L'écran

### Barre du haut

| Élément | Rôle |
|---|---|
| Nom du projet | Menu : Nouveau, Ouvrir, Enregistrer sous, Renommer |
| **Enregistrer** | Enregistre le projet (**Ctrl+S**) |
| **Lecteur**, **Diaporama**, **Cuelist** | Change de module ; celui qui ne s'affiche plus continue de jouer |
| **Texte** | Affiche ou masque le panneau Texte |
| **? Aide** | Ouvre cette aide (**F1**) |
| **Video output** | Écran et moteur de sortie, son, mires de test, mapping 4 coins |
| **Monitoring** | Fenêtre flottante qui montre l'image envoyée sur la sortie |
| **Télécommandes** | OSC, MIDI, consoles Yamaha et Midas, correspondances, raccourcis |
| **NOIR** | Coupe l'image : écran de sortie au noir (**Échap**) |

### Barre du bas

- **Sortie** : voyant vert quand l'image passe par libmpv, orange quand libmpv manque (la
  sortie affiche alors une mire), rouge quand la sortie est arrêtée (**Relancer la sortie**).
- Version de mpv et décodeur en cours (GPU ou CPU).
- **Images perdues / retardées** : images que la sortie n'a pas pu afficher à temps, cumulées
  sur tous les médias depuis la dernière remise à zéro. Le détail s'affiche au survol. Pour
  repartir de zéro : le bouton **↺**, un clic droit sur le compteur → **Remettre à zéro**,
  l'adresse OSC `output/drops/reset` ou une télécommande.
- Images par seconde du média, charge du processeur et du GPU.

## Le Lecteur

Une playlist de vidéos, d'images et de sons. **Ajouter** ouvre des fichiers ou un dossier ;
on peut aussi les glisser sur la liste. **Vider** efface la liste (avec confirmation).

- **GO** (ou **Espace**) lance l'élément sélectionné, ou reprend après une pause.
- **⏮ / ⏭** : élément précédent / suivant. **■** coupe l'image.
- **Flèche Haut / Bas** : change l'élément sélectionné sans le lancer.
- **Aléatoire** enchaîne au hasard, **Tout boucler** reprend au début à la fin de la liste.
- L'icône de boucle au bout d'une ligne fait tourner cet élément en boucle.
- **Transition** : **Coupe** (passage direct), **Noir** (fondu au noir puis ouverture) ou
  **Fondu enchaîné**, avec sa durée.

### Aperçu SUIVANT

À droite, l'aperçu montre l'élément qui partira au prochain GO. On s'y promène sans toucher à
la sortie : cliquez ou glissez sur la barre sous l'image, **▶** lit l'aperçu (sans le son),
**⏮ / ⏭** avancent ou reculent d'une image. Les repères d'entrée et de sortie de l'élément sont
dessinés sur la barre. **Aperçu** (en haut de la liste) l'affiche ou le masque.

### Clic droit sur un élément

| Commande | Effet |
|---|---|
| **Lire** | Lance l'élément |
| **Éditer…** | Ouvre l'éditeur de l'élément (voir [Éditer un élément](#éditer-un-élément)) |
| **Informations sur le fichier…** | Détail du fichier, comme MediaInfo (voir [Informations sur le fichier](#informations-sur-le-fichier)) |
| **Lire en boucle** / **Lire une seule fois** | Boucle de l'élément |
| **Retirer** | Enlève l'élément de la liste |

Un élément réglé dans l'éditeur porte la marque **✎**.

## Éditer un élément

Clic droit → **Éditer…**, sur un élément du Lecteur comme sur un cue de la Cuelist. La fenêtre
reprend celle d'un jingle de JingleStar. Les réglages sont enregistrés avec le projet.

- **Volume** du son de l'élément, en dB, et lecture **une fois** ou **en boucle**.
- **Fondus au noir** : durée du fondu d'entrée (depuis le noir) et du fondu de sortie (vers le
  noir), image et son ensemble.
- **Frise** : les vignettes du média sur toute sa durée. Les poignées **entrée** et **sortie**
  se glissent pour couper le début et la fin ; les fondus se voient en voile sur la frise.
  Pendant le glissement, l'aperçu montre l'image à cet endroit.
- **Aperçu** : la même navigation que l'aperçu SUIVANT, pour vérifier un point d'entrée à
  l'image près.
- **Égaliseur** : coupe-bas (12 ou 24 dB/oct), grave, deux bandes en cloche, aigu, comme dans
  JingleStar. La forme d'onde du son s'affiche derrière la courbe.
- **Lire sur la sortie** lance l'élément pour l'essayer. Si l'élément est déjà à l'antenne,
  volume, égaliseur et fondus changent tout de suite, sans couper l'image.

### Images fixes

Pour une image, l'éditeur propose la **durée d'affichage** : **Jusqu'au média suivant** (l'image
reste jusqu'au prochain GO) ou **Pendant** un nombre de secondes, après quoi le Lecteur enchaîne.
Le fondu de sortie d'une image demande une durée d'affichage.

## Informations sur le fichier

Clic droit sur un élément du Lecteur ou un cue → **Informations sur le fichier…**. La fenêtre
détaille le conteneur, chaque piste vidéo, audio et de sous-titres : codec, profil, débit,
résolution, cadence, balayage, espace et plage de couleurs, canaux, échantillonnage, langue,
métadonnées.

- **Rechercher** filtre les lignes (par exemple « bit rate » ou « HDR »).
- **Tout afficher** passe à la vue complète de MediaInfo : tous les champs, sous toutes leurs
  formes.
- **Copier** met le rapport dans le presse-papiers, en texte, pour l'envoyer à un technicien.

Le rapport vient de la bibliothèque MediaInfo quand elle est installée (`MediaInfo.dll` dans le
dossier `mpv` de CueStar sous Windows). Sans elle, CueStar affiche ce que libmpv sait du fichier.
La lecture du fichier se fait dans un programme à part : un fichier abîmé ne fait pas planter
la régie.

## Le Diaporama

Lit un dossier de photos et de vidéos.

- **Dossier** : le dossier à lire ; **Relire** prend en compte les fichiers ajoutés ou retirés.
- **Inclure les sous-dossiers**, **Durée des photos**, **Ajustement** (image entière avec bandes
  noires, remplir l'écran en recadrant, ou automatique) et **Vidéos avec le son** (lues en
  entier).
- **Espace** lance ou met en pause, **← / →** passent au média précédent / suivant.
- **Aléatoire** remélange à chaque tour, **Tout boucler** reprend au début.

Les changements s'appliquent tout de suite, sans relancer le diaporama.

## La Cuelist

Une conduite de cues : chacun est un média, ou du **noir**. Le cue **en attente** part au GO.

- **Ajouter** : des médias, un dossier, ou **Un cue Noir** (coupe l'image avec la transition du
  cue).
- **GO** (ou **Espace**) lance le cue en attente et met le suivant en attente.
- **⏮ / ⏭** (ou **Flèche Haut / Bas**) changent le cue en attente sans le lancer.
- **Pause** (ou **P**) suspend et reprend le cue qui joue.
- Clic sur un cue : le met en attente. Double-clic : le lance. Glisser un cue change l'ordre.
- Clic droit : **Lancer maintenant**, **Réglages…**, **Informations sur le fichier…**, **Mettre
  en attente**, **Retirer de la conduite**.

**Réglages…** ouvre l'éditeur du cue : son nom, son média, sa **transition** (Coupe, Noir,
Fondu enchaîné), ce qui se passe **à la fin** (attendre le GO, ou enchaîner sur le cue
suivant), puis les mêmes réglages que l'[éditeur d'un élément](#éditer-un-élément).

## Le panneau Texte

La conduite écrite, à côté du module en cours : gras, italique, souligné, barré, couleurs,
taille. **Importer…** reprend un texte existant. Il est enregistré avec le projet.

## Sortie vidéo et son

**Video output** ouvre les réglages de la sortie :

- **Écran de sortie** : automatique (premier écran secondaire) ou un écran précis ; **Sortie
  dans une fenêtre** quand il n'y a qu'un écran.
- **Décodage** : automatique, GPU ou CPU. **Rendu de secours** et **Rendu léger** pour une
  petite machine.
- **Cadence** : recalage de vitesse ou synchro audio ; **Interpolation** lisse le rythme quand
  la cadence de la vidéo ne tombe pas juste sur celle de l'écran ; **Adapter la fréquence de
  l'écran à la vidéo** (Windows).
- **Sortie audio** : carte son, HDMI de l'écran de sortie, interface Dante… et son volume.
- **Mires de test** : barres SMPTE, mire animée, grille, blanc, gris.
- **Mapping 4 coins** : déplacez les coins de l'image pour la caler sur une surface ; flèches
  = 1 pixel, Maj + flèches = 10 pixels.

Les changements qui demandent de relancer la sortie l'annoncent : l'écran passe au noir un
instant.

## Projets et sauvegarde

- La session (listes, réglages, texte) est gardée en continu et retrouvée au lancement.
- **Ctrl+S** enregistre le projet, **Ctrl+Maj+S** l'enregistre ailleurs, **Ctrl+O** en ouvre
  un, **Ctrl+N** repart d'un projet vierge (après confirmation).
- Si des médias ont été déplacés, un bandeau le signale : **Relocaliser…** les cherche par leur
  nom dans un dossier.
- Les préférences de la machine (écran de sortie, décodage, son, télécommandes) ne voyagent pas
  avec le projet.

## Pilotage OSC

CueStar se pilote à distance en OSC (QLab, TouchOSC, Chataigne, Companion, un script…), avec les
mêmes réglages que JingleStar.

### Activer

Dans **Télécommandes**, onglet **OSC**, cochez **Activer**, réglez les options puis cliquez sur
**Appliquer** :

| Réglage | Par défaut | Rôle |
|---|---|---|
| Accès | Local | **Local** : seulement depuis ce PC ; **Réseau** : depuis les autres machines |
| Port (UDP) | 9000 | Port sur lequel CueStar reçoit les commandes |
| Préfixe | `/cuestar` | Début de toutes les adresses |
| OSCQuery | désactivé, port 9001 | Serveur HTTP qui décrit les adresses et l'état courant |
| Découverte | désactivée | Annonce en mDNS / Bonjour pour être trouvé automatiquement |
| Nom annoncé | CueStar | Nom visible sur le réseau |

Une fois appliqué, la fenêtre affiche l'adresse IP et les ports en écoute, ou l'erreur si un
port est déjà pris, puis le dernier message reçu.

### Adresses

Les numéros commencent à 1. Les adresses sont données avec le préfixe par défaut ; les adresses
du Diaporama d'origine (`/diaporama/…`) sont aussi acceptées.

| Adresse | Argument | Effet |
|---|---|---|
| `/cuestar/noir` | — | Écran de sortie au noir |
| `/cuestar/output/drops/reset` | — | Remet à zéro le compteur d'images perdues et retardées |
| `/cuestar/lecteur/go` | — | Lance l'élément sélectionné (ou reprend) |
| `/cuestar/lecteur/play` | — | Lecture |
| `/cuestar/lecteur/pause` | — | Pause |
| `/cuestar/lecteur/toggle` | — | Pause / reprise |
| `/cuestar/lecteur/stop` | — | Arrêt (noir) |
| `/cuestar/lecteur/next` | — | Élément suivant |
| `/cuestar/lecteur/prev` | — | Élément précédent |
| `/cuestar/lecteur/goto` | int | Lance l'élément n° |
| `/cuestar/lecteur/shuffle` | bool | Ordre aléatoire |
| `/cuestar/lecteur/loop` | bool | Tout boucler |
| `/cuestar/lecteur/transition` | `cut`, `black` ou `fade`, puis float (s) | Transition et durée |
| `/cuestar/cuelist/go` | — | Lance le cue en attente (ou reprend) |
| `/cuestar/cuelist/play`, `pause`, `toggle`, `stop` | — | Transport de la Cuelist |
| `/cuestar/cuelist/next`, `prev` | — | Cue en attente suivant / précédent |
| `/cuestar/cuelist/standby` | int | Met le cue n° en attente |
| `/cuestar/diaporama/play`, `pause`, `toggle`, `stop` | — | Transport du Diaporama |
| `/cuestar/diaporama/next`, `prev` | — | Média suivant / précédent |
| `/cuestar/diaporama/goto` | int | Média n° |
| `/cuestar/diaporama/settings/shuffle`, `loop`, `recursive`, `video_sound` | bool | Réglages du Diaporama |
| `/cuestar/diaporama/settings/photo_duration` | float (1 à 60 s) | Durée des photos |
| `/cuestar/diaporama/settings/fit` | `contain`, `cover` ou `auto` | Ajustement |
| `/cuestar/diaporama/settings/folder` | string | Dossier à lire |

- Les déclencheurs partent sans argument ou avec une valeur non nulle : un bouton qui envoie 1 à
  l'appui puis 0 au relâché ne déclenche qu'une fois.
- Les booléens acceptent `T`/`F`, un entier ou un float (≥ 0,5 = vrai), ou `true`/`false`/`on`/`off`.
- Les paquets groupés (bundles) sont acceptés.

### Retour d'état

`/cuestar/subscribe` (argument : port de réponse, facultatif) abonne l'expéditeur pendant 5 s :
à renouveler toutes les 2 à 3 s. CueStar lui renvoie `media`, `paused`, `time`, `remaining`,
`lecteur/playing`, `cuelist/standby`, `cuelist/standby_name` et `diaporama/index`.

### OSCQuery

- `http://<ip>:9001/` renvoie l'arborescence des adresses en JSON (types, accès, plages et
  valeurs courantes).
- `http://<ip>:9001/?HOST_INFO` renvoie les informations du serveur.
- `http://<ip>:9001/<chemin>?VALUE` renvoie la valeur d'un nœud.

En lecture seule, on y trouve aussi le média à l'écran, la position, le temps restant, le nombre
d'éléments et de cues, l'élément en cours et le nom du cue en attente.

### Exemple

```
oscsend localhost 9000 /cuestar/lecteur/goto i 3
oscsend localhost 9000 /cuestar/cuelist/go
oscsend localhost 9000 /cuestar/lecteur/transition sf black 1.5
curl http://localhost:9001/cuestar/cuelist/standby_name?VALUE
```

## Pilotage MIDI et consoles

**Télécommandes** relie des contrôleurs MIDI, une console Yamaha ou une Midas M32 / Behringer
X32 aux actions de CueStar, comme dans JingleStar :

1. Branchez la source dans l'onglet **MIDI**, **Yamaha** ou **Midas M32 / X32**.
2. Dans **Correspondances**, ajoutez une ligne, choisissez l'action, cliquez sur **Apprendre** puis
   touchez la note, le contrôleur ou la touche de la console.

Actions disponibles : GO, pause, arrêt, suivant, précédent, élément n° (selon la valeur reçue),
aléatoire et tout boucler pour le Lecteur ; lancer, pause, arrêt, suivant, précédent pour le
Diaporama ; GO, cue suivant, précédent, pause, arrêt pour la Cuelist ; **Noir** ; remise à zéro
du compteur d'images perdues.

## Raccourcis clavier et souris

La liste des touches est aussi dans **Télécommandes → Raccourcis**. Les touches seules
n'agissent pas pendant la saisie d'un texte.

| Action | Raccourci |
|---|---|
| Noir : couper la sortie | **Échap** |
| Ouvrir ou fermer l'aide | **F1** |
| GO du Lecteur ou de la Cuelist ; lancer / pause du Diaporama | **Espace** |
| Pause / reprise de la Cuelist | **P** |
| Élément ou cue précédent / suivant en attente | **Flèche Haut / Bas** |
| Média précédent / suivant du Diaporama | **Flèche Gauche / Droite** |
| Enregistrer / Enregistrer sous | **Ctrl+S** / **Ctrl+Maj+S** |
| Ouvrir un projet | **Ctrl+O** |
| Nouveau projet | **Ctrl+N** |
| Texte : gras / italique / souligné | **Ctrl+B** / **Ctrl+I** / **Ctrl+U** |
| Éditer un élément, informations sur le fichier | Clic droit sur l'élément |
| Lancer un élément ou un cue | Double-clic |
| Se déplacer dans l'aperçu | Clic ou glisser sur la barre sous l'image |
| Rechercher dans l'aide | Ctrl+F dans la fenêtre d'aide |

## En cas de problème

- **La sortie affiche une mire au lieu des médias** : libmpv est introuvable. Posez
  `libmpv-2.dll` à côté de CueStar ou dans son dossier `mpv`, puis relancez.
- **Sortie arrêtée** (voyant rouge) : **Relancer la sortie** dans la barre du bas.
- **Images perdues ou retardées qui montent** : essayez le décodage **GPU**, le **Rendu léger**
  ou **Adapter la fréquence de l'écran à la vidéo** dans **Video output** ; **Informations sur
  le fichier** montre si le média est très lourd (débit, résolution, 10 bits).
- **Pas de son** : choisissez la carte dans **Video output → Sortie audio**.
- **Médias introuvables** : **Relocaliser…** dans le bandeau.
- **Un comportement étrange ou un plantage** : le journal `cuestar.log`, dans le dossier de
  configuration, garde les erreurs et la trace des plantages.
- **L'OSC ne répond pas** : vérifiez que le pilotage est activé et appliqué, l'accès (Local ou
  Réseau), le port, le préfixe des adresses, et que le pare-feu laisse passer le port UDP.
