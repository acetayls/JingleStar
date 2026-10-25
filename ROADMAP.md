# Feuille de route

Idées notées pour plus tard, et ce qui en a déjà été développé.

## Fait

- [x] **Préférences** : le bouton **Appliquer** applique les réglages et ferme la fenêtre.
- [x] **Pause sur la playlist** : un bouton **⏸** dans le transport de la playlist ; la lecture
  s'arrête en micro-fondu et reprend au même endroit, sans clic, avec ⏸ ou GO. Aussi par OSC
  (`playlist/pause`, `playlist/paused`), MIDI et console (« Playlist : pause / reprise »).
- [x] **Module GO** : un type de module pour suivre une conduite de spectacle, façon QLab. Une
  liste ordonnée de cues lancés un par un avec un gros bouton **GO** (ou la barre d'espace, l'OSC
  et les télécommandes) ; le cue en attente est bien visible et passe au suivant après chaque GO.
  Précédent / suivant, arrêt de chaque cue, volume par cue, noms modifiables, ordre au
  glisser-déposer, commandes OSC (`go`, `cue/next`, `cue/previous`, `cue/standby`) et actions
  de télécommande.

## À faire

### Module GO : suite

- Notes par cue (texte affiché sous le cue en attente).
- Fondus d'entrée et de sortie par cue, et cues enchaînés automatiquement (« auto-continue »).
- Option « GO arrête les cues en cours » pour les enchaînements francs.

### Intégration Bitfocus Companion

Piloter JingleStar depuis Companion : un module Companion dédié (actions, retours d'état et
variables : cue en attente, morceau en cours, jingles qui jouent), ou au minimum des presets
OSC prêts à l'emploi qui s'appuient sur les adresses existantes et OSCQuery.

### Intégration Stream Deck

Des touches Stream Deck pour lancer jingles, GO et playlists, avec retour d'état sur les touches
(jingle qui joue, cue en attente, pause) : via Companion, ou avec un plugin Stream Deck dédié.
