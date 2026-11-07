# Journal des modifications

## 0.1.1 — en cours (pas encore publiée)

Première version de CueStar : diffusion de vidéos, d'images et de sons sur un écran de sortie,
dans la charte de JingleStar.

### Modules

- **Lecteur** : une playlist façon JingleStar (boucle, aléatoire, transitions **Coupe**,
  **Noir** et **Fondu enchaîné**), avec un **aperçu SUIVANT** redimensionnable, masquable et
  navigable.
- **Diaporama** : un dossier de photos et de vidéos (sous-dossiers, durée, ajustement, son des
  vidéos, aléatoire, boucle), réglages appliqués à chaud, transport avec les boutons de la
  playlist JingleStar.
- **Présentation** (nouveau) : diffuse un fichier PowerPoint **.pptx** sur la sortie, sans
  PowerPoint. Vignettes des diapositives, **LANCER** / **SUIVANTE** / précédente / **Aller à**,
  diapositives masquées sautées, transition au choix, **Relire** après une modification. Les
  diapositives sont dessinées par CueStar à la résolution de la sortie (textes, puces, formes,
  images, tableaux, SmartArt, graphiques simples) et la suivante est préparée d'avance.
  Ouvre aussi les **PDF** (une page = une diapositive). **Télécommandes de présentation**
  reconnues (Page suivante / précédente, écran noir B ou point, F5 et Maj+F5), dans tous les
  onglets et depuis la fenêtre de sortie.
- **Cuelist** façon JingleStar : GO, cue en attente, enchaîner, boucle, transition, cue Noir,
  glisser-déposer.
- **Panneau Texte** de JingleStar à côté des modules, enregistré dans le projet.

### Éditer un élément

- Clic droit **Éditer…** sur un élément du Lecteur ou un cue : frise de vignettes avec poignées
  d'entrée et de sortie, boucle, **fondus au noir** d'entrée et de sortie, volume, **EQ** de
  JingleStar, durée d'affichage d'une image fixe. Les réglages sont appliqués par la sortie,
  l'EQ à chaud.
- Clic droit **Informations sur le fichier…** (Lecteur et Cuelist) : rapport **MediaInfo**
  complet, ou ce que libmpv sait du fichier si MediaInfo manque.

### Sortie

- La sortie tourne dans un **processus séparé** : une régie qui rame ne retarde aucune image.
- **Sortie composée par CueStar** (Direct3D 11 sous Windows, OpenGL sous macOS et Linux) avec
  deux lecteurs : fondu enchaîné image et son, fondu au noir, **mapping 4 coins**. Affichage de
  secours si l'API de rendu de mpv manque.
- Un cue part en ~30 ms ; l'image précédente reste jusqu'à la première image du média suivant.
- **Préférences** (anciennement « Video output ») en onglets : **Écran et moteur** (écran,
  décodage, cadence, interpolation, adaptation de la fréquence de l'écran), **Audio** (carte et
  volume), **Mires de test**, **Mapping 4 coins**.
- **Monitoring** : fenêtre flottante avec le retour de l'image de sortie, fps, CPU et GPU.
- Compteur d'**images perdues et retardées** cumulé sur tous les médias, avec remise à zéro
  (clic droit, bouton, OSC `output/drops/reset`).

### Télécommandes

- **OSC** (préfixe `/cuestar`, `/diaporama/…` accepté) avec **OSCQuery**, annonce **mDNS** et
  retour d'état (média, temps restant, cue en attente…) pour Companion. La liste des adresses
  est dans l'aide, plus dans la fenêtre, comme dans JingleStar.
- **MIDI**, consoles **Yamaha** et **Midas M32 / X32**, assignations avec mode apprendre,
  raccourcis clavier : les mêmes écrans que JingleStar.

### Projets et interface

- Projet **.cuestar** comme JingleStar : session, sauvegarde sûre, médias retrouvés s'ils ont
  bougé ; journal de la régie et de la sortie.
- Interface en **français ou en anglais** (choix gardé sur la machine).
- **Manuel** français et anglais (**F1**, bouton **? Aide**).
- Icône CueStar plus lisible en grand comme en petit.

### Téléchargements

- Paquets prêts à l'emploi, avec **libmpv** et **MediaInfo** embarqués : installeur et zip
  portable **Windows**, **.dmg macOS** (Intel et Apple Silicon), **AppImage Linux**. Les licences
  (CueStar, mpv, FFmpeg, MediaInfo, bibliothèques Rust) sont livrées dans chaque paquet.
