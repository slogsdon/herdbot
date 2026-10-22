#!/bin/bash
# Dry-run the release: universal, ad-hoc signed, nothing uploaded. Needs the Rust x86_64 target.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/release.sh --dry-run --no-notarize
version="$(sed -n 's/^version = \"\(.*\)\"/\1/p' Cargo.toml | head -1)"
test -s "dist/Herdbot-$version.zip"
test -s "dist/herdbot.rb"
grep -q "version \"$version\"" dist/herdbot.rb
codesign --verify --deep --strict dist/Herdbot.app
# Capture first: `grep -q` closing a pipe early makes the producer die of SIGPIPE under pipefail.
for binary in MacOS/HerdbotMenu Helpers/herdbot; do
  archs="$(lipo -archs "dist/Herdbot.app/Contents/$binary")"
  [[ " $archs " == *" x86_64 "* && " $archs " == *" arm64 "* ]] || { echo "Contents/$binary has: $archs" >&2; exit 1; }
done
listing="$(unzip -l "dist/Herdbot-$version.zip")"
[[ "$listing" == *Herdbot.app/Contents/Helpers/herdbot* ]] || { echo "zip lacks the helper" >&2; exit 1; }
echo "release dry run ok"
