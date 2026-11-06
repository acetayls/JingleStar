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
avec le nom du cue en attente, et « Tout arrêter ». Les touches de jingle, de morceau et de
cue montrent une **barre de lecture** qui avance en bas.

### Vumètres

Le retour « Module : vumètre » dessine le niveau gauche et droite d'un module, sur toute la
touche ou en bande sur le bord droit. JingleStar envoie les niveaux environ 20 fois par seconde
dès que la case « Recevoir les vumètres » est cochée (par défaut). Aucun réglage n'est
nécessaire dans JingleStar.

### Variables utiles

- `module_1_jingle_3_name`, `module_1_jingle_3_remaining`
- `module_2_track`, `module_2_track_title`, `module_2_track_remaining`
- `module_3_cue_standby`, `module_3_cue_name`, `module_3_cue_playing_name`
