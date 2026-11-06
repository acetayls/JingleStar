#!/usr/bin/env python3
"""Régénère crates-rust.txt : bibliothèques Rust compilées dans CueStar, avec leur licence.
À lancer depuis la racine du dépôt MediaStar après un changement de dépendances :
    python3 apps/cuestar/packaging/licences/crates.py
"""
import json
import pathlib
import subprocess

meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"]))
pkgs = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
root = next(i for i, p in pkgs.items() if p["name"] == "cuestar")

# Dépendances normales seulement (pas de build-dependencies ni dev-dependencies), toutes plateformes.
seen, todo = set(), [root]
while todo:
    i = todo.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] is None for k in d["dep_kinds"]):
            todo.append(d["pkg"])

workspace = set(meta["workspace_members"])
lines = sorted(
    f'{p["name"]} {p["version"]} : {p.get("license") or "voir " + (p.get("repository") or "crates.io")}'
    for i, p in ((i, pkgs[i]) for i in seen)
    if i not in workspace
)
out = pathlib.Path(__file__).with_name("crates-rust.txt")
out.write_text(
    "Bibliothèques Rust compilées dans CueStar (nom, version : licence)\n"
    "Généré par crates.py à partir de Cargo.lock.\n\n" + "\n".join(lines) + "\n",
    encoding="utf-8",
)
print(f"{out} : {len(lines)} bibliothèques")
