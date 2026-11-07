import json, sys, collections, statistics as st
import numpy as np

def load(p):
    L = [json.loads(l) for l in open(p, encoding="utf8")]
    f = L[0]["freq"]; t0 = L[0]["t"]
    for x in L:
        if "t" in x: x["s"] = (x["t"] - t0) / f
        if "pt" in x: x["ps"] = (x["pt"] - t0) / f
    return L

def frames(L):
    px = [x for x in L if x["k"] == "px"]
    T = np.array([x["ps"] for x in px]); C = np.array([x["px"] for x in px], dtype=float)  # N x 4 x 3
    return T, C

def dist(a, b): return float(np.sqrt(((np.asarray(a, float) - np.asarray(b, float)) ** 2).sum()))

def target_color(T, C, a, b):
    sel = (T >= a) & (T <= b)
    if sel.sum() == 0: return None
    m = C[sel, 0, :]
    return np.median(m, axis=0)

def is_black(c): return max(c) < 30

def loads(L):
    out = []; mk = None
    for x in L:
        if x["k"] == "mark": mk = x["label"]
        if x["k"] == "cmd" and isinstance(x["cmd"], dict) and "Load" in x["cmd"]:
            out.append(dict(t=x["s"], path=x["cmd"]["Load"]["path"].split("/")[-1], mark=mk, tr=x["cmd"]["Load"]["transition"]))
            mk = None
    return out

def latency(L, settle=1.5, win=3.3):
    T, C = frames(L); ld = loads(L)
    evs = [x for x in L if x["k"] == "ev"]
    res = []
    for i, l in enumerate(ld):
        end = ld[i + 1]["t"] if i + 1 < len(ld) else T[-1]
        tc = target_color(T, C, l["t"] + settle, min(l["t"] + win, end - 0.05))
        if tc is None:
            k = np.where(T < end - 0.05)[0]
            if len(k) == 0: continue
            tc = C[k[-1], 0]
        idx = np.where((T > l["t"]) & (T < end))[0]
        first = None; blacks = 0; prev = None
        for j in idx:
            if dist(C[j, 0], tc) < 45: first = T[j]; break
            if is_black(C[j, 0]): blacks += 1
        loaded = next((e["s"] for e in evs if e["s"] > l["t"] and "Loaded" in (e["ev"] if isinstance(e["ev"], dict) else {})), None)
        res.append(dict(path=l["path"], mark=l["mark"], lat=None if first is None else (first - l["t"]) * 1000, black_frames=blacks,
                        loaded=None if loaded is None else (loaded - l["t"]) * 1000))
    return res

def cadence(T, C, a, b, vsync=1 / 60):
    """Intervalles entre changements d'image (contenu) en nombre de vsyncs."""
    sel = np.where((T >= a) & (T <= b))[0]
    ch = []; last = None
    for j in sel:
        sig = C[j, 1:, :].ravel()
        if last is None or np.abs(sig - last).max() > 6: ch.append(T[j]); last = sig
    d = np.diff(ch) / vsync
    return np.rint(d).astype(int), ch

if __name__ == "__main__":
    L = load(sys.argv[1])
    r = latency(L)
    by = collections.defaultdict(list)
    for x in r: by[(x["mark"].split()[0] if x["mark"] else "?", x["path"])].append(x)
    print(f"{'cas':6} {'fichier':22} {'GO->image ms (3 essais)':30} {'Loaded ms':18} noirs")
    for k in sorted(by):
        v = by[k]
        lats = [x["lat"] for x in v]
        print(f"{k[0]:6} {k[1]:22} {str([None if a is None else round(a) for a in lats]):30} {str([None if x['loaded'] is None else round(x['loaded']) for x in v]):18} {[x['black_frames'] for x in v]}")
    for m in ("play", "black"):
        a = [x["lat"] for x in r if x["mark"] and x["mark"].startswith(m) and x["lat"] is not None]
        if a: print(m, "médiane", round(st.median(a)), "min", round(min(a)), "max", round(max(a)), "n", len(a))
