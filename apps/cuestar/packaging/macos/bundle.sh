#!/usr/bin/env bash
# Construit CueStar.app (avec libmpv et ses dépendances) et CueStar-<version>-macos-<arch>.dmg.
# À lancer sur macOS, depuis n'importe où, après `cargo build --release -p cuestar --target <cible>` :
#   apps/cuestar/packaging/macos/bundle.sh aarch64-apple-darwin dist
#
# libmpv vient de Homebrew (`brew install mpv dylibbundler`) : il est copié dans
# Contents/Frameworks avec toutes ses bibliothèques, chemins réécrits en @loader_path.
#
# Signature et notarisation, si ces variables sont définies (sinon signature ad hoc,
# et Gatekeeper demande une confirmation au premier lancement) :
# (création du compte, du certificat et des secrets GitHub : voir SIGNATURE.md)
#   MACOS_SIGN_IDENTITY   « Developer ID Application: … (TEAMID) », certificat dans le trousseau
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD   compte développeur et mot de passe d'app,
#                         pour notariser le .dmg avec notarytool
set -euo pipefail

TARGET="${1:-aarch64-apple-darwin}"
OUT="${2:-dist}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"          # apps/cuestar
WORKSPACE="$(cd "$ROOT/../.." && pwd)"     # racine du dépôt MediaStar (dossier target/)
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
case "$TARGET" in
  x86_64-*) ARCH=intel ;;
  aarch64-*) ARCH=apple-silicon ;;
  *) ARCH="$TARGET" ;;
esac

BIN="$WORKSPACE/target/$TARGET/release/cuestar"
[ -x "$BIN" ] || BIN="$WORKSPACE/target/release/cuestar"
[ -x "$BIN" ] || { echo "Binaire introuvable (lancer cargo build --release -p cuestar --target $TARGET)"; exit 1; }

BREW="$(brew --prefix)"
LIBMPV="$(brew --prefix mpv)/lib/libmpv.2.dylib"
[ -f "$LIBMPV" ] || { echo "libmpv introuvable : $LIBMPV (brew install mpv)"; exit 1; }
command -v dylibbundler >/dev/null || { echo "dylibbundler absent (brew install dylibbundler)"; exit 1; }

# L'architecture de libmpv doit être celle de CueStar (Homebrew ne fournit que celle de la machine).
BIN_ARCH="$(lipo -archs "$BIN")"
MPV_ARCH="$(lipo -archs "$LIBMPV")"
[ "$BIN_ARCH" = "$MPV_ARCH" ] || { echo "Architectures différentes : cuestar $BIN_ARCH, libmpv $MPV_ARCH"; exit 1; }

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
APP="$OUT/CueStar.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$BIN" "$APP/Contents/MacOS/cuestar"
cp "$ROOT/assets/icons/cuestar.icns" "$APP/Contents/Resources/CueStar.icns"
printf 'APPL????' > "$APP/Contents/PkgInfo"
mkdir -p "$APP/Contents/Resources/licences"
cp "$HERE/../licences/"* "$APP/Contents/Resources/licences/"

# libmpv et ses dépendances dans Contents/Frameworks
FW="$APP/Contents/Frameworks"
cp "$LIBMPV" "$FW/libmpv.2.dylib"
chmod u+w "$FW/libmpv.2.dylib"
install_name_tool -id "@rpath/libmpv.2.dylib" "$FW/libmpv.2.dylib"
dylibbundler --overwrite-files --bundle-deps --no-codesign \
  --fix-file "$FW/libmpv.2.dylib" \
  --dest-dir "$FW" --install-path "@loader_path/" \
  --search-path "$BREW/lib"
# Aucune dépendance ne doit encore pointer vers Homebrew.
if otool -L "$FW"/*.dylib | grep -E "^\s+($BREW|/usr/local/(opt|Cellar)|/opt/homebrew)"; then
  echo "Des bibliothèques pointent encore vers Homebrew"; exit 1
fi

# Version minimale de macOS : la plus haute exigée par les bibliothèques embarquées.
MINOS="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
for f in "$FW"/*.dylib; do
  m="$(vtool -show-build "$f" 2>/dev/null | awk '/minos/{print $2; exit}')"
  [ -n "$m" ] && [ "$(printf '%s\n%s\n' "$MINOS" "$m" | sort -V | tail -1)" = "$m" ] && MINOS="$m"
done
sed -e "s/@VERSION@/$VERSION/g" -e "s/@MINOS@/$MINOS/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
echo "macOS minimum : $MINOS"

# Signature : chaque bibliothèque, puis l'application (pas de --deep, déconseillé par Apple).
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
SIGN=(codesign --force --sign "$IDENTITY")
[ "$IDENTITY" != "-" ] && SIGN+=(--options runtime --timestamp)
for f in "$FW"/*.dylib; do "${SIGN[@]}" "$f"; done
"${SIGN[@]}" --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --verbose=2 "$APP"

# Image disque avec raccourci vers /Applications
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
# Version non notarisée : mode d'emploi pour passer Gatekeeper (voir SIGNATURE.md).
NOTARISE=0
[ "$IDENTITY" != "-" ] && [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ] && NOTARISE=1
[ "$NOTARISE" = 1 ] || cp "$HERE/LISEZ-MOI - Ouvrir CueStar.txt" "$STAGE/"
DMG="$OUT/CueStar-$VERSION-macos-$ARCH.dmg"
rm -f "$DMG"
hdiutil create -volname "CueStar" -srcfolder "$STAGE" -ov -format UDZO "$DMG"

if [ "$IDENTITY" != "-" ]; then
  codesign --force --sign "$IDENTITY" --timestamp "$DMG"
  if [ "$NOTARISE" = 1 ]; then
    xcrun notarytool submit "$DMG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
      --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$DMG"
    spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
  else
    echo "Signé sans notarisation (APPLE_ID, APPLE_TEAM_ID ou APPLE_APP_PASSWORD manquant)"
  fi
fi
echo "Créé : $DMG"
