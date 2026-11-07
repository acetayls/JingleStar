import sys, csv, datetime, numpy as np, collections, ana
L = ana.load(sys.argv[1]); T, C = ana.frames(L)
gpu = list(csv.reader(open(sys.argv[2]))) if len(sys.argv) > 2 else []
start_wall = None
marks = [x for x in L if x["k"] == "mark"]; ends = [m["s"] for m in marks] + [T[-1]]
res = [x for x in L if x["k"] == "res"]
evs = [x for x in L if x["k"] == "ev"]
lat = {r["mark"]: r for r in ana.latency(L)}
# horloge murale du début du log : premier échantillon GPU ~ début ; on recale via la fin
print(f"{'fichier':20} {'GO ms':>6} {'img/s':>6} {'cadence (écarts en vsync)':34} {'pire':>5} {'drops':>6} {'CPU%':>5} {'RAM Mo':>7}")
for i, m in enumerate(marks):
    a, b = m["s"], ends[i + 1]
    f = m["label"].split()[1]
    iv, ch = ana.cadence(T, C, a + 1.0, b - 0.2)
    cnt = collections.Counter(iv.tolist())
    fps = (len(ch) - 1) / (ch[-1] - ch[0]) if len(ch) > 2 else 0
    worst = iv.max() if len(iv) else None
    dr = [e["ev"]["Drops"] for e in evs if a < e["s"] < b and isinstance(e["ev"], dict) and "Drops" in e["ev"]]
    rr = [r for r in res if a + 1 < r["s"] < b]
    cpu = (rr[-1]["cpu100ns"] - rr[0]["cpu100ns"]) / 1e7 / (rr[-1]["s"] - rr[0]["s"]) * 100 / 16 if len(rr) > 2 else 0
    ram = max(r["ws"] for r in rr) / 2**20 if rr else 0
    print(f"{f:20} {round(lat[m['label']]['lat']) if lat.get(m['label']) and lat[m['label']]['lat'] else '-':>6} {fps:6.1f} {str(dict(sorted(cnt.items()))):34} {worst:>5} {str(max((d['dropped'] for d in dr), default=0)):>6} {cpu:5.1f} {ram:7.0f}")
