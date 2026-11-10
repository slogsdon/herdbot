# Herdr session setting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status (2026-11-09):** CLI and Swift implementation salvaged and reviewed; automated verification recorded below. Manual UI, disposable-session lifecycle, and installed launchd checks remain pending.

**Goal:** Add an optional herdr session setting to Settings, defaulting to `default`, after the session spike establishes a supported lifecycle and socket-resolution path.

**Architecture:** Store the selected session in `herdbot.toml`; resolve its socket through the supported interface found by the spike; keep the existing default behavior until a deliberate setting change. Do not migrate existing workspaces automatically.

**Tech Stack:** Rust edition 2024, `herdr-client`, Swift 6, SwiftUI, Rust/Swift tests.

**Spec:** `docs/superpowers/specs/2026-10-22-settings-design.md`; prerequisite report: `docs/superpowers/research/2026-10-22-herdr-session-spike.md`.

## Global Constraints

- Do not start before the session spike report is complete and confirms a supported socket-resolution method.
- Initial/default value is `default`; never move or recreate existing bot workspaces as a side effect of reading or changing this setting.
- If the spike cannot establish safe behavior for existing bot state, keep named-session selection unavailable and report the blocker rather than guessing.
- Implement in `/Volumes/Dock/Code/herdbot/.worktrees/settings-session-salvage` on `feature/settings-session-salvage`, based on `ab27f29`; do not commit, push, or merge. Report branch, diff, tests, and live check.

## Review Focus

- Missing setting resolves to `default` and preserves current socket behavior.
- Unknown/unavailable session fails before daemon startup with an actionable error.
- Switching session never deletes, moves, or silently recreates a bot workspace.
- Installed launchd service receives the chosen session/socket after an explicit restart or reinstall as determined by the spike.
- The Settings form shows only sessions herdr reports as available and preserves selection on a transient list failure.
- Creating a Herdr session starts only its headless server; it never opens an interactive terminal or changes existing workspaces.

---

### Task 1: Config and socket resolution

**Files:** Modify `crates/herdbot/src/config.rs`, `main.rs`, `crates/herdbot/src/service.rs`, and the herdr socket resolver in `crates/herdr-client/src/lib.rs`; tests in the owning modules.

**Interfaces:** Add `herdbot config sessions --json`, backed by the spike-verified herdr session discovery method, and a `herdr.session` setting with default `default`. Existing `HERDR_SOCKET` continues to override discovery for explicit operator configuration.

- [x] Convert the spike report's supported session-list/socket-resolution result into tests for default, available, unavailable, and malformed session data.
- [x] Add tests proving no bot workspace files are moved or deleted when the selected session changes.
- [x] Implement session metadata and socket resolution using only the spike-verified interface; reject missing or stopped sessions before connecting.
- [x] Preserve lifecycle restart: each new daemon resolves the saved setting; only explicit operator HERDR_SOCKET is captured in launchd. Installed propagation remains unverified.
- [x] Run `cargo test -p herdbot` and `cargo test -p herdr-client`; expect all tests pass.
- [ ] Reverify a disposable named session live (not run: live operations restricted to read-only). Historical evidence is in the spike report.
- [x] Report implementation and verification; do not commit.

### Task 2: Settings session picker

**Files:** Modify settings models/state and Settings view under `ui/macos/Sources/HerdbotKit/` and `ui/macos/Sources/HerdbotMenu/`; tests in `ui/macos/Tests/HerdbotKitTests/`.

**Interfaces:** The picker consumes the session choices from Task 1 metadata and saves the selected value through the existing `SettingsCLI` interface.

- [x] Test default selection, available-session choices, unavailable-list failure, save failure, and retained selection.
- [ ] Original UI red phase not reproduced: code and tests were salvaged together.
- [x] Render the session selector only when the CLI reports named-session support; explain that changing the setting does not move existing workspaces and state the apply/restart behavior.
- [x] Run `cd ui/macos && ./scripts/test.sh`; 197 tests pass.
- [ ] Manually verify selection, saved value, and displayed apply timing.
- [x] Report branch, diff, checks, and live evidence; do not commit.

### Task 3: Create or start a named session from the CLI

**Files:** Modify `crates/herdbot/src/main.rs` and the smallest suitable session helper; add tests and README usage.

**Interfaces:** Add `herdbot session create <name> [--json]`. Use Herdr's supported `herdr --session <name> server` headless entry point, which creates a missing session and starts an existing stopped session. Existing running sessions are an idempotent success. Report success only after Herdr reports the session running with a socket path.

- [x] Test argument validation, already-running idempotence, launch failure, and readiness reporting without touching the user's default session.
- [ ] Reverify disposable headless creation live (not run: read-only live constraint).
- [x] Run `cargo build`, `cargo test -p herdbot`, and `cargo clippy -p herdbot -- -D warnings`.
- [x] Report branch, diff, checks, and live evidence; do not commit.

### Task 4: Add session creation to Settings

**Files:** Modify `SettingsCLI`, `SettingsState`, `SettingsView`, and their tests under `ui/macos/`.

**Interfaces:** Add a name field and Create action to the global session picker. On success, refresh the session list and select the new running session as a draft; saving the setting remains an explicit separate action. Keep CLI errors visible and retain the entered name on failure.

- [x] Test create success, validation/CLI failure, retained name, refreshed choices, and explicit save behavior.
- [x] Run `cd ui/macos && ./scripts/test.sh` and `./scripts/build-app.sh`; tests pass and arm64 app/signature verified.
- [ ] Manually verify create, selection, and save with a disposable session.
- [x] Report branch, diff, checks, and live evidence; do not commit.

## Salvage rulings and evidence

- Applied spike, CLI, then UI patches in order, translating `ui/menubar` paths to `ui/macos`; hand-reconciled this plan, collaboration code, and lifecycle controls. The source branches also include CLI/UI session creation (Tasks 3–4), retained here.
- Retained main's service restart implementation. Capturing a discovered socket in launchd would turn it into a permanent operator override, so the new daemon resolves saved config on each start. Environment setup runs before Tokio worker threads.
- Added persistent per-home socket ownership with exclusive first creation, bot/room state checks, and legacy daemon liveness checks. A different socket requires a separate home; no automatic migration, deletion, or recreation. Legacy homes without a binding use conservative default ownership even when HERDR_SOCKET is supplied; existing custom-socket homes require operator-verified adoption or a separate home.
- Config/read-only snapshots and lifecycle recovery remain usable when the selected session is unavailable. Socket commands, including card answers, resolve the selection. The picker preserves drafts when listing/saving fails.
- Salvaged tests were run after salvage; their original red phase was not reproduced. New ownership and command-routing regressions were observed failing before their fixes, then passing.
- Manual checks for Shane: attach a disposable named session in normal herdr UI; create from CLI/Settings and verify readiness/cleanup; choose/save/reopen a session; verify retained selection on list/save errors; confirm lifecycle controls and apply timing; explicitly restart/reinstall with a suitable separate home and verify the daemon's reported socket and existing default workspaces.

## Final automated verification (2026-11-09)

Evidence is under `.superpowers/sdd/settings-session-salvage/` in this worktree.

- `cargo build --workspace`: exit 0, `Finished dev profile [unoptimized + debuginfo] target(s) in 1.73s`.
- `cargo test --workspace`: exit 0; 162 herdbot + 34 core + 5 client + 2 doctests passed, zero failures. An earlier final attempt failed in unchanged `daemon::tests::liveness_probes_never_fill_the_lock_backlog` (`probe 367 refused`); the full rerun passed. Likely: scheduling-sensitive backlog test (same unchanged test passed before and after); no daemon code changed.
- `cargo clippy --workspace -- -D warnings`: exit 0, `Finished dev profile [unoptimized + debuginfo] target(s) in 0.63s`.
- `cd ui/macos && ./scripts/test.sh`: exit 0, `Test run with 197 tests in 20 suites passed after 0.395 seconds.`
- `OUT_DIR=build/session-salvage ./scripts/build-app.sh`: exit 0, `built build/session-salvage/Herdbot.app (arm64, version 0.1.2)`; plist and signature verification passed. App was not launched or installed.
- CLI fixtures: version/config/status work with an unavailable selected session; daemon rejects it before binding; explicit socket override cannot adopt existing workspace IDs; state contents remain unchanged.
- Fresh source review found and fixed card-answer routing, room marker coverage, snapshot availability, legacy-override ownership and concurrent first-binding defects. Final source recheck found no important remaining defects.
- `git diff --check`: clean. No commit, install, daemon restart, or existing-session mutation.

Reflection: salvage retained useful CLI/UI tests, but socket discovery alone did not establish safe workspace ownership. The review and failing fixture tests exposed that gap; future session work must review workspace IDs and inherited environment together. No vault writes were made under the worktree-only scope.
