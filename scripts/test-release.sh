#!/bin/bash
# Dry-run the release: universal, ad-hoc signed, nothing uploaded. Needs the Rust x86_64 target.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/release.sh --dry-run --no-notarize
version="$(sed -n 's/^version = \"\(.*\)\"/\1/p' Cargo.toml | head -1)"
test -s "dist/Herdbot-$version.zip"
test -s "dist/herdbot.rb"
grep -q "version \"$version\"" dist/herdbot.rb
lipo -archs dist/Herdbot.app/Contents/MacOS/HerdbotMenu | grep -q 'x86_64'
lipo -archs dist/Herdbot.app/Contents/Helpers/herdbot | grep -q 'arm64'
codesign --verify --deep --strict dist/Herdbot.app
unzip -l "dist/Herdbot-$version.zip" | grep -q 'Herdbot.app/Contents/Helpers/herdbot'
echo "release dry run ok"
