# Packaging and first run: Design

Approved 2026-10-22. Goal: a new user who already has herdr and at least one harness CLI installs herdbot and sends their first message without a Rust or Swift toolchain, and without guessing why something doesn't work. Scope is herdbot only; herdr and the harnesses are assumed present.

## Non-goals

- Auto-update (Sparkle or similar). Upgrades go through `brew upgrade --cask` or a new zip.
- CI. The release script is written to run anywhere so a workflow can wrap it later.
- Linux and Windows.
- Checking that a harness is signed in. It can't be verified from outside; the existing cooldown handling covers auth failures at runtime.
- Submitting to the official Homebrew cask repo. The cask lives in the author's own tap.

## Decisions

| Question | Decision | Why |
|---|---|---|
| Scope | CLI and menu bar app together | The app is half the product now; a CLI-only package would leave the UI as a build-it-yourself step |
| Signing | Developer ID, hardened runtime, notarized and stapled | No Gatekeeper warning on first launch; the author has or will get a membership |
| Packaging | One cask, one `Herdbot.app` with the CLI at `Contents/Helpers/herdbot` | One download, one version stream |
| First-run setup | CLI is the source of truth (`setup`, `doctor --json`); the app renders it | Keeps the rule that the app only talks to the CLI; the checks exist once |
| Release build | Local `scripts/release.sh` on the author's Mac | Certificate and Apple credentials stay on the machine; no CI to debug first |
| macOS floor | 15 (the app's current floor) | Unchanged |
| Architectures | Universal (arm64 and x86_64) for the CLI and the app | Assumption: x86_64 is wanted. Dropping it halves build time |
| Bundle id | `dev.herdbot.menu` stays; display name becomes Herdbot | Notification permission and per-home unread counts carry over. Assumption: no new id wanted |

## Artifact

`Herdbot.app`:

```
Herdbot.app/Contents/MacOS/HerdbotMenu     the Swift app (unchanged executable name)
Herdbot.app/Contents/Helpers/herdbot       the Rust CLI and daemon, universal
Herdbot.app/Contents/Info.plist            version stamped by release.sh from Cargo.toml
```

The helper is signed first with the hardened runtime, then the app, then the whole bundle is notarized and stapled. The version comes from the workspace `Cargo.toml`; `release.sh` writes it into `CFBundleShortVersionString`.

## Stable binary path

The daemon plist, the MCP configs and the panes all record the path of the running `herdbot`. The recorded path must be the one the user installed, not a resolved one, or an update that replaces the bundle breaks every recorded reference.

Verified 2026-10-22: `service.rs:41` canonicalizes the exe; `route.rs:232` and `run.rs:141` use `current_exe()` as is. The first task is to make all three record the invoked path, `/Applications/Herdbot.app/Contents/Helpers/herdbot` or the `/opt/homebrew/bin/herdbot` link, and to check what `current_exe()` returns when started through the link (unverified: it may return the link or its target). Moving the app is then caught by `doctor`.

## CLI additions (Rust)

- `herdbot --version`, from `CARGO_PKG_VERSION`.
- The daemon writes `.state/daemon.json` (`{"version","pid","exe"}`) on start and removes it on clean exit.
- `herdbot doctor [--json]`. Each check is `{id, status: ok|warn|fail, message, fix?}`. Checks:
  - `herdr`: socket reachable and protocol compatible (tested with 0.9.1, protocol 22).
  - `harnesses`: at least one of claude, codex, cursor-agent, pi on `PATH`; a missing one is `warn`, none is `fail`.
  - `home`: `HERDBOT_HOME` exists and `herdbot.toml` is present (fix: `home`).
  - `service`: plist installed and pointing at this binary (fix: `service`).
  - `daemon`: running, and `daemon.json` version equals `--version` (fix: `restart-daemon`).
  - `cli-link`: a `herdbot` on `PATH` resolves to this binary (fix: `cli-link`).
  - `location`: binary is not under `target/`.
  - `login-item`: app registered to start at login; reported by the app, not the CLI (fix: `login-item`).
- `herdbot setup [--yes] [--only <fix>]`. Runs the fixes in order: create the home and a commented `herdbot.toml`; link the CLI into the first writable directory of `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin`; `service install`; restart the daemon if stale; offer a starter bot `helper` with a real `BOT.md` template. Ends with the `doctor` summary. `--only` runs one fix (the app uses this); `--yes` skips prompts.
- `herdbot uninstall [--purge]`. Removes the service and the CLI link. `--purge` also removes the home directory after a typed confirmation.
- `service install` is allowed from inside an app bundle. It still refuses `target/`.

## App additions (Swift)

The app's only seam stays the CLI.

- A first-run window listing `doctor --json` checks with a Fix button per failing check, which runs `herdbot setup --only <fix> --yes`. It opens automatically when a check is `fail` and the user hasn't dismissed it for this version.
- The menu bar icon turns amber while any check is `warn` or `fail`.
- A daemon-version mismatch shows a "Restart daemon" action (the `restart-daemon` fix).
- Login-item registration through `SMAppService.mainApp`, as the `login-item` check and fix. This closes the login-item item deferred in the UI layer design.
- The existing `HERDBOT_BIN` and install-spot lookup gains `Bundle.main`'s `Contents/Helpers/herdbot` as the first candidate.

## Release and distribution

`scripts/release.sh`:

1. Build the universal CLI (`cargo build --release` for both targets, `lipo`).
2. Build the universal app (`ui/menubar/scripts/build-app.sh`), embed the helper.
3. Sign inside-out with the Developer ID identity and hardened runtime.
4. Notarize with `notarytool` (a keychain profile), staple, zip.
5. `gh release create v<version>` with the zip and its sha256.
6. Update the cask in the tap, or print the version and sha256 to paste.

Flags: `--no-notarize` (smoke test, ad-hoc or Developer ID signed), `--dry-run` (build and sign only, no upload). The script takes the identity and notary profile from environment variables and fails early if either is missing.

Cask `herdbot` in `slogsdon/homebrew-tap`: `app "Herdbot.app"`; a `binary` stanza linking `/opt/homebrew/bin/herdbot` to the helper; `uninstall launchctl: "dev.herdbot.daemon"`; `zap` for the app's preferences. The home directory is left alone by `zap` unless the user passes `--purge`.

A direct zip download works without Homebrew: the first-run window creates the CLI link and the service itself.

## Updates

An update replaces the bundle and leaves the old daemon process running old code. Likely (basis: replacing files by rename doesn't kill a running process; not tested): the old daemon keeps running until restarted. `doctor`'s `daemon` check catches it, and the app offers the restart. Open agents live in herdr, and queued messages survive a daemon restart, so a restart is safe.

## Testing

- Unit tests for each doctor check against a faked environment (fake socket, fake `PATH`, temp home).
- `setup` and `uninstall` run in a temp `HOME` and `HERDBOT_HOME`, asserting files created and removed.
- `release.sh --no-notarize --dry-run`: assert `codesign --verify --deep --strict` and `spctl -a -vv` on the result; assert the helper is universal with `lipo -archs`.
- By hand, with the author (credentials needed): real notarization, then a fresh-user test on a new macOS user account or a clean VM covering the first-run window, `brew install --cask herdbot`, and an upgrade with a running daemon.

## Plan breakdown

1. **CLI:** `--version`, the recorded-path fix, `daemon.json`, `doctor`, `setup`, `uninstall`. Shippable alone (a source install gains the first-run checks).
2. **App:** the helper lookup, the first-run window, the amber icon, restart action, login item.
3. **Release:** `release.sh`, the tap and cask, the fresh-user test.

## Risks and open items

- **Unverified:** what `current_exe()` returns when the binary is started through a symlink; the plan's first task checks it.
- **Unverified:** that a stapled, notarized bundle keeps passing Gatekeeper after Homebrew's quarantine handling; checked in the fresh-user test.
- **Assumption:** `SMAppService.mainApp` works for an agent app (`LSUIElement`) on macOS 15; if not, the login item falls back to a LaunchAgent plist written by `setup`.
- The cask `binary` link points into the app bundle, so deleting the app by hand leaves a dangling link; `doctor` reports it.
