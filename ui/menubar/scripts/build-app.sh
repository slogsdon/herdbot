#!/bin/bash
# Build build/HerdbotMenu.app. Notifications need a bundle id; the signature is ad-hoc.
set -euo pipefail
cd "$(dirname "$0")/.."
swift build -c release
bin="$(swift build -c release --show-bin-path)/HerdbotMenu"
app="build/HerdbotMenu.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/HerdbotMenu"
cp Info.plist "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
echo "built $app"
