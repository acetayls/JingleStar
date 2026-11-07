import sys, numpy as np, ana
L = ana.load(sys.argv[1]); T, C = ana.frames(L)
marks = [x for x in L if x["k"] == "mark"]
cmds = [x for x in L if x["k"] == "cmd"]
evs = [x for x in L if x["k"] == "ev"]
def next_cmd(t): return next(c for c in cmds if c["s"] >= t)
def sig_changes(a, b):
    idx = np.where((T > a) & (T < b))[0]; last = None; out = []
    for j in idx:
        s = C[j, 1:, :].ravel()
        if last is None: last = s; continue
        if np.abs(s - last).max() > 6: out.append(T[j]); last = s
    return out
def col_at(t):
    j = np.searchsorted(T, t) - 1; return C[max(j, 0), 0]
ends = [m["s"] for m in marks] + [T[-1]]
print("== GO sur média préchargé (Load en pause, puis Play) : Play -> première image en mouvement")
for i, m in enumerate(marks):
    lab = m["label"]; end = ends[i + 1]
    if lab.startswith("resume"):
        c = next_cmd(m["s"]); ch = sig_changes(c["s"], end)
        print(f"  {lab.split()[1]:22} {round((ch[0]-c['s'])*1000) if ch else None} ms")
print("== Enchaînement en fin de média (Ended -> Load immédiat)")
for i, m in enumerate(marks):
    lab = m["label"]
    if lab.startswith("follow ") or lab.startswith("follow hold"):
        c = next_cmd(m["s"]); end = ends[i + 1]
        ended = [e["s"] for e in evs if "Ended" in str(e["ev"]) and e["s"] <= c["s"] + 0.01 and e["s"] > c["s"] - 5]
        newc = ana.target_color(T, C, c["s"] + 1.5, end - 0.1)
        idx = np.where((T > c["s"] - 3) & (T < end))[0]
        # dernière image de A (contenu qui bouge) puis première de B
        first = next((T[j] for j in idx if T[j] > c["s"] and ana.dist(C[j, 0], newc) < 45), None)
        prev = [T[j] for j in idx if T[j] < (first or end)]
        moving = sig_changes(c["s"] - 3, first or end)
        blk = [T[j] for j in idx if T[j] < (first or end) and T[j] > c["s"] - 3 and ana.is_black(C[j, 0])]
        lastmove = moving[-1] if moving else None
        print(f"  {lab:34} Ended->Load {round((c['s']-ended[-1])*1000) if ended else 'pas de Ended'} ms ; Load->1re image B {round((first-c['s'])*1000) if first else None} ms ; "
              f"trou visible (dernière image animée A -> 1re B) {round((first-lastmove)*1000) if first and lastmove else None} ms ; noir {round((blk[-1]-blk[0])*1000)+17 if blk else 0} ms")
print("== Transitions")
for i, m in enumerate(marks):
    lab = m["label"]
    if lab.startswith("xfade") or lab.startswith("black"):
        c = next_cmd(m["s"]); end = ends[i + 1]
        old = col_at(c["s"]); new = ana.target_color(T, C, c["s"] + 3, end - 0.05)
        idx = np.where((T > c["s"]) & (T < end))[0]
        start = next((T[j] for j in idx if ana.dist(C[j, 0], old) > 20), None)
        done = next((T[j] for j in idx if ana.dist(C[j, 0], new) < 12), None)
        blk = [T[j] for j in idx if ana.is_black(C[j, 0])]
        gaps = np.diff([T[j] for j in idx if T[j] >= (start or 0) and T[j] <= (done or 0)])
        print(f"  {lab:28} début {round((start-c['s'])*1000) if start else None} ms, durée {round((done-start)*1000) if start and done else None} ms, "
              f"noir {round(len(blk)*1000/60)} ms, plus long écart entre images pendant la transition {round(gaps.max()*1000) if len(gaps) else None} ms")
dr = [e["ev"]["Drops"] for e in evs if isinstance(e["ev"], dict) and "Drops" in e["ev"]]
print("Drops max", max(d["dropped"] for d in dr), "delayed max", max(d["delayed"] for d in dr))
