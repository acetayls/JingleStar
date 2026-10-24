#!/usr/bin/env bash
# Construit JingleStar.app et JingleStar-<version>-<arch>.dmg (à lancer sur macOS).
# Usage : packaging/macos/bundle.sh <cible rust> [dossier de sortie]
#   ex. : packaging/macos/bundle.sh x86_64-apple-darwin dist
set -euo pipefail

TARGET="${1:-x86_64-apple-darwin}"
OUT="${2:-dist}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
case "$TARGET" in
  x86_64-*) ARCH=intel ;;
  aarch64-*) ARCH=apple-silicon ;;
  *) ARCH="$TARGET" ;;
esac

BIN="$ROOT/target/$TARGET/release/jinglestar"
[ -x "$BIN" ] || { echo "Binaire introuvable : $BIN (lancer cargo build --release --target $TARGET)"; exit 1; }

mkdir -p "$OUT"
APP="$OUT/JingleStar.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/jinglestar"
sed "s/@VERSION@/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# Icône .icns à partir du PNG 1024 px
ICONSET="$(mktemp -d)/JingleStar.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$HERE/icon-1024.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$HERE/icon-1024.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/JingleStar.icns"

# Signature ad hoc (pas de compte développeur : Gatekeeper demandera une confirmation)
codesign --force --deep --sign - "$APP"
codesign --verify --verbose "$APP"

# Image disque avec raccourci vers /Applications
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$OUT/JingleStar-$VERSION-macos-$ARCH.dmg"
rm -f "$DMG"
hdiutil create -volname "JingleStar" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "Créé : $DMG"
