"""Écrit les scénarios du banc pour un dossier de médias produit par medias.sh.

    python scenarios.py <dossier des médias> <dossier des scénarios>

Scénarios (une commande par ligne, lus par bench-sortie) :
- latence.txt  : 3 tours de GO sur chaque fichier, depuis un média en cours puis depuis le noir ;
- prepare.txt  : les mêmes GO avec le média préparé d'avance (comme la cuelist), fondus et enchaînements ;
- boucles.txt  : chaque fichier en boucle, plusieurs tours (fluidité, coutures) ;
- endurance.txt : 10 min de 4K60 en boucle puis 100 GO au hasard.
"""
import os
import random
import sys

FILES = ["h264_1080p30.mp4", "h264_1080p60.mp4", "h264_4k30.mp4", "hevc_4k30.mp4", "hevc10_4k60.mp4", "av1_4k30.mp4",
         "vp9_1080p30.webm", "prores_1080p30.mov", "prores_4k30.mov", "hap_1080p30.mov", "hap_4k30.mov", "dnxhr_4k30.mov",
         "still_4k.png"]


def write(path, lines):
    with open(path, "w", encoding="utf8") as f:
        f.write("\n".join(lines) + "\n")


def main():
    media, out = sys.argv[1].rstrip("/\\") + "/", sys.argv[2]
    os.makedirs(out, exist_ok=True)
    files = [f for f in FILES if os.path.exists(media + f)]
    rnd = random.Random(7)

    lat = ["wait 3"]
    for _ in range(3):
        o = files[:]; rnd.shuffle(o)
        for f in o:
            lat += [f"mark play {f}", f"load {media}{f} hold", "wait 3.5"]
        o = files[:]; rnd.shuffle(o)
        for f in o:
            lat += ["stop", "wait 1.5", f"mark black {f}", f"load {media}{f} hold", "wait 3.5"]
    write(os.path.join(out, "latence.txt"), lat)

    seq = []
    for _ in range(3):
        o = files[:]; rnd.shuffle(o); seq += o
    prep = ["wait 3", f"prepare {media}{seq[0]}", "wait 2"]
    for i, f in enumerate(seq):
        prep += [f"mark play {f}", f"load {media}{f} hold"]
        if i + 1 < len(seq):
            prep += [f"prepare {media}{seq[i + 1]}"]
        prep += ["wait 3.5"]
    xs = [f for f in ["h264_1080p30.mp4", "hevc_4k30.mp4", "prores_4k30.mov", "hap_4k30.mov", "av1_4k30.mp4"] if f in files]
    prep += [f"prepare {media}{xs[0]}", "wait 1"]
    for i, f in enumerate(xs):
        prep += [f"mark xfade:1.0 {f}", f"load {media}{f} hold xfade:1.0"]
        if i + 1 < len(xs):
            prep += [f"prepare {media}{xs[i + 1]}"]
        prep += ["wait 4"]
    for a, b in [("h264_1080p30.mp4", "hevc_4k30.mp4"), ("hevc_4k30.mp4", "prores_1080p30.mov"), ("prores_1080p30.mov", "hap_4k30.mov")]:
        prep += [f"mark followsrc {a}", f"load {media}{a} hold signal", f"prepare {media}{b}", "waitev Ended 30",
                 f"mark follow {b}", f"load {media}{b} hold", "wait 3"]
    write(os.path.join(out, "prepare.txt"), prep)

    loops = ["wait 3"]
    for f in files:
        if f.endswith(".png"):
            continue
        loops += [f"mark loop {f}", f"load {media}{f} loop", "wait 33.5"]
    write(os.path.join(out, "boucles.txt"), loops)

    longr = ["wait 3", "mark long loop hevc10_4k60.mp4", f"load {media}hevc10_4k60.mp4 loop", "wait 600", "mark long cuts"]
    prev = None
    for _ in range(100):
        f = rnd.choice([x for x in files if x != prev]); prev = f
        tr = rnd.choice(["", "", "xfade:1.0", "black:1.0"])
        longr += [f"load {media}{f} loop {tr}".rstrip(), "wait 6"]
    longr += ["mark long idle", "stop", "wait 30"]
    write(os.path.join(out, "endurance.txt"), longr)


if __name__ == "__main__":
    main()
