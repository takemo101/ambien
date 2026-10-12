#!/bin/bash
# Builds Ambien.app and a DMG for GitHub Releases.
#
#   scripts/package-app.sh            # writes dist/Ambien-<version>-arm64.dmg and its .sha256
#
# The app is signed ad-hoc only (what Apple Silicon needs to run it): no Developer ID, no
# notarization, no staple. A Mac that downloads the DMG shows a Gatekeeper warning on first launch;
# README.md says how to approve it. There is no in-app update in this build.
#
# Environment: AMBIEN_BUILD sets CFBundleVersion (raise it for every release; default 1).
# CARGO_NET_OFFLINE=true builds without the network when the dependencies are already fetched.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "package-app.sh: needs an Apple Silicon Mac (arm64)." >&2
    exit 1
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
BUILD="${AMBIEN_BUILD:-1}"
[ -n "$VERSION" ] || { echo "package-app.sh: no version in Cargo.toml" >&2; exit 1; }

DIST="dist"
APP="$DIST/Ambien.app"
STAGE="$DIST/dmg-stage"
DMG="$DIST/Ambien-$VERSION-arm64.dmg"

echo "==> building ambien $VERSION (build $BUILD) for macOS 15+"
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --locked

rm -rf "$APP" "$STAGE" "$DMG" "$DMG.sha256"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/ambien "$APP/Contents/MacOS/ambien"
sed -e "s/@VERSION@/$VERSION/" -e "s/@BUILD@/$BUILD/" resources/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null
cp LICENSE "$APP/Contents/Resources/LICENSE"
python3 scripts/third-party-licenses.py > "$APP/Contents/Resources/THIRD_PARTY_LICENSES.txt"

echo "==> ad-hoc signing"
codesign --force --sign - --timestamp=none --identifier com.takemo101.ambien "$APP"
codesign --verify --strict --verbose=2 "$APP"

ARCHS="$(lipo -archs "$APP/Contents/MacOS/ambien")"
[ "$ARCHS" = "arm64" ] || { echo "package-app.sh: expected arm64 only, got: $ARCHS" >&2; exit 1; }

echo "==> creating $DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/Ambien.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Ambien $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG" >/dev/null
rm -rf "$STAGE"

( cd "$DIST" && shasum -a 256 "$(basename "$DMG")" > "$(basename "$DMG").sha256" )
echo "==> done"
cat "$DMG.sha256"
