#!/usr/bin/env bash
# Construit CueStar-<version>-linux-x86_64.AppImage, avec libmpv et ses dépendances.
# À lancer sur Linux après `cargo build --release -p cuestar` :
#   apps/cuestar/packaging/linux/appimage.sh dist
# Il faut libmpv installé (paquet libmpv-dev ou libmpv2) ; linuxdeploy est téléchargé s'il manque.
# Construire sur une distribution ancienne (Ubuntu 22.04) pour que l'AppImage tourne partout.
set -euo pipefail

OUT="${1:-dist}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"          # apps/cuestar
WORKSPACE="$(cd "$ROOT/../.." && pwd)"     # racine du dépôt MediaStar
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
BIN="$WORKSPACE/target/release/cuestar"
[ -x "$BIN" ] || { echo "Binaire introuvable : $BIN (lancer cargo build --release -p cuestar)"; exit 1; }

# libmpv chargé à l'exécution : linuxdeploy ne le voit pas, on le lui donne.
LIBMPV="$(ldconfig -p | awk '/libmpv\.so\.[12] /{print $NF; exit}')"
[ -n "$LIBMPV" ] || { echo "libmpv introuvable (apt install libmpv-dev)"; exit 1; }
echo "libmpv : $LIBMPV"

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
TOOLS="$OUT/.outils"
mkdir -p "$TOOLS"
LINUXDEPLOY="$(command -v linuxdeploy-x86_64.AppImage || true)"
if [ -z "$LINUXDEPLOY" ]; then
  LINUXDEPLOY="$TOOLS/linuxdeploy-x86_64.AppImage"
  [ -x "$LINUXDEPLOY" ] || {
    curl -fsSL -o "$LINUXDEPLOY" https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
    chmod +x "$LINUXDEPLOY"
  }
fi
# Pas de FUSE dans les conteneurs ni sur les runners : l'AppImage de linuxdeploy s'extrait.
export APPIMAGE_EXTRACT_AND_RUN=1

APPDIR="$OUT/CueStar.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/share/doc/cuestar/licences"
cp "$HERE/../licences/"* "$APPDIR/usr/share/doc/cuestar/licences/"

cd "$OUT"
LDAI_OUTPUT="CueStar-$VERSION-linux-x86_64.AppImage" \
LINUXDEPLOY_OUTPUT_VERSION="$VERSION" \
"$LINUXDEPLOY" --appdir "$APPDIR" \
  --executable "$BIN" \
  --library "$LIBMPV" \
  --desktop-file "$HERE/cuestar.desktop" \
  --icon-file "$ROOT/assets/icons/cuestar-256.png" \
  --icon-filename cuestar \
  --output appimage
echo "Créé : $OUT/CueStar-$VERSION-linux-x86_64.AppImage"
