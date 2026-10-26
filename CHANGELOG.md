# Journal des modifications

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
