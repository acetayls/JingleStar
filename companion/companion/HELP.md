## JingleStar

Pilote JingleStar depuis Companion (Stream Deck, etc.) : jingles, playlists, cuelist, volumes,
avec l'état de JingleStar affiché sur les touches.

### Réglages dans JingleStar

Préférences → Pilotage à distance : activer **OSC** et **OSCQuery**. Les ports par défaut
(OSC 8000, OSCQuery 8001) conviennent.

### Réglages dans Companion

- **Adresse IP** de l'ordinateur où tourne JingleStar (`127.0.0.1` si c'est le même).
- **Port OSCQuery** : 8001 par défaut. Le port OSC et le préfixe des adresses sont lus
  automatiquement auprès de JingleStar.

### Presets

Les presets suivent le projet ouvert dans JingleStar : une catégorie par module, avec les
12 jingles (nom, couleur, rouge quand il joue), le transport de playlist, le GO de la cuelist
avec le nom du cue en attente, et « Tout arrêter ».

### Variables utiles

- `module_1_jingle_3_name`, `module_1_jingle_3_remaining`
- `module_2_track`, `module_2_track_title`, `module_2_track_remaining`
- `module_3_cue_standby`, `module_3_cue_name`, `module_3_cue_playing_name`
