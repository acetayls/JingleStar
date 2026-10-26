# Notes pour les prochains audits

À lire avant d'auditer JingleStar : ces points sont des **choix assumés** ou déjà traités.
Inutile de les signaler à nouveau, sauf si le contexte change.

## Choix assumés (ne pas signaler)

### Pilotage réseau OSC / OSCQuery ouvert, sans protection

OSC (UDP) et OSCQuery (HTTP, `Access-Control-Allow-Origin: *`) écoutent sur toutes les
interfaces, sans mot de passe, sans liste d'adresses autorisées et sans file de commandes bornée.
**C'est voulu** (décision de Pierrick, octobre 2026) : OSC est un protocole ouvert, utilisé dans
des réseaux de régie privés, où cette protection n'apporte rien et compliquerait la mise en
œuvre (consoles, QLab, Companion, tablettes). L'OSC est d'ailleurs désactivé par défaut.

Ce choix couvre aussi les points voisins : choix de l'interface d'écoute, filtrage par IP,
en-tête CORS, bornes des files OSC/MIDI/console et du tampon de lecture de la console Yamaha.

Ce qui reste vrai et vérifié : une valeur reçue par le réseau ne peut ni planter l'application
ni rendre un son muet (valeurs non numériques refusées, index bornés, volumes bornés).

### Compilation et livraison

- Les builds se font en local sur le PC de Pierrick, pas sur GitHub Actions (crédits épuisés).
  Ajouter `clippy`, `fmt` ou `cargo audit` à la CI n'a donc pas d'effet pour l'instant ;
  `cargo clippy --all-targets` se lance à la main.
- Pas de signature de l'exécutable Windows (alerte SmartScreen) : il faut acheter un
  certificat de signature, décision à prendre par Pierrick.
- Pas de version macOS tant que Pierrick ne la demande pas.

## Corrigé en 0.1.2 (audits Sonnet et Opus du 26 octobre 2026)

| Constat | Correction | Où |
|---|---|---|
| Session illisible remplacée puis écrasée | Valeurs `null` réparées ; sinon fichier mis de côté (`session-illisible-<date>.json`), copie `.bak` chargée, bandeau d'alerte. Écriture `sync_all` avant renommage, fichier temporaire propre au nom, 5 copies datées au lancement. Jamais de `null` écrit. | `project.rs` (`load_session`, `write_to`, `sanitize`) |
| Carte son perdue, « Appliquer » sans effet | Flux mort détecté (compteur d'appels du callback), reconnexion toutes les 2 s au même périphérique uniquement, bandeau rouge + « Reconnecter ». « Appliquer » rouvre une sortie en panne. Interface absente au lancement : bandeau orange, retour automatique quand elle revient et que rien ne joue. | `controller.rs` (`poll_device`), `engine.rs` (`request_open`, `strict`) |
| Ouverture du périphérique bloquante | Ouverture en tâche de fond, indicateur dans la barre du bas. | `engine.rs`, `controller.rs` |
| Boucle infinie de playlist sur fichiers illisibles | Compteur d'échecs : arrêt après un tour complet, message. | `controller.rs` (`on_loaded`) |
| Panique du décodeur | `catch_unwind` autour du décodage et de la mesure ; journal `jinglestar.log` et `panic hook`. | `controller.rs` (`guarded`), `log.rs`, `main.rs` |
| Pilotage dépendant de la boucle d'affichage | Contrôleur dans un `Mutex`, thread « pilotage » qui appelle `poll` toutes les 5 ms ; boîtes de dialogue dans leur propre thread. | `ui/mod.rs` (`SharedCtrl`), `ui/dialogs.rs` |
| Cues « Enchaîner » à une image près | Enchaînement confié au mixeur (`PlayAfter`) : départ à l'échantillon près, replanifié à chaque changement de la conduite. | `mixer.rs`, `controller/cues.rs` |
| Volume NaN (OSC) / dB sans plafond | `arg_f32` refuse les valeurs non finies ; `db_to_gain` : NaN = silence, plafond +24 dB ; contrôleur et chargement bornent les valeurs. | `osc.rs`, `buffer.rs`, `controller.rs`, `project.rs` |
| Mémoire sans plafond | Entrelacement sur place, capacité réservée d'avance, tampons d'origine libérés avant l'entrelacement ; seuil d'alerte réglable ; son de plus de 20 min signalé. | `decode.rs`, `buffer.rs`, `controller.rs` |
| Gels à la fermeture des consoles | `Drop` n'attend plus le thread réseau ; pause M32 interruptible. | `console.rs`, `m32.rs` |
| Fichiers introuvables | Nouvel essai toutes les 5 s, fichier modifié relu, recherche près du projet à l'ouverture, bouton « Rechercher… » (relocalisation par nom). | `controller.rs` (`check_files`), `project.rs` (`resolve_moved`, `relocate`) |
| Qualité audio | Multicanal mixé en stéréo (ITU, normalisé) ; paquets corrompus et fichiers tronqués signalés ; dither TPDF en sortie 16 bits et moins ; mesure de sonie toujours au taux d'origine. | `decode.rs`, `engine.rs` |
| Commandes perdues sans signalement | Commandes perdues comptées et signalées (barre du bas, journal). | `engine.rs`, `controller.rs` |
| Index de playlist périmés après modification (Opus M1) | Morceau en file recalculé après suppression, ajout ou relecture du dossier ; morceau en cours retrouvé par son chemin. | `controller.rs` (`requeue`, `relocate_current`) |
| Une seule file de décodage (Opus M2) | File prioritaire (clic, GO, départ de playlist, préchargement du GO) servie avant les préchargements. La mesure de niveau d'un morceau pas encore analysé reste faite avant de le rendre : il part au bon niveau, sans saut de volume (choix assumé). | `controller.rs` (`spawn_loaders`, `request_urgent_load`) |
| Projet ouvert jamais réenregistré, préférences machine dans le projet (Opus M6) | Bouton « Enregistrer » (Ctrl+S) avec point de modification ; carte son, préférences et télécommandes gardées dans `machine.json` et conservées à l'ouverture d'un projet. | `controller.rs` (`MachinePrefs`, `save_project`), `ui/mod.rs` |
| GitHub Actions à chaque push (Opus F8) | Workflow lancé seulement à la demande. | `.github/workflows/build.yml` |
| Pas de test du contrôleur | Tests : playlist illisible en boucle, NaN, panique du décodeur, enchaînement de cues par le mixeur ; tests projet (réparation, `.bak`, relocalisation) et mixeur (`PlayAfter`). | `controller.rs`, `project.rs`, `mixer.rs` |

## Pas encore traité (connu)

- **Lecture en flux des longs morceaux** : tout son reste décodé en mémoire. L'alerte mémoire et
  le passage des longs morceaux en playlist (seuls le morceau en cours et le suivant sont en
  mémoire) suffisent pour l'instant.
- **Reprise des sons en cours après une perte de carte son** : ils sont arrêtés à la reconnexion.
- Points de priorité basse des deux audits (allocation possible dans le callback au-delà de
  8192 images, index du mixeur décalés à la suppression d'un module, `Project.version` non lu,
  nom mDNS fixe, `println!` de débogage, découpage de `controller.rs` et `ui/mod.rs`).
- Points faibles de l'audit Opus non demandés : F1, F3, F4, F5 (fait en 0.1.2 avec le mixage
  multicanal), F7, F9.
