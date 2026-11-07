# Banc de mesure de la sortie CueStar (Windows)

Mesure le moteur de sortie tel qu'il tourne en spectacle : `bench-sortie` lance `cuestar --output`,
lui envoie des commandes comme la régie, et capture l'écran de sortie image par image (duplication
d'écran DXGI : heure de présentation de chaque image et 4 pixels relus). Il relève aussi le CPU et la
mémoire du moteur toutes les 0,5 s. La carte graphique se suit à côté avec `nvidia-smi`.

Les médias de test portent un carré de couleur en haut à gauche, propre à chaque fichier : la sonde
voit ainsi l'instant exact où un média apparaît, et toute image noire entre deux.

## Préparer

```bash
cargo build --release -p cuestar
cargo build --release --manifest-path tools/bench-sortie/Cargo.toml
bash tools/bench-sortie/medias.sh <dossier des médias>      # ffmpeg (chemin en tête du script), ~13 Go
python tools/bench-sortie/scenarios.py <dossier des médias> <dossier des scénarios>
```

## Lancer

```bash
tools/bench-sortie/target/release/bench-sortie.exe target/release/cuestar.exe <scénario.txt> <journal.jsonl> [x de l'écran de sortie] [préférences JSON]
```

- `x de l'écran de sortie` : bord gauche de l'écran capturé dans le bureau Windows (1920 par défaut :
  l'écran à droite d'un écran principal 1080p).
- `préférences JSON` : réglages machine passés au moteur, `{"volume":0.0}` par défaut (son coupé).
  Exemple : `{"volume":0.0,"cadence":"Resample"}`.
- `CUESTAR_MPV_OPTS="nom=valeur,…"` : options mpv en plus, pour essayer un réglage.
- `CUESTAR_DEBUG=1` et `PROBE_STDERR=<fichier>` : statistiques du compositeur, une ligne par seconde.

La session Windows doit rester déverrouillée : écran de verrouillage = plus de capture.

Commandes d'un scénario : `wait <s>`, `mark <texte>`, `load <chemin> [hold] [loop] [paused] [signal]
[xfade:<s>|black:<s>]`, `prepare <chemin> [loop]`, `play`, `pause`, `stop`, `waitev <Événement> <délai s>`.

## Lire les résultats

```bash
python tools/bench-sortie/analyse/ana.py <journal>        # GO -> première image à l'écran, par fichier
python tools/bench-sortie/analyse/ana_seq.py <journal>    # préchargement, enchaînements, fondus
python tools/bench-sortie/analyse/ana_loop.py <journal>   # cadence à l'écran, pertes, CPU, mémoire
python tools/bench-sortie/analyse/onscreen.py <journal>   # GO -> événement OnScreen du moteur
```

La cadence lue à l'écran n'est fiable que sur les codecs intra (HAP, ProRes, DNxHR, PNG) : en H.264
et HEVC, la compression fige le bruit d'une image à l'autre. Pour ceux-là, se fier aux pertes comptées
par mpv.
