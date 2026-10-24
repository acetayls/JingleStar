# Aide de JingleStar

JingleStar lance des jingles et des playlists sur une ou plusieurs sorties audio. L'écran est
découpé en **modules** (1 à 8) : chacun est soit une grille de 12 jingles, soit une playlist,
avec son propre volume, son égaliseur et sa paire de sorties.

## Prise en main

1. Au premier lancement, un seul module en mode **Jingles** est affiché.
2. Cliquez sur un bouton vide : une fenêtre propose de choisir un fichier son. Vous pouvez aussi
   **glisser-déposer** un ou plusieurs fichiers sur un bouton (plusieurs fichiers remplissent les
   boutons suivants).
3. Cliquez sur un bouton rempli pour lancer le son, cliquez à nouveau pour l'arrêter.
4. **Échap** ou le bouton rouge **TOUT ARRÊTER** coupe tous les sons.
5. Vérifiez la sortie audio dans la barre du bas. Pour en changer : **Préférences → Audio**.

Tout est enregistré automatiquement : en relançant JingleStar, vous retrouvez la session telle
que vous l'avez laissée.

Formats lus : WAV, AIFF, MP3, FLAC, OGG/Vorbis, AAC/M4A.

## L'écran

### Barre du haut

- **＋ Ajouter un module** : ajoute un module à la fin (8 au maximum). Le compteur indique le nombre de modules.
- **⤢ Remplir** : les modules se partagent la largeur de la fenêtre (4 par rangée au plus). Décochez-le pour régler une largeur fixe avec le curseur qui apparaît ; il y a alors autant de modules par rangée qu'il en tient.
- **Ouvrir…** / **Enregistrer sous…** : ouvre ou enregistre un projet (fichier `.jinglestar`).
- **Télécommandes** : pilotage MIDI et console Yamaha, correspondances et mode apprendre. Voir [Pilotage MIDI](#pilotage-midi) et [Pilotage depuis une console Yamaha](#pilotage-depuis-une-console-yamaha).
- **⚙ Préférences** : audio, langue, anti-veille et OSC.
- **? Aide** : ouvre cette aide (touche **F1**).
- **■ TOUT ARRÊTER** : arrête tous les sons de tous les modules (touche **Échap**).

### Barre du bas

Elle affiche la sortie audio en service (pilote, périphérique, nombre de canaux, fréquence
d'échantillonnage) avec un voyant vert, ou un voyant rouge s'il n'y a aucune sortie. Les erreurs
et les messages (projet enregistré, fichier illisible…) s'y affichent aussi.

## Les modules

Chaque module a, de haut en bas :

- son **numéro** et son **nom** ;
- le choix de **sortie** (paire stéréo 1-2, 3-4…) ;
- le bouton **EQ** (égaliseur du module) ;
- le bouton **✏** (mode édition) ;
- le choix du mode : **Jingles** ou **Playlist** ;
- à droite, le **fader** de volume (-∞ à +10 dB) avec sa valeur en dB en dessous, le **vumètre** stéréo et le bouton **■** qui arrête le module.

Le cadre du module devient vert quand un son y joue.

- **Volume** : glissez le fader. Un **double-clic** le remet à 0 dB.
- **Changer de mode** : cliquez sur **Jingles** ou **Playlist**. Le module s'arrête ; les jingles et la playlist sont conservés tous les deux.
- **Renommer** : passez en mode édition (✏), le nom devient modifiable.
- **Supprimer** : en mode édition, cliquez sur **🗑** puis confirmez avec **Supprimer ?**. Les modules suivants remontent d'un rang. Le dernier module restant ne peut pas être supprimé.

### Mode édition (✏)

Le crayon bascule le module entre le mode **lecture** et le mode **édition** :

- en lecture, un clic sur un jingle le lance ou l'arrête ;
- en édition, un clic sur un jingle ouvre son éditeur (et propose un fichier si le bouton est vide), le nom du module devient modifiable, et les boutons **EQ** et **🗑** apparaissent ; en mode Playlist, les réglages de transition et d'équilibrage du niveau sont aussi affichés.

En fermant l'éditeur d'un jingle (**✓ Terminé** ou la croix), le module repasse en mode lecture.

## Les jingles

Un module en mode **Jingles** offre 12 boutons (3 colonnes, 4 rangées). Chaque bouton montre son
numéro, le nom du son, sa couleur et sa durée. Pendant la lecture, une barre de progression
indique le temps écoulé et le temps restant ; un jingle en boucle affiche **⟳** à la place du
temps restant. Le volume du jingle est affiché en haut à droite (en orange s'il n'est pas à
0 dB). Des repères complètent le bouton : **🔁** à côté du volume pour un jingle en boucle, **EQ** en bas à gauche si son égaliseur est actif, **…** pendant le chargement du son et
**⚠** si le fichier est introuvable ou illisible.

- **Clic** : lance le son depuis son point de début ; un second clic l'arrête (avec son fondu de sortie s'il en a un).
- Plusieurs jingles d'un même module peuvent jouer en même temps.
- **Clic droit** : menu **Choisir un fichier…**, **Éditer…** (ouvre l'éditeur sans passer en mode édition) et **Vider**.
- **Glisser-déposer** un fichier sur un bouton remplace son son ; plusieurs fichiers remplissent les boutons suivants du module.

### Éditer un jingle

Ouvrez l'éditeur par le mode édition (✏ puis clic sur le bouton) ou par **clic droit → Éditer…**.

- **Nom** : le texte affiché sur le bouton (par défaut, le nom du fichier).
- **Fichier** : **Changer…** pour choisir un autre son.
- **Volume** : de -40 à +10 dB, par pas de 0,5 dB.
- **Lecture** : **Une fois** ou **En boucle**.
- **Fondus** : durée du fondu d'**entrée** et de **sortie**, de 0 à 30 s. Le fondu de sortie s'applique aussi quand on arrête le son.
- **Couleur** : 8 couleurs au choix.
- **▶ Écouter / ■ Stop** : pour tester le son depuis l'éditeur.
- **Vider l'emplacement** : retire le son du bouton.
- **✓ Terminé** : ferme l'éditeur.

Les changements s'appliquent immédiatement, même pendant la lecture.

### Forme d'onde, début, fin et boucle

L'éditeur affiche la forme d'onde du son avec des poignées à faire glisser, façon QLab :

- poignée **verte** (en bas) : **début** de la lecture ;
- poignée **rouge** (en bas) : **fin** de la lecture ;
- traits **orange** (en haut), en mode boucle : **début** et **fin de la boucle**.

Les parties exclues sont assombries et les fondus sont dessinés en rampe. Le début et la fin se
saisissent dans la moitié basse de la forme d'onde, les points de boucle dans la moitié haute : on
peut ainsi les séparer même s'ils sont superposés.

Sous la forme d'onde, les champs **Début**, **Boucle → …** et **Fin** permettent de saisir les
points au centième de seconde. **Réinitialiser les points** revient au son entier.

En boucle, le son joue du début jusqu'à la fin de la boucle, repart au début de la boucle, et ainsi
de suite jusqu'à ce qu'on l'arrête.

### Égaliseur du jingle

En bas de l'éditeur, la section **Égaliseur** (dépliable) règle un égaliseur propre à ce jingle.
Elle s'affiche en orange avec la mention « actif » quand il modifie le son. Voir
[L'égaliseur](#légaliseur).

## La playlist

Un module en mode **Playlist** lit les fichiers audio d'un dossier, triés par nom.

- **📁 Dossier…** : choisit le dossier. Vous pouvez aussi glisser un dossier (ou un fichier de ce dossier) sur la playlist.
- **⟳** : relit le dossier (après y avoir ajouté ou retiré des fichiers).
- Le cadre du haut affiche le morceau en cours, une barre de progression, le temps écoulé et le temps restant.

### Transport

- **⏮** / **⏭** : morceau précédent / suivant (la lecture continue si elle était en cours).
- **▶ GO** : lance la lecture au morceau sélectionné, ou au premier.
- **■** : arrête la playlist.
- **Clic** sur un morceau : le sélectionne (à l'arrêt). **Double-clic** : le lance immédiatement.

### Options

Les morceaux s'enchaînent toujours automatiquement. Un voyant ambre indique les options actives.

- **Aléatoire** : ordre de lecture au hasard, sans répéter un morceau avant d'avoir joué toute la liste.
- **Boucle** : reprend au début à la fin de la liste.

### Transition

En mode édition (ou quand le fondu enchaîné est choisi), la ligne **Transition** propose :

- **Fin de fichier** : le morceau suivant démarre exactement à la fin du précédent, sans blanc ;
- **Fondu enchaîné** : le suivant démarre en fondu pendant que le précédent s'efface, sur une durée réglable de 0,5 à 20 s (3 s par défaut).

### Équilibrage du niveau

Pour que tous les morceaux sonnent au même niveau, passez en mode édition et cochez **Équilibrer le
niveau**. Chaque morceau est mesuré en arrière-plan, puis lu avec le gain qui l'amène à la cible.
Les fichiers ne sont jamais modifiés.

- **Mode de mesure** : **Sonie (LUFS)** (sonie intégrée ITU-R BS.1770 / EBU R128, le plus proche de l'oreille), **Crête**, **Crête vraie** (dBTP) ou **RMS**.
- **Cible** : le niveau visé. Par défaut -16 LUFS ; repères : -23 LUFS pour la radio et la télévision (EBU R128), -14 à -16 LUFS pour le streaming et la sonorisation.
- **Max** (activé par défaut, -1 dBTP) : si le gain ferait dépasser ce plafond de crête vraie à un morceau, son gain est réduit pour ne jamais saturer.

Le gain appliqué s'affiche à droite de chaque morceau et à côté du morceau en cours : en gris
normalement, en **orange** s'il a été réduit par le plafond, **…** pendant la mesure, **—** pour un
morceau muet ou trop court (gain inchangé), **⚠** si la mesure a échoué. Au survol, une infobulle
donne les mesures détaillées (sonie, crête, crête vraie, RMS). Pendant les mesures, un indicateur
affiche leur avancement. Les mesures sont gardées en mémoire et refaites seulement si le fichier
change.

Hors mode édition, une ligne **Équilibrage** rappelle le réglage en cours.

## L'égaliseur

Chaque jingle et chaque module ont leur égaliseur. Celui du module s'applique à tout ce que le
module joue (jingles et playlist), après celui du jingle.

- **Égaliseur du module** : en mode édition, bouton **EQ** dans l'en-tête du module. Hors mode édition, une petite étiquette **EQ** orange signale qu'il est actif.
- **Égaliseur du jingle** : section **Égaliseur** de l'éditeur du jingle.

Il comporte cinq bandes :

| Bande | Réglages |
|---|---|
| Coupe-bas | fréquence, pente 12 ou 24 dB/oct |
| Shelf grave | fréquence, gain, Q |
| Param. 1 | fréquence, gain, Q |
| Param. 2 | fréquence, gain, Q |
| Shelf aigu | fréquence, gain, Q |

Le gain va de -18 à +18 dB, la fréquence de 20 Hz à 20 kHz, le Q de 0,1 à 18.

### À la souris, sur la courbe

- **Glisser un point** : déplace la fréquence (horizontalement) et le gain (verticalement), avec un léger cran à 0 dB.
- **Molette** sur un point : élargit ou resserre la bande (Q).
- **Double-clic** sur un point : active ou désactive la bande.

### Avec les champs

Sous la courbe, chaque bande a un bouton (activer / désactiver) et des champs **Fréq.**, **Gain**
et **Q / pente** à glisser ou à saisir. Un double-clic sur un gain le remet à 0 dB. Modifier une
valeur active la bande.

- **Égaliseur actif** : interrupteur général. Décoché, le son passe sans traitement mais les réglages sont gardés (la courbe reste visible, estompée).
- **Réinitialiser** : revient à un égaliseur neutre.

## Le routage des sorties

Chaque module sort sur une **paire stéréo** : 1-2, 3-4, 5-6… Le choix se fait dans le menu
**Sortie** de l'en-tête du module.

- Avec une carte son stéréo, seule la paire 1-2 existe : tous les modules y sont mixés.
- Avec une carte multicanal, donnez une paire différente à chaque module pour un multipiste (par exemple, jingles sur 1-2 et musique d'ambiance sur 3-4).
- Si la paire choisie n'existe pas sur le périphérique en service (carte débranchée, autre carte choisie), le menu affiche **⚠ Sortie x-y → 1-2** en orange : le module sort alors sur 1-2, et retrouvera sa paire quand elle sera de nouveau disponible.

## Les préférences

**⚙ Préférences** ouvre la fenêtre des réglages. Les réglages **Audio** et **OSC** prennent effet
avec **Appliquer**, qui ferme aussi la fenêtre ; la langue et l'anti-veille s'appliquent tout de suite.

### Audio

- **Pilote** : WASAPI (Windows), CoreAudio (macOS), ALSA (Linux), ou ASIO si JingleStar a été compilé avec cette option. « Par défaut » prend le pilote du système.
- **Périphérique** : la sortie à utiliser, avec son nombre de canaux. ★ marque la sortie par défaut du système.
- **Taille de buffer** : Automatique, ou de 64 à 2048 échantillons. Plus petit = moins de latence, mais plus de risque de craquements.
- **Rafraîchir la liste** : relit les pilotes et périphériques (après avoir branché une carte).

Appliquer un changement audio rouvre le périphérique, ce qui interrompt les sons en cours. Si
rien n'a changé dans l'audio, la lecture n'est pas interrompue.

### Système

- **Langue** : Français ou English. L'interface et cette aide changent immédiatement.
- **Empêcher la mise en veille pendant la lecture** : tant qu'un son joue, l'ordinateur et l'écran ne se mettent pas en veille.

### OSC

Voir [Pilotage OSC](#pilotage-osc).

## Projets et sauvegarde

- La session est **enregistrée automatiquement** et restaurée au lancement suivant (modules, jingles, playlists, préférences).
- **Enregistrer sous…** crée un fichier `.jinglestar` (un projet), pour garder plusieurs configurations.
- **Ouvrir…** charge un projet : tous les sons sont arrêtés, puis le projet remplace la session en cours.

Un projet garde le **chemin** des fichiers son, pas les sons eux-mêmes : si vous déplacez ou
renommez un fichier, le bouton correspondant signale une erreur. Rechoisissez le fichier
(**Changer…** ou clic droit → **Choisir un fichier…**).

## Pilotage OSC

JingleStar se pilote à distance en OSC (par exemple depuis QLab, TouchOSC, Chataigne, une surface
de contrôle ou un script).

### Activer

Dans **Préférences → OSC**, cochez **Activer le pilotage OSC**, réglez les options puis cliquez sur
**Appliquer** :

| Réglage | Par défaut | Rôle |
|---|---|---|
| Port UDP (réception) | 8000 | Port sur lequel JingleStar reçoit les commandes |
| Préfixe des adresses | `/jinglestar` | Début de toutes les adresses (peut être vide) |
| OSCQuery | activé, port 8001 | Serveur HTTP qui décrit les commandes disponibles |
| Découverte réseau | activée | Annonce en mDNS / Bonjour pour être trouvé automatiquement |
| Nom annoncé | JingleStar | Nom visible sur le réseau |

Une fois appliqué, les préférences affichent l'adresse IP et les ports en écoute, ou l'erreur si
un port est déjà pris.

### Adresses

Les numéros de module (1 à 8), de jingle (1 à 12) et de morceau commencent à 1. Les adresses sont
données avec le préfixe par défaut.

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

- Les déclencheurs (`play`, `stop`, `toggle`, `next`…) partent sans argument ou avec une valeur non nulle : un bouton qui envoie 1 à l'appui puis 0 au relâché ne déclenche qu'une fois.
- Les booléens acceptent `T`/`F`, un entier ou un float (≥ 0,5 = vrai), ou `true`/`false`/`on`/`off`.
- Les commandes de jingle n'agissent que sur un module en mode Jingles, celles de playlist sur un module en mode Playlist. Les paquets groupés (bundles) sont acceptés.

### OSCQuery

- `http://<ip>:8001/` renvoie l'arborescence des commandes en JSON (types, accès, plages et valeurs courantes).
- `http://<ip>:8001/?HOST_INFO` renvoie les informations du serveur.
- `http://<ip>:8001/<chemin>?VALUE` renvoie la valeur d'un nœud.

L'arborescence suit l'état du projet : nombre de modules, mode de chacun, nom des jingles. En
lecture seule, on y trouve aussi `name`, `jingle/<j>/name`, `jingle/<j>/playing`,
`playlist/playing` et `playlist/tracks`. Le service est annoncé en `_oscjson._tcp` et `_osc._udp` :
Chataigne, TouchOSC, Vezér ou Ossia Score le trouvent seuls. Les notifications en direct
(WebSocket `LISTEN`) ne sont pas proposées : les clients relisent les valeurs en HTTP.

### Exemple

```
oscsend localhost 8000 /jinglestar/module/1/jingle/3/play
oscsend localhost 8000 /jinglestar/module/2/volume f -6
curl http://localhost:8001/jinglestar/module/2/volume?VALUE
```

## Pilotage MIDI

JingleStar se pilote avec un clavier, un contrôleur à pads ou à faders, ou la sortie MIDI d'une
console (par exemple une Yamaha QL avec une interface USB-MIDI). Les notes, les CC et les
program change déclenchent les mêmes actions que l'OSC.

### Brancher

Cliquez sur **Télécommandes** dans la barre du haut, cochez **Activer l'entrée MIDI** et
choisissez le port (ou « Premier port disponible »). Les réglages s'appliquent tout de suite.
Si le contrôleur est débranché, JingleStar le reconnecte seul quand il revient ; **⟳** relit la
liste des ports.

### Apprendre une correspondance

1. Cliquez sur **＋ Ajouter et apprendre** : une ligne apparaît, en attente.
2. Appuyez sur la touche, le pad, ou bougez le fader du contrôleur : il est affecté à la ligne.
3. Choisissez l'action, le module et, pour un jingle, son numéro.

**Apprendre** sur une ligne existante la réaffecte. Une nouvelle ligne reprend l'action de la
précédente au jingle suivant : pour un pad de 12 touches, enchaînez **＋ Ajouter et apprendre**
puis la touche suivante. **Dernier message reçu** montre ce qui arrive, pour vérifier un
branchement.

### Comment les messages agissent

| Message | Boutons (lancer, arrêter, suivant…) | Volumes | Aléatoire, boucle | Morceau n° |
|---|---|---|---|---|
| Note | à l'appui (le relâché est ignoré) | vélocité | bascule | — |
| CC | quand la valeur passe à 64 ou plus | 0 → minimum, 127 → maximum | 64 ou plus = activé | valeur |
| Program change | à chaque message | — | bascule | numéro du program |

Pour **Playlist : morceau n°** sur un program change, tous les program change du canal appris
choisissent le morceau : program 1 → morceau 1, program 2 → morceau 2…

Les actions de jingle sont ignorées si le module est en mode playlist, et inversement.

### Reprise en douceur des volumes

Un fader (console ou contrôleur MIDI) affecté à un volume ne prend la main qu'une fois arrivé au
volume actuel de JingleStar, à 1 dB près ou en le franchissant : pas de saut de volume si les
deux ne sont pas au même endroit. Tant que ce n'est pas le cas, la ligne de la correspondance
affiche **amener le fader à … dB**. Si le volume est ensuite changé à la souris ou en OSC, le
fader doit le rejoindre de nouveau.

## Pilotage depuis une console Yamaha

JingleStar écoute directement une console Yamaha sur le réseau (QL, CL, TF, DM7, Rivage), sans
matériel en plus : branchez l'ordinateur sur le même réseau que la console. JingleStar ne fait
qu'**écouter** les changements de la console ; il ne modifie jamais rien sur celle-ci (voir
[Écoute stricte](#écoute-stricte-console-en-exploitation)).

### Se connecter

Dans **Télécommandes**, section **Console Yamaha (réseau)**, saisissez l'**IP** de la console
(réglée sur la console dans SETUP → NETWORK), laissez le port **49280** et cliquez sur
**Connecter**. Si la console est éteinte ou débranchée, JingleStar retente seul toutes les
quelques secondes. La ligne **Reçu** montre le dernier message envoyé par la console et depuis
combien de temps.

### Écoute stricte (console en exploitation)

**Écoute stricte**, cochée par défaut, garantit que JingleStar n'écrit **aucun octet** vers la
console : il ouvre la connexion et ne fait que lire. Décochée, JingleStar envoie seulement deux
requêtes de lecture, `devinfo productname` (le nom du modèle, affiché alors) à la connexion et
`devstatus runmode` toutes les 5 secondes, pour détecter une console éteinte. Aucune commande de
modification (`set`, rappel de scène…) n'existe dans JingleStar.

Avant de brancher l'ordinateur sur le réseau d'une console en service, donnez-lui une adresse IP
libre de ce réseau (une adresse déjà prise par la console, une tablette ou un rack perturberait
leur liaison) et branchez-le sur le port réseau de contrôle, jamais sur les ports Dante.

### Ce qui peut déclencher une action

- **Touche ON d'un canal** : chaque appui bascule le ON et déclenche l'action. Un canal libre
  (entrée inutilisée, par exemple placée dans une banque de faders personnalisée) devient ainsi
  un bouton. Si votre console permet d'affecter une touche **USER DEFINED** au ON de ce canal,
  cette touche devient un bouton de JingleStar.
- **Fader** : règle un volume de module (donc aussi celui de la playlist d'un module en mode
  playlist) ou de jingle. Le niveau en dB affiché sur la console est repris tel quel : 0 dB sur
  la console = 0 dB dans JingleStar, la courbe du fader Yamaha est donc respectée ; -∞ donne le
  minimum. Pour un bouton (lancer, arrêter…), l'action part quand le fader quitte -∞.
- **Rappel de scène** : déclenche l'action de la scène apprise. Avec **Playlist : morceau n°**,
  toute scène rappelée lance le morceau du même numéro.

Pour les options de playlist (aléatoire, boucle), le ON d'un canal impose l'état :
ON allumé = option activée.

### Apprendre

Comme en MIDI : **＋ Ajouter et apprendre**, puis appuyez sur le ON, bougez le fader ou rappelez la
scène sur la console. Le déclencheur s'affiche, par exemple **QL · InCh/Fader/On 32** (touche ON
du canal 32) ou **QL · Scène 12**.

## Raccourcis clavier et souris

| Action | Raccourci |
|---|---|
| Tout arrêter | **Échap** (sauf pendant la saisie d'un texte) |
| Ouvrir ou fermer l'aide | **F1** |
| Lancer / arrêter un jingle | Clic sur le bouton (mode lecture) |
| Menu d'un jingle | Clic droit sur le bouton |
| Remplir des boutons | Glisser-déposer un ou plusieurs fichiers |
| Choisir le dossier d'une playlist | Glisser-déposer un dossier sur la playlist |
| Lancer un morceau de la playlist | Double-clic sur le morceau |
| Volume du module à 0 dB | Double-clic sur le fader |
| Gain d'une bande d'EQ à 0 dB | Double-clic sur le champ Gain |
| Largeur (Q) d'une bande d'EQ | Molette sur le point de la courbe |
| Activer / désactiver une bande d'EQ | Double-clic sur le point de la courbe |
| Valeur précise dans un champ numérique | Clic sur le champ, saisie au clavier, puis Entrée (ou glisser pour ajuster) |

## En cas de problème

- **Aucune sortie audio** (voyant rouge) : ouvrez **Préférences → Audio**, choisissez un périphérique, cliquez sur **Rafraîchir la liste** si besoin, puis **Appliquer**.
- **Craquements** : augmentez la **taille de buffer** (512 ou 1024 échantillons).
- **Un bouton affiche une erreur** : le fichier a été déplacé, renommé, ou son format n'est pas lu. Rechoisissez le fichier.
- **Un module ne sort pas où il faut** : vérifiez son menu **Sortie** ; un ⚠ orange signale une paire absente de la carte en service.
- **L'OSC ne répond pas** : vérifiez que le pilotage est activé et appliqué, que le port affiché correspond à celui de l'émetteur, le préfixe des adresses, et que le pare-feu laisse passer le port UDP.
