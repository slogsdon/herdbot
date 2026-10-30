# UI package directory rename

## Task 1 — move the Swift package under `ui/macos`

- Worktree: `/Volumes/Dock/Code/herdbot/.worktrees/ui-macos-rename-tier2`
- Branch: `feature/ui-macos-rename-tier2`
- Preserved package/module names `HerdbotKit` and `HerdbotMenu`, and app name `Herdbot`.
- Moved the Swift package, tests, scripts, and Info.plist; updated every tracked reference to the former package path in README, release script, plans, specs, usability notes, task list, and existing SDD ledger.
- `cargo build`: passed.
- `cargo test -p herdbot`: 144 passed.
- `cargo clippy -p herdbot -- -D warnings`: passed.
- `cd ui/macos && ./scripts/test.sh`: 156 tests in 17 suites passed.
- `cd ui/macos && ./scripts/build-app.sh`: produced `build/Herdbot.app`; executable, Info.plist, and strict codesign verification passed.
- `scripts/test-release.sh`: universal `arm64`/`x86_64` dry run passed; zip, helper, cask, and signatures checked; nothing uploaded.
- `git diff --check`: passed.
- Stale path scan: no tracked matches.
- No app launch, install, daemon restart, commit, or push.
- Three delegated attempts failed before repository access; this implementation was completed directly in the isolated worktree.
- EasyUI visual patterns do not apply: no UI behavior or presentation changed.
