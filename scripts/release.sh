#!/bin/bash
# Build, sign, (notarize,) zip and publish Herdbot.app. Run from anywhere.
#   scripts/release.sh [--no-notarize] [--dry-run]
# Environment:
#   SIGN_IDENTITY   Developer ID Application identity. Required unless --dry-run (then ad-hoc is used).
#   NOTARY_PROFILE  notarytool keychain profile. Required unless --no-notarize.
#   ARCHS           default "arm64 x86_64"
#   TAP_DIR         a checkout of the Homebrew tap; the rendered cask is copied to Casks/herdbot.rb (not committed)
# --dry-run builds and signs only: no upload, no tag. --no-notarize skips notarytool and stapling.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"

notarize=1 dry=0
for arg in "$@"; do
  case "$arg" in
    --no-notarize) notarize=0 ;;
    --dry-run) dry=1 ;;
    *) echo "release: unknown argument $arg" >&2; exit 2 ;;
  esac
done

fail() { echo "release: $*" >&2; exit 1; }

ARCHS="${ARCHS:-arm64 x86_64}"
version="$(sed -n 's/^version = \"\(.*\)\"/\1/p' Cargo.toml | head -1)"
[ -n "$version" ] || fail "could not read the version from Cargo.toml"

if [ "$dry" = 1 ]; then
  SIGN_IDENTITY="${SIGN_IDENTITY:--}"
else
  [ -n "${SIGN_IDENTITY:-}" ] && [ "$SIGN_IDENTITY" != "-" ] || fail "set SIGN_IDENTITY to a Developer ID Application identity (security find-identity -v -p codesigning)"
  security find-identity -v -p codesigning | grep -qF "$SIGN_IDENTITY" || fail "no code-signing identity matching '$SIGN_IDENTITY' in the keychain"
fi
if [ "$notarize" = 1 ] && [ "$dry" = 0 ]; then
  [ -n "${NOTARY_PROFILE:-}" ] || fail "set NOTARY_PROFILE to a notarytool keychain profile, or pass --no-notarize"
fi
for arch in $ARCHS; do
  rust_arch="$arch"
  [ "$arch" = arm64 ] && rust_arch=aarch64
  rustup target list --installed | grep -q "^$rust_arch-apple-darwin$" || fail "missing Rust target: rustup target add $rust_arch-apple-darwin"
done
if [ "$dry" = 0 ]; then
  command -v gh >/dev/null || fail "gh is required to publish"
  [ -z "$(git status --porcelain --untracked-files=no)" ] || fail "the working tree has uncommitted changes"
  git rev-parse "v$version" >/dev/null 2>&1 && fail "tag v$version already exists; bump the workspace version"
fi

dist="$repo/dist"
rm -rf "$dist"
mkdir -p "$dist/helper"

# 1. The CLI, one slice per architecture, then lipo.
slices=()
for arch in $ARCHS; do
  rust_arch="$arch"
  [ "$arch" = arm64 ] && rust_arch=aarch64
  cargo build --release -p herdbot --target "$rust_arch-apple-darwin"
  slices+=("$repo/target/$rust_arch-apple-darwin/release/herdbot")
done
if [ "${#slices[@]}" -eq 1 ]; then cp "${slices[0]}" "$dist/helper/herdbot"; else lipo -create "${slices[@]}" -output "$dist/helper/herdbot"; fi

# 2. The app with the helper inside, signed inside-out.
ARCHS="$ARCHS" HERDBOT_HELPER="$dist/helper/herdbot" SIGN_IDENTITY="$SIGN_IDENTITY" \
  APP_VERSION="$version" OUT_DIR="$dist" "$repo/ui/menubar/scripts/build-app.sh"
app="$dist/Herdbot.app"
zip="$dist/Herdbot-$version.zip"

# 3. Notarize and staple, then zip what was stapled.
if [ "$notarize" = 1 ] && [ "$dry" = 0 ]; then
  ditto -c -k --keepParent "$app" "$dist/notarize.zip"
  xcrun notarytool submit "$dist/notarize.zip" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  rm "$dist/notarize.zip"
fi
ditto -c -k --keepParent "$app" "$zip"

# 4. Checks that must hold for any build.
codesign --verify --deep --strict "$app"
for binary in "$app/Contents/MacOS/HerdbotMenu" "$app/Contents/Helpers/herdbot"; do
  archs="$(lipo -archs "$binary")"
  for arch in $ARCHS; do case " $archs " in *" $arch "*) ;; *) fail "$binary is missing $arch (has: $archs)" ;; esac; done
done
"$app/Contents/Helpers/herdbot" --version | grep -qx "herdbot $version" || fail "the bundled herdbot does not report version $version"

# 5. The cask.
sha="$(shasum -a 256 "$zip" | cut -d' ' -f1)"
echo "$sha  Herdbot-$version.zip" > "$zip.sha256"
sed -e "s/@VERSION@/$version/" -e "s/@SHA256@/$sha/" packaging/herdbot.rb.tmpl > "$dist/herdbot.rb"
test -s "$dist/herdbot.rb" && ! grep -q '@' "$dist/herdbot.rb" || fail "cask template left placeholders"

if [ "$dry" = 1 ]; then
  echo "dry run: built $zip ($sha); nothing uploaded"
  exit 0
fi

# 6. Publish. The tap is copied into but never committed or pushed.
gh release create "v$version" "$zip" "$zip.sha256" --title "herdbot $version" --generate-notes
if [ -n "${TAP_DIR:-}" ]; then
  mkdir -p "$TAP_DIR/Casks"
  cp "$dist/herdbot.rb" "$TAP_DIR/Casks/herdbot.rb"
  echo "copied the cask to $TAP_DIR/Casks/herdbot.rb; review, commit and push it"
else
  echo "cask at $dist/herdbot.rb; copy it to the tap's Casks/herdbot.rb"
fi
