# Aide de JingleStar

JingleStar lance des jingles et des playlists sur une ou plusieurs sorties audio. L'écran est
découpé en **modules** (1 à 4) : chacun est soit une grille de 12 jingles, soit une playlist,
avec son propre volume, son égaliseur et sa paire de sorties.

## Prise en main

1. Au premier lancement, un seul module en mode **Jingles** est affiché.
2. Cliquez sur un bouton vide : une fenêtre propose de choisir un fichier son. Vous pouvez aussi
   **glisser-déposer** un ou plusieurs fichiers sur un bouton (plusieurs fichiers remplissent les
   boutons suivants).
3. Cliquez sur un bouton rempli pour lancer le son, cliquez à nouveau pour l'arrêter.
4. **Échap** ou le bouton rouge **TOUT ARRÊTER** coupe tous les sons.
5. Vérifiez la sortie audio dans la barre du bas. Pour en changer : **Préférences → Audio**.
6. À droite de la barre du bas, **● Tout est chargé** (vert) indique que tous les sons des jingles
   et des cues sont décodés en mémoire et partiront instantanément. Pendant le chargement, le voyant
   est orange et compte les sons prêts ; **⚠** (rouge) signale un fichier introuvable ou illisible
   (son nom s'affiche au survol). À côté, la mémoire occupée par les sons décodés (environ 23 Mo
   par minute de son).

Tout est enregistré automatiquement : en relançant JingleStar, vous retrouvez la session telle
que vous l'avez laissée.

Formats lus : WAV, AIFF, MP3, FLAC, OGG/Vorbis, AAC/M4A.

## L'écran

### Barre du haut

- **＋ Ajouter un module** : ajoute un module à la fin (4 au maximum). Le compteur indique le nombre de modules.
- Les modules sont côte à côte sur une rangée. **Glissez la séparation** entre deux modules pour régler leur largeur ; un **double-clic** sur une séparation remet toutes les largeurs à égalité. Les largeurs sont enregistrées avec le projet.
- **Nouveau** : projet vierge avec un module **Jingles** et un module **Playlist**. Un second clic sur **Remplacer le projet ?** confirme (**Annuler** sinon) ; les préférences (audio, télécommandes, langue) sont gardées. Enregistrez d'abord le projet en cours si vous voulez le retrouver.
- **Ouvrir…** / **Enregistrer sous…** : ouvre ou enregistre un projet (fichier `.jinglestar`).
- **Télécommandes** : pilotage OSC, MIDI, console Yamaha et Midas M32, correspondances et mode apprendre. Voir [Pilotage MIDI](#pilotage-midi), [Pilotage depuis une console Yamaha](#pilotage-depuis-une-console-yamaha) et [Pilotage depuis une Midas M32 / Behringer X32](#pilotage-depuis-une-midas-m32--behringer-x32).
- **⚙ Préférences** : audio, langue et anti-veille.
- **? Aide** (touche **F1**) : ce manuel. Le champ **🔍 Rechercher** en haut (ou **Ctrl+F**) n'affiche que les passages qui contiennent les mots tapés, sans tenir compte des accents ni des majuscules ; **✕** revient au sommaire.
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
- le choix du mode : **Jingles**, **Playlist**, **Cuelist** ou **Texte** ;
- à droite, le **fader** de volume (-∞ à +10 dB) avec des repères à +3, 0 et -6 dB et sa valeur en dB en dessous, le **vumètre** stéréo et le bouton **■** qui arrête le module.

Le cadre du module devient vert quand un son y joue.

- **Volume** : glissez le fader. Un **double-clic** sur le fader le remet à 0 dB ; un **clic sur la valeur en dB** permet de la taper au clavier (Entrée valide, Échap annule). Un nombre tapé sans signe est négatif : `6` donne -6 dB ; tapez `+3` pour monter au-dessus de 0 dB.
- **Changer de mode** : cliquez sur **Jingles**, **Playlist**, **Cuelist** ou **Texte**. Le module s'arrête ; les jingles, la playlist, la conduite et le texte sont conservés.
- **Renommer** : **double-clic sur le nom** du module, tapez le nouveau nom puis Entrée (Échap annule). En mode édition (✏), le nom est aussi modifiable directement.
- **Module Texte** : un grand champ libre pour écrire sa conduite, sauvegardé avec le projet. Échap y arrête quand même les jingles.
- **Supprimer** : en mode édition, cliquez sur **🗑** puis confirmez avec **Supprimer ?**. Les modules suivants remontent d'un rang. Le dernier module restant ne peut pas être supprimé.

### Mode édition (✏)

Le crayon bascule le module entre le mode **lecture** et le mode **édition** :

- en lecture, un clic sur un jingle le lance ou l'arrête ;
- en édition, un clic sur un jingle ouvre son éditeur (et propose un fichier si le bouton est vide), le nom du module devient modifiable, et les boutons **EQ** et **🗑** apparaissent ; en mode Playlist, les réglages de transition et d'équilibrage du niveau sont aussi affichés.

En fermant l'éditeur d'un jingle (**✓ Terminé** ou la croix), le module repasse en mode lecture.

## Les jingles

Un module en mode **Jingles** offre 12 boutons (3 colonnes, 4 rangées). Chaque bouton montre son
numéro, le nom du son, sa couleur et sa durée. Pendant la lecture, une barre de progression
indique le temps écoulé et le temps restant ; un jingle en boucle affiche **BOUCLE** à la place du
temps restant. Le jingle qui joue est entouré d'un cadre vert épais. Le volume du jingle est affiché en haut à droite (en orange s'il n'est pas à
0 dB). Des repères complètent le bouton : en haut à gauche, après le numéro, un badge orange **⟲** pour un jingle en boucle et **EQ** si son égaliseur est actif ; **…** pendant le chargement du son et
**⚠** si le fichier est introuvable ou illisible.

- **Clic** : lance le son depuis son point de début ; un second clic l'arrête avec son fondu de sortie (limité à 1 s pour un jingle joué une fois, complet pour un jingle en boucle ; une boucle arrêtée pendant son intro, avant d'avoir atteint la boucle, coupe tout de suite).
- Plusieurs jingles d'un même module peuvent jouer en même temps.
- **Clic droit** : menu **Choisir un fichier…**, **Éditer…** (ouvre l'éditeur sans passer en mode édition) et **Vider**.
- **Glisser-déposer** un fichier sur un bouton remplace son son ; plusieurs fichiers remplissent les boutons suivants du module.

### Éditer un jingle

Ouvrez l'éditeur par le mode édition (✏ puis clic sur le bouton) ou par **clic droit → Éditer…**.

- **Nom** : le texte affiché sur le bouton (par défaut, le nom du fichier).
- **Fichier** : **Changer…** pour choisir un autre son.
- **Volume** : de -40 à +10 dB, par pas de 0,5 dB.
- **Lecture** : **Une fois** ou **En boucle**.
- **Fondus** : durée du fondu d'**entrée** et de **sortie**, de 0 à 30 s.
  - Joué **une fois** : le son monte au départ et descend avant sa fin. Arrêté avant la fin, il s'éteint avec son fondu de sortie, limité à 1 s.
  - **En boucle** : le fondu d'entrée ne joue qu'au premier départ, pas à chaque tour. Le fondu de sortie ne sert qu'à l'arrêt, et en entier : c'est ainsi qu'on fait disparaître lentement un fond sonore. Arrêté pendant l'intro (avant le début de la boucle), il coupe tout de suite.
  - **Échap**, **TOUT ARRÊTER** et le **■** du module coupent toujours en 1 s au plus.
- **Couleur** : 8 couleurs au choix.
- **Écouter / Stop** : le gros bouton vert en haut de l'éditeur, pour tester le son.
- **Vider l'emplacement** : retire le son du bouton.
- **✓ Terminé** : ferme l'éditeur.

Les changements s'appliquent immédiatement, même pendant la lecture. Si la fenêtre est plus haute que l'écran, une barre de défilement apparaît à droite.

### Forme d'onde, début, fin et boucle

L'éditeur affiche la forme d'onde du son avec des poignées à faire glisser, façon QLab :

- poignée **verte** (en bas) : **début** de la lecture ;
- poignée **rouge** (en bas) : **fin** de la lecture ;
- traits **orange** (en haut), en mode boucle : **début** et **fin de la boucle**.

Les **fondus** d'entrée et de sortie se règlent avec les **carrés blancs** dans la bande au-dessus
de la forme d'onde : glissez-les vers l'intérieur pour allonger le fondu. La durée s'affiche à côté
et la forme d'onde est dessinée **atténuée** sur toute la durée du fondu, comme on l'entendra, avec en
transparence le son d'origine (en boucle, le fondu de sortie ne sert qu'à l'arrêt et se règle
dans le champ).

**Zoom** : la molette (ou Ctrl + molette, ou le pincement du pavé tactile) zoome autour du
pointeur ; le défilement horizontal ou un glisser hors des poignées déplace la vue ; un double-clic
ou **Tout afficher** revient au son entier. Zoomé, une **barre de défilement** sous la forme d'onde
montre la partie visible : glissez-la pour vous promener dans le son, ou cliquez à côté pour y aller.

Pendant la lecture (bouton du jingle ou **Écouter**), un **clic dans la forme d'onde**, hors des
poignées, fait repartir la lecture à cet endroit, avec un micro-fondu sans clic.

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

Un module en mode **Playlist** lit une liste de fichiers audio : un dossier entier, trié par nom,
ou des fichiers choisis un par un.

- **＋ Ajouter** :
  - **Des fichiers…** ou **Un dossier…** : ajoute les sons à la fin de la liste en cours ;
  - **Ouvrir un dossier (remplace la liste)…** : la liste devient le contenu du dossier.
  Vous pouvez aussi glisser des fichiers ou un dossier sur la playlist.
- **⟳** : relit le dossier (après y avoir ajouté ou retiré des fichiers).
- **🗑 Vider** : retire tous les morceaux, après confirmation (**Vider ?** pour confirmer, **Annuler** sinon). Les fichiers ne sont pas effacés.
- Le cadre du haut affiche le morceau en cours, une barre de progression, le temps écoulé et le temps restant.

### Transport

- **⏮** / **⏭** : morceau précédent / suivant (la lecture continue si elle était en cours).
- **▶ GO** : lance la lecture au morceau sélectionné, ou au premier. En pause, reprend la lecture. Après un **■**, relance le morceau arrêté depuis le début ; quand la liste est finie, repart du premier morceau. Le son est préparé à l'avance : le départ est immédiat.
- **⏸** : pause. Le morceau s'éteint en 10 ms et reprend exactement au même endroit, sans clic, avec ⏸ ou GO. Le bouton reste allumé en ambre et « EN PAUSE » s'affiche tant que la pause dure.
- **■** : arrête la playlist.
- **Barre de lecture** : un clic ou un glisser déplace la lecture dans le morceau en cours (appliqué au relâché, avec un micro-fondu pour éviter les clics).
- **Clic** sur un morceau : le sélectionne (à l'arrêt). **Double-clic** : le lance immédiatement.
- **Gain d'un morceau** : la valeur en dB à droite de chaque morceau se règle à la main par un
  **clic** (Entrée valide, Échap annule). Un gain manuel s'affiche en bleu et remplace
  l'équilibrage automatique ; le morceau en cours le prend tout de suite. Clic droit sur le morceau →
  **Revenir au gain automatique**.

### Options

Les morceaux s'enchaînent toujours automatiquement. Un voyant ambre indique les options actives.

- **Aléatoire** : ordre de lecture au hasard, sans répéter un morceau avant d'avoir joué toute la liste.
- **Tout boucler** : reprend au début à la fin de la liste.

### Transition

La ligne **Transition** propose :

- **Fin de fichier** : le morceau suivant démarre exactement à la fin du précédent, sans blanc ;
- **Fondu enchaîné** : le suivant démarre en fondu pendant que le précédent s'efface, sur une durée réglable de 0,5 à 20 s (3 s par défaut).

### Équilibrage du niveau

Pour que tous les morceaux sonnent au même niveau, cochez **Équilibrer le niveau** sous la
transition. La cible et le mode de mesure se règlent en mode édition (✏). Chaque morceau est mesuré en arrière-plan, puis lu avec le gain qui l'amène à la cible.
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

## La Cuelist (conduite)

Le mode **Cuelist** suit une conduite de spectacle, à la façon de QLab : une liste ordonnée de sons
(les **cues**) que l'on lance un par un.

- **Ajouter des sons** : **＋ Ajouter des sons…**, ou glissez des fichiers ou des dossiers sur la
  conduite. Un morceau de playlist peut aussi y être glissé, à l'endroit voulu.
- **EN COURS** : le cadre vert, en haut, suit le dernier cue lancé qui joue encore : son nom,
  le temps écoulé, une barre de progression et le temps restant.
- **EN ATTENTE** : le cadre ambre affiche le cue que le prochain GO va lancer, et celui qui suit.
  Dans la liste, il est surligné et marqué ▶.
- **GO** (ou la **barre d'espace**) : lance le cue en attente et met le suivant en attente.
  Les cues lancés continuent de jouer ensemble jusqu'à leur fin. Le délai anti double-GO des
  préférences protège aussi ce bouton. Après le dernier cue, le cadre affiche **Fin de la
  conduite** et GO s'éteint.
- **Précédent** / **Suivant** : déplacent le cue en attente sans rien lancer. Un **clic** sur un
  cue le met en attente.
- **Arrêter** : arrête tous les cues de la conduite ; le cue en attente ne change pas. Pendant
  qu'un cue joue, sa ligne se remplit en vert, affiche le temps restant et un bouton **■** qui
  n'arrête que lui.
- **Enchaîner ou arrêter** : la petite flèche à côté du numéro d'un cue. **↓** (orange) :
  à la fin du cue, le suivant part tout seul et le cue en attente avance. **·** : à la fin, on
  attend le prochain GO. Un clic bascule de l'un à l'autre. Un cue arrêté à la main (■, Arrêter,
  Échap) n'enchaîne pas.
- **Réglages d'un cue** : **double-clic** sur le cue, ou **clic droit → Réglages…**. La fenêtre
  est celle d'un jingle : volume, fondus d'entrée et de sortie, points de début et de fin sur la
  forme d'onde (clic dans la forme d'onde pendant la lecture pour s'y déplacer), égaliseur, et le
  choix **À la fin : Arrêter / Enchaîner**. Pas de boucle pour les cues pour l'instant. Un badge
  **EQ** sur la ligne signale un égaliseur actif.
- **Volume** : un **clic** sur la valeur en dB d’un cue permet de la taper (Entrée valide ; sans signe, la valeur est négative).
- **Ordre** : glissez un cue vers le haut ou le bas ; un trait bleu montre où il sera posé. Le
  cue en attente reste le même son.
- **Clic droit** sur un cue : **Lancer maintenant** (sans changer le cue en attente), **Réglages…**, **Mettre en
  attente**, **Retirer de la conduite**.
- En **mode édition** (✏), les noms des cues se modifient directement et **🗑** vide la conduite.

La barre d'espace pilote la première Cuelist de l'écran (sauf pendant la saisie d'un texte).
Les télécommandes ont les actions **Conduite : GO**, **Conduite : cue suivant** et **Conduite :
cue précédent**, et l'OSC les adresses `go` et `cue/…` (voir [Pilotage OSC](#pilotage-osc)).

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
et **Q / pente** à glisser ou à saisir. Un clic (ou double-clic) sur une valeur permet de la taper au clavier : Entrée valide, Échap annule. Modifier une
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

**⚙ Préférences** ouvre la fenêtre des réglages. Les réglages **Audio** prennent effet
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
- **Empêcher la mise en veille pendant la lecture** : tant qu'un son joue (ou est en pause), l'ordinateur et l'écran ne se mettent pas en veille. Sous Windows, JingleStar le demande au système (visible avec `powercfg /requests`) ; sous macOS, par une assertion d'alimentation (visible avec `pmset -g assertions`). Un portable qu'on referme se met quand même en veille.

En bas de la fenêtre, la **version de build** (version, commit et date de compilation) permet de
vérifier quelle mise à jour est installée ; un clic la copie.

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

Dans **Télécommandes**, section **OSC** en haut de la fenêtre, cochez **Activer le pilotage OSC**, réglez les options puis cliquez sur
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

Les numéros de module (1 à 4), de jingle (1 à 12) et de morceau commencent à 1. Les adresses sont
données avec le préfixe par défaut.

| Adresse | Argument | Effet |
|---|---|---|
| `/jinglestar/stop_all` | — | Arrête tout |
| `/jinglestar/module/<m>/stop` | — | Arrête le module |
| `/jinglestar/module/<m>/volume` | float, dB (-60 à +10) | Volume du module |
| `/jinglestar/module/<m>/mode` | `jingle`, `playlist`, `text` ou `go` | Change le mode du module |
| `/jinglestar/module/<m>/jingle/<j>/play` | — | Lance le jingle depuis le début (le relance s'il joue) |
| `/jinglestar/module/<m>/jingle/<j>/stop` | — | Arrête le jingle |
| `/jinglestar/module/<m>/jingle/<j>/toggle` | — | Lance ou arrête, comme un clic |
| `/jinglestar/module/<m>/jingle/<j>/volume` | float, dB (-40 à +10) | Volume du jingle |
| `/jinglestar/module/<m>/playlist/play` | — ou int | Lecture (au morceau sélectionné, ou au morceau n°) |
| `/jinglestar/module/<m>/playlist/track` | int | Lance le morceau n° |
| `/jinglestar/module/<m>/playlist/stop` | — | Arrête la playlist |
| `/jinglestar/module/<m>/playlist/pause` | — | Pause / reprise |
| `/jinglestar/module/<m>/playlist/paused` | bool | Met en pause (1) ou reprend (0) |
| `/jinglestar/module/<m>/playlist/next` | — | Morceau suivant |
| `/jinglestar/module/<m>/playlist/previous` | — | Morceau précédent |
| `/jinglestar/module/<m>/playlist/shuffle` | bool | Aléatoire |
| `/jinglestar/module/<m>/playlist/loop` | bool | Boucle sur la liste |
| `/jinglestar/go` | — | GO de la première Cuelist |
| `/jinglestar/module/<m>/go` | — | GO : lance le cue en attente, passe au suivant |
| `/jinglestar/module/<m>/cue/next` | — | Cue suivant en attente |
| `/jinglestar/module/<m>/cue/previous` | — | Cue précédent en attente |
| `/jinglestar/module/<m>/cue/standby` | int | Met le cue n° en attente |

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

**Apprendre** sur une ligne existante la réaffecte. Le bouton **✏** à côté de la valeur apprise
permet de la corriger à la main : type (note, CC, program change, touche ou scène de console),
canal, numéro de note ou de contrôle, voie de console. On peut ainsi créer une correspondance sans
avoir le contrôleur sous la main.

 Une nouvelle ligne reprend l'action de la
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
[Console en exploitation](#console-en-exploitation)).

### Se connecter

Dans **Télécommandes**, section **Console Yamaha (réseau)**, saisissez l'**IP** de la console
(réglée sur la console dans SETUP → NETWORK), laissez le port **49280** et cliquez sur
**Connecter**. Si la console est éteinte ou débranchée, JingleStar retente seul toutes les
quelques secondes. La ligne **Reçu** montre le dernier message envoyé par la console et depuis
combien de temps.

### Console en exploitation

JingleStar n'écrit **aucun octet** vers une console Yamaha : il ouvre la connexion et ne fait que
lire. Aucune commande n'existe dans JingleStar pour ces consoles, ni modification (`set`, rappel
de scène…) ni même demande d'information.

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
du canal 32) ou **Console · Scène 12**.

## Pilotage depuis une Midas M32 / Behringer X32

JingleStar écoute aussi les consoles Midas M32 et Behringer X32 sur le réseau (OSC, port UDP
**10023**), avec le même tableau de correspondances et le même mode apprendre.

### Se connecter

Dans **Télécommandes**, section **Console Midas M32 / Behringer X32**, saisissez l'**IP** de la
console (SETUP → Network sur la console), laissez le port **10023** et cliquez sur **Connecter**.
Le modèle s'affiche dès que la console répond, et la ligne **Reçu** montre son dernier message.

### Ce qui est envoyé à la console

Contrairement aux Yamaha, la M32 n'envoie ses changements qu'aux clients qui s'y abonnent, et
l'abonnement dure 10 secondes. Une écoute sans aucun envoi est donc impossible. JingleStar
envoie uniquement :

- `/xremote` toutes les 8 secondes : l'abonnement aux changements ;
- `/info` en même temps : le nom du modèle et la version.

Ces deux requêtes n'ont aucun argument et ne modifient rien sur la console. JingleStar ne
contient aucune autre commande pour la M32. Comme pour les Yamaha, donnez à l'ordinateur une
adresse IP libre sur le réseau de la console.

### Ce qui peut déclencher une action

- **Touche MUTE d'un canal, d'un bus ou d'un DCA** (paramètre `…/mix/on`) : chaque appui
  déclenche l'action, comme le ON des Yamaha. Les **groupes de mute** (`/config/mute/1` à 6)
  marchent de la même façon. Un canal libre devient ainsi un bouton.
- **Fader** : règle un volume. La position est convertie en dB selon la loi de la M32
  (0 dB sur la console = 0 dB dans JingleStar), avec la reprise en douceur.
- **Rappel de scène** : déclenche l'action de la scène apprise ; avec **Playlist : morceau n°**,
  la scène 1 lance le morceau 1, etc.

Le déclencheur appris s'affiche par exemple **M32 · ch/05/mix/on** (touche MUTE du canal 5).

## Raccourcis clavier et souris

| Action | Raccourci |
|---|---|
| Tout arrêter | **Échap** (sauf pendant la saisie d'un texte) |
| Ouvrir ou fermer l'aide | **F1** |
| GO de la première Cuelist | **Espace** (sauf pendant la saisie d'un texte) |
| Changer l'ordre de la conduite | Glisser un cue |
| Lancer / arrêter un jingle | Clic sur le bouton (mode lecture) |
| Menu d'un jingle | Clic droit sur le bouton |
| Remplir des boutons | Glisser-déposer un ou plusieurs fichiers |
| Ajouter à une playlist | Glisser-déposer des fichiers ou un dossier sur la playlist |
| Lancer un morceau de la playlist | Double-clic sur le morceau |
| Volume du module à 0 dB | Double-clic sur le fader |
| Taper une valeur en dB (fader, jingle, morceau) | Clic sur la valeur, puis Entrée (Échap annule) ; sans signe = négatif |
| Largeur des modules | Glisser la séparation ; double-clic pour égaliser |
| Rechercher dans l'aide | Ctrl+F dans la fenêtre d'aide |
| Renommer un module | Double-clic sur son nom |
| Zoomer sur la forme d'onde | Molette, glisser pour se déplacer, double-clic pour tout afficher |
| Se déplacer dans un morceau de playlist | Clic ou glisser sur la barre de lecture |
| Largeur (Q) d'une bande d'EQ | Molette sur le point de la courbe |
| Activer / désactiver une bande d'EQ | Double-clic sur le point de la courbe |
| Valeur précise dans un champ numérique | Clic sur le champ, saisie au clavier, puis Entrée (ou glisser pour ajuster) |

## En cas de problème

- **Aucune sortie audio** (voyant rouge) : ouvrez **Préférences → Audio**, choisissez un périphérique, cliquez sur **Rafraîchir la liste** si besoin, puis **Appliquer**.
- **Craquements** : augmentez la **taille de buffer** (512 ou 1024 échantillons).
- **Un bouton affiche une erreur** : le fichier a été déplacé, renommé, ou son format n'est pas lu. Rechoisissez le fichier.
- **Un module ne sort pas où il faut** : vérifiez son menu **Sortie** ; un ⚠ orange signale une paire absente de la carte en service.
- **L'OSC ne répond pas** : vérifiez que le pilotage est activé et appliqué, que le port affiché correspond à celui de l'émetteur, le préfixe des adresses, et que le pare-feu laisse passer le port UDP.
