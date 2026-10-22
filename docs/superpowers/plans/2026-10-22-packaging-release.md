# Packaging release (plan 3 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One command, `scripts/release.sh`, builds a universal `Herdbot.app` (Swift app plus the Rust CLI at `Contents/Helpers/herdbot`), signs it inside-out, optionally notarizes and staples it, zips it, and prepares a GitHub release and a Homebrew cask.

**Architecture:** Spec: `docs/superpowers/specs/2026-10-22-packaging-design.md` ("Artifact", "Release and distribution"). `ui/menubar/scripts/build-app.sh` learns to assemble a bundle from environment inputs (arch list, helper binary, signing identity, version); `scripts/release.sh` drives the Rust and Swift builds around it, then signs, notarizes, zips and publishes. The cask template lives in `packaging/herdbot.rb.tmpl` and is rendered with the version and sha256.

**Tech Stack:** bash, `cargo`, `swift build --triple`, `lipo`, `codesign`, `xcrun notarytool`/`stapler`, `ditto`, `gh`, Homebrew cask DSL.

## Global Constraints

- macOS 15 floor (`LSMinimumSystemVersion` stays 15.0). Universal means arm64 and x86_64 for both the app and the helper.
- Bundle id `dev.herdbot.menu` and the executable name `HerdbotMenu` do not change. The bundle is `Herdbot.app`; `CFBundleName` and `CFBundleDisplayName` become `Herdbot`.
- Signing order: helper first (hardened runtime, timestamp), then the app bundle. Never `codesign --deep` to sign; `--deep` is only used for verification.
- The version is the workspace `Cargo.toml` `version`, stamped into `CFBundleShortVersionString`; `CFBundleVersion` is the same string.
- The release script must be usable by the author with no CI, and fail early with a clear message when a prerequisite is missing. Credentials come from the environment or the keychain, never from files in the repo.
- **An agent running this plan has no Developer ID certificate and no notarization credentials.** It verifies everything with `--no-notarize --dry-run` and ad-hoc signing. Real signing, notarization, `gh release create` and the fresh-user test are Shane's, listed under "Prerequisites and hand-off".
- Never run `gh release create`, `git push`, `brew install`, `cargo install`, `launchctl`, or touch the real `~/Bots` or `/Applications`. Output goes under `dist/` (git-ignored).
- Commits: conventional messages, no AI attribution, explicit `git add` paths, no push.

**Branch/worktree:** from `feature/packaging-app` (or `main` once plans 1 and 2 are merged): `git -C ~/Code/herdbot worktree add .worktrees/packaging-release -b feature/packaging-release <base>`.

## Prerequisites and hand-off (Shane, not the agent)

1. `rustup target add x86_64-apple-darwin` (only `aarch64-apple-darwin` is installed; verified 2026-10-22).
2. A Developer ID Application certificate in the login keychain; note its identity string (`security find-identity -v -p codesigning`).
3. A notarytool profile: `xcrun notarytool store-credentials herdbot-notary --apple-id … --team-id … --password <app-specific password>`.
4. A tap repo `slogsdon/homebrew-tap` (does not exist yet; checked 2026-10-22) with a `Casks/` directory. The release repo `slogsdon/herdbot` is public (checked).
5. The real run: `SIGN_IDENTITY="Developer ID Application: … (TEAMID)" NOTARY_PROFILE=herdbot-notary scripts/release.sh`, then the fresh-user test (Task 4).

## File map

- Modify `ui/menubar/scripts/build-app.sh`, `ui/menubar/Info.plist`, `ui/menubar/README.md`.
- Create `scripts/release.sh`, `packaging/herdbot.rb.tmpl`, `scripts/test-release.sh`.
- Modify `.gitignore` (add `dist/`), `README.md`.

---

### Task 1: `build-app.sh` assembles the bundle from inputs

**Files:** Modify `ui/menubar/scripts/build-app.sh`, `ui/menubar/Info.plist`.

**Interfaces:**
- Consumes (environment, all optional): `ARCHS` (default: the host arch only, e.g. `arm64`; space-separated, e.g. `"arm64 x86_64"`), `HERDBOT_HELPER` (path to a `herdbot` binary to embed), `SIGN_IDENTITY` (default `-` = ad-hoc), `APP_VERSION` (default: read from the repo's `Cargo.toml`), `OUT_DIR` (default `build`).
- Produces: `$OUT_DIR/Herdbot.app` with `Contents/MacOS/HerdbotMenu`, `Contents/Helpers/herdbot` (when given), and `Contents/Info.plist`, signed.

- [ ] **Step 1: Info.plist.** Set `CFBundleName` to `Herdbot`, add `<key>CFBundleDisplayName</key><string>Herdbot</string>`, keep `CFBundleExecutable` = `HerdbotMenu` and `CFBundleIdentifier` = `dev.herdbot.menu`.

- [ ] **Step 2: Rewrite the script.**

```bash
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
APP_VERSION="${APP_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1)}"
[ -n "$APP_VERSION" ] || { echo "build-app: could not read the version from Cargo.toml" >&2; exit 1; }

# One cross-compiled build per architecture, then lipo: a multi-arch `swift build` needs full Xcode,
# a --triple build works with Command Line Tools (checked 2026-10-22).
slices=()
for arch in $ARCHS; do
  swift build -c release --triple "$arch-apple-macosx15.0" >&2
  slices+=("$(swift build -c release --triple "$arch-apple-macosx15.0" --show-bin-path)/HerdbotMenu")
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
```

- [ ] **Step 3: Verify.** From `ui/menubar`: `./scripts/build-app.sh` (host arch, no helper) then `plutil -extract CFBundleShortVersionString raw build/Herdbot.app/Contents/Info.plist` equals the `Cargo.toml` version, `codesign --verify --deep --strict build/Herdbot.app`, and `lipo -archs build/Herdbot.app/Contents/MacOS/HerdbotMenu` prints `arm64`. Then `ARCHS="arm64 x86_64" HERDBOT_HELPER=$(command -v true) OUT_DIR=/tmp/…/out ./scripts/build-app.sh` (use a scratch dir) and assert `lipo -archs` prints both architectures and `test -x …/Contents/Helpers/herdbot`. (`true` is a stand-in; the real helper is built in Task 2.) The x86_64 Swift cross-build was confirmed to work under Command Line Tools on this machine.

- [ ] **Step 4: Update references** to `build/HerdbotMenu.app` in `ui/menubar/README.md` (lines 12, 13, 27, 61, 95) to `build/Herdbot.app`; the inner executable path stays `Contents/MacOS/HerdbotMenu`.

- [ ] **Step 5: Commit.**

```bash
git add ui/menubar/scripts/build-app.sh ui/menubar/Info.plist ui/menubar/README.md
git commit -m "build(menubar): assemble a universal Herdbot.app with the bundled CLI"
```

---

### Task 2: `scripts/release.sh`

**Files:** Create `scripts/release.sh`, `scripts/test-release.sh`; modify `.gitignore`.

**Interfaces:**
- Consumes: `ui/menubar/scripts/build-app.sh` (Task 1); environment `SIGN_IDENTITY`, `NOTARY_PROFILE`, `ARCHS` (default `arm64 x86_64`), `TAP_DIR` (optional path to a checkout of the tap repo).
- Produces: `dist/Herdbot-<version>.zip`, `dist/Herdbot-<version>.zip.sha256`, `dist/herdbot.rb` (rendered cask), and with no `--dry-run` a GitHub release `v<version>`.

- [ ] **Step 1: Write `scripts/release.sh`.**

```bash
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
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
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
  rustup target list --installed | grep -q "^$arch-apple-darwin$" || fail "missing Rust target: rustup target add $arch-apple-darwin"
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
  cargo build --release -p herdbot --target "$arch-apple-darwin"
  slices+=("$repo/target/$arch-apple-darwin/release/herdbot")
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
```

`chmod +x scripts/release.sh`. Add `dist/` to `.gitignore`.

Note on step 4 under ad-hoc: `codesign --verify --deep --strict` passes for ad-hoc signatures; `spctl --assess` would reject them and is deliberately not run here (it belongs to the real-run hand-off).

- [ ] **Step 2: `scripts/test-release.sh`** (the dry-run smoke test, runnable by an agent):

```bash
#!/bin/bash
# Dry-run the release: universal, ad-hoc signed, nothing uploaded. Needs the Rust x86_64 target.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/release.sh --dry-run --no-notarize
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
test -s "dist/Herdbot-$version.zip"
test -s "dist/herdbot.rb"
grep -q "version \"$version\"" dist/herdbot.rb
lipo -archs dist/Herdbot.app/Contents/MacOS/HerdbotMenu | grep -q 'x86_64' 
lipo -archs dist/Herdbot.app/Contents/Helpers/herdbot | grep -q 'arm64'
codesign --verify --deep --strict dist/Herdbot.app
unzip -l "dist/Herdbot-$version.zip" | grep -q 'Herdbot.app/Contents/Helpers/herdbot'
echo "release dry run ok"
```

- [ ] **Step 3: Verify.** If `rustup target list --installed` lacks `x86_64-apple-darwin`, run `ARCHS=arm64 scripts/release.sh --dry-run --no-notarize` and check the `dist/` outputs by hand (zip, sha256, `herdbot.rb`, one architecture); the full `scripts/test-release.sh` then waits for Shane's `rustup target add`. Say which you ran. Also check the failure paths print their message and exit non-zero: `scripts/release.sh --bogus`; `scripts/release.sh` (no identity, not dry-run); `SIGN_IDENTITY=x scripts/release.sh` (identity not in keychain).

- [ ] **Step 4: Commit.**

```bash
git add scripts/release.sh scripts/test-release.sh .gitignore
git commit -m "build: release script for the universal signed app and cask"
```

---

### Task 3: The cask template and docs

**Files:** Create `packaging/herdbot.rb.tmpl`; modify `README.md`, `ui/menubar/README.md`.

- [ ] **Step 1: `packaging/herdbot.rb.tmpl`.**

```ruby
cask "herdbot" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "https://github.com/slogsdon/herdbot/releases/download/v#{version}/Herdbot-#{version}.zip"
  name "Herdbot"
  desc "Bots on herdr: registry, routing, subagent panes and messaging"
  homepage "https://github.com/slogsdon/herdbot"

  depends_on macos: ">= :sequoia"

  app "Herdbot.app"
  binary "#{appdir}/Herdbot.app/Contents/Helpers/herdbot"

  uninstall launchctl: "dev.herdbot.daemon",
            quit:      "dev.herdbot.menu"

  zap trash: [
    "~/Library/LaunchAgents/dev.herdbot.daemon.plist",
    "~/Library/Preferences/dev.herdbot.menu.plist",
  ]
end
```

The bots, memory and rooms under `~/Bots` are deliberately not in `zap`; `herdbot uninstall --purge` is the way to delete them.

- [ ] **Step 2: README.** In the top-level `README.md`, add an **Install** section before "Quick start": `brew install --cask slogsdon/tap/herdbot` (after the tap exists), then open Herdbot (menu bar) and use its setup window, or run `herdbot setup` in a terminal; the zip from GitHub Releases works the same way (drag to Applications, open it, press "Set up everything"). Keep "From source" as the existing `cargo install` path. Add a **Releasing** section for the author: the prerequisites above, the exact command, `--dry-run`/`--no-notarize`, and that `scripts/test-release.sh` is the smoke test. In `ui/menubar/README.md` replace the build instructions with the env-driven `build-app.sh` description (`ARCHS`, `HERDBOT_HELPER`, `SIGN_IDENTITY`, `APP_VERSION`, `OUT_DIR`).

- [ ] **Step 3: Verify.** Render the template as the script does with dummy values and check Ruby syntax: `sed -e 's/@VERSION@/0.0.0/' -e 's/@SHA256@/0/' packaging/herdbot.rb.tmpl > $TMP/herdbot.rb && ruby -c $TMP/herdbot.rb` prints `Syntax OK`. If `brew` is installed, `brew style --cask $TMP/herdbot.rb` is a bonus; do not run `brew install`.

- [ ] **Step 4: Commit.**

```bash
git add packaging/herdbot.rb.tmpl README.md ui/menubar/README.md
git commit -m "docs: install and release instructions, and the cask template"
```

---

### Task 4: Fresh-user test (Shane, with the real signed build)

Not an agent task. After Shane runs the real release (Prerequisites 1-5) and pushes the cask, on a new macOS user account or a clean VM with herdr and one harness installed:

1. `brew install --cask slogsdon/tap/herdbot`. Expect no Gatekeeper prompt, and `herdbot --version` works (the `binary` link).
2. Open Herdbot. Expect the setup window (service/daemon failing, cli-link ok). Press "Set up everything". Expect the checks to turn ok, a starter bot `helper`, and the menu bar icon to leave the triangle state.
3. `herdbot send helper "hello"`; the bot's workspace opens in herdr and replies.
4. "Start at login" in the setup window registers the login item; log out and in, and the app is running. If `SMAppService` fails here, fall back to a LaunchAgent plist written by `setup` (spec risk) as a follow-up.
5. Upgrade: bump the workspace version, release again, `brew upgrade --cask herdbot` with the daemon running. Expect the setup window's daemon row to fail with "runs 0.1.0, the binary on disk is …", and "Restart daemon" to fix it.
6. `herdbot uninstall` then `brew uninstall --cask herdbot`; `~/Bots` is still there. Record anything that differed from this list.

---

## Self-review (done)

- Spec coverage: bundle layout and inside-out signing (T1), universal builds (T1-2), version stamping (T1), notarize/staple/zip (T2), `gh release create` (T2), cask with `binary`, `uninstall launchctl`, `zap` (T3), dry-run and no-notarize flags (T2), fail-early prerequisites (T2), fresh-user test (T4). The `spctl` assessment and real notarization are Shane's by necessity.
- The `--triple` per-arch Swift build is used instead of a multi-arch `swift build` because the latter needs full Xcode (the former was built and checked as x86_64 here).
- Unverified: that the hardened-runtime app needs no entitlements (SwiftUI, Carbon hotkey, subprocesses, notifications; the real notarized run will say), and that Homebrew's `binary` stanza accepts a path inside `appdir` (documented stanza; `brew style` and the fresh-user test cover it).
- Names are consistent: env vars `ARCHS`, `HERDBOT_HELPER`, `SIGN_IDENTITY`, `APP_VERSION`, `OUT_DIR`, `NOTARY_PROFILE`, `TAP_DIR`; paths `dist/`, `packaging/herdbot.rb.tmpl`.
