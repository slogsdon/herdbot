#!/bin/bash
# Build $OUT_DIR/Herdbot.app. Inputs (environment, all optional):
#   ARCHS            space-separated: "arm64", "x86_64" or "arm64 x86_64" (default: this machine's)
#   HERDBOT_HELPER   a herdbot binary to embed at Contents/Helpers/herdbot
#   SIGN_IDENTITY    codesign identity; "-" (default) is ad-hoc. Anything else also enables the hardened runtime.
#   APP_VERSION      default: the workspace version in ../../Cargo.toml
#   OUT_DIR          default: build
# Notifications need a bundle id. Signing is inside-out (helper, then app); --deep is never used to sign.
set -euo pipefail
cd "$(dirname "$0")/.."

ARCHS="${ARCHS:-$(uname -m)}"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
OUT_DIR="${OUT_DIR:-build}"
APP_VERSION="${APP_VERSION:-$(sed -n 's/^version = \"\(.*\)\"/\1/p' ../../Cargo.toml | head -1)}"
[ -n "$APP_VERSION" ] || { echo "build-app: could not read the version from Cargo.toml" >&2; exit 1; }

# One cross-compiled build per architecture; multi-arch swift build needs full Xcode.
slices=()
for arch in $ARCHS; do
  swift build -c release --triple "$arch-apple-macosx15.0" --scratch-path ".build/release-$arch" >&2
  slices+=("$(swift build -c release --triple "$arch-apple-macosx15.0" --scratch-path ".build/release-$arch" --show-bin-path)/HerdbotMenu")
done

app="$OUT_DIR/Herdbot.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
if [ "${#slices[@]}" -eq 1 ]; then
  cp "${slices[0]}" "$app/Contents/MacOS/HerdbotMenu"
else
  lipo -create "${slices[@]}" -output "$app/Contents/MacOS/HerdbotMenu"
fi
cp Info.plist "$app/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$APP_VERSION" "$app/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$APP_VERSION" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"

sign=(codesign --force --sign "$SIGN_IDENTITY")
[ "$SIGN_IDENTITY" = "-" ] || sign+=(--options runtime --timestamp)
if [ -n "${HERDBOT_HELPER:-}" ]; then
  mkdir -p "$app/Contents/Helpers"
  cp "$HERDBOT_HELPER" "$app/Contents/Helpers/herdbot"
  "${sign[@]}" "$app/Contents/Helpers/herdbot"
fi
"${sign[@]}" "$app"
codesign --verify --deep --strict "$app"
echo "built $app ($(lipo -archs "$app/Contents/MacOS/HerdbotMenu"), version $APP_VERSION)"
