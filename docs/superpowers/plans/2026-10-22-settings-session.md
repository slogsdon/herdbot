# Herdr session setting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an optional herdr session setting to Settings, defaulting to `default`, after the session spike establishes a supported lifecycle and socket-resolution path.

**Architecture:** Store the selected session in `herdbot.toml`; resolve its socket through the supported interface found by the spike; keep the existing default behavior until a deliberate setting change. Do not migrate existing workspaces automatically.

**Tech Stack:** Rust edition 2024, `herdr-client`, Swift 6, SwiftUI, Rust/Swift tests.

**Spec:** `docs/superpowers/specs/2026-10-22-settings-design.md`; prerequisite report: `docs/superpowers/research/2026-10-22-herdr-session-spike.md`.

## Global Constraints

- Do not start before the session spike report is complete and confirms a supported socket-resolution method.
- Initial/default value is `default`; never move or recreate existing bot workspaces as a side effect of reading or changing this setting.
- If the spike cannot establish safe behavior for existing bot state, keep named-session selection unavailable and report the blocker rather than guessing.
- Implement in a dedicated worktree from `feature/roadmap`; do not commit, push, or merge. Report branch, diff, tests, and live check.

## Review Focus

- Missing setting resolves to `default` and preserves current socket behavior.
- Unknown/unavailable session fails before daemon startup with an actionable error.
- Switching session never deletes, moves, or silently recreates a bot workspace.
- Installed launchd service receives the chosen session/socket after an explicit restart or reinstall as determined by the spike.
- The Settings form shows only sessions herdr reports as available and preserves selection on a transient list failure.

---

### Task 1: Config and socket resolution

**Files:** Modify `crates/herdbot/src/config.rs`, `main.rs`, `crates/herdbot/src/service.rs`, and the herdr socket resolver in `crates/herdr-client/src/lib.rs`; tests in the owning modules.

**Interfaces:** Add `herdbot config sessions --json`, backed by the spike-verified herdr session discovery method, and a `herdr.session` setting with default `default`. Existing `HERDR_SOCKET` continues to override discovery for explicit operator configuration.

- [ ] Convert the spike report's supported session-list/socket-resolution result into tests for default, available, unavailable, and malformed session data.
- [ ] Add failing tests proving no bot workspace files are moved or deleted when the selected session changes.
- [ ] Implement session metadata and socket resolution using only the spike-verified interface; reject missing sessions before connecting.
- [ ] Ensure service installation/restart propagates the selected session/socket using the existing launchd environment mechanism.
- [ ] Run `cargo test -p herdbot` and `cargo test -p herdr-client`; expect all tests pass.
- [ ] Run a live check against a disposable named session; verify default session and existing bot workspace remain untouched.
- [ ] Report implementation and verification; do not commit.

### Task 2: Settings session picker

**Files:** Modify settings models/state and Settings view under `ui/menubar/Sources/HerdbotKit/` and `ui/menubar/Sources/HerdbotMenu/`; tests in `ui/menubar/Tests/HerdbotKitTests/`.

**Interfaces:** The picker consumes the session choices from Task 1 metadata and saves the selected value through the existing `SettingsCLI` interface.

- [ ] Test default selection, available-session choices, unavailable-list failure, save failure, and retained selection.
- [ ] Run `cd ui/menubar && ./scripts/test.sh`; confirm failures before implementation.
- [ ] Render the session selector only when the CLI reports named-session support; explain that changing the setting does not move existing workspaces and state the apply/restart behavior.
- [ ] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.
- [ ] Manually verify selection, saved value, and displayed apply timing.
- [ ] Report branch, diff, checks, and live evidence; do not commit.
