# Settings setup and daemon lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** implementation complete and automated-verified in `feature/settings-lifecycle-salvage` (uncommitted, 2026-10-22); manual UI checks deferred by Shane. Salvaged from the unreviewed `feature/settings-lifecycle-ui` WIP (`1daf59e`) and reviewed against this plan: disable now sets launchd's persistent override (a plain bootout came back at the next login), enable clears it and only bootstraps when the job is not loaded, `service status` tells "installed but not running" from "not installed", and `setup --only restart-daemon` loads a disabled service before restarting it.

**Goal:** Expose setup rerun, daemon disable/enable, and uninstall from Settings with accurate confirmation and status feedback.

**Architecture:** Reuse the existing SetupState/SetupView and CLI service commands; add only missing reversible daemon disable/enable operations, then expose actions in Settings.

**Tech Stack:** Rust edition 2024, Swift 6, SwiftUI, AppKit, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-10-22-settings-design.md`

## Global Constraints

- Depends on Settings navigation and CLI config plan for the Settings destination.
- Disable is reversible and distinct from uninstall.
- Uninstall keeps bot data by default; purge retains existing typed-path confirmation.
- The herdr session setting defaults to `default`; changing it never silently moves existing bot workspaces.
- No daemon restart or removal occurs without explicit user action.
- Implement in a dedicated worktree from `feature/roadmap`; do not commit, push, or merge. Report branch, diff, checks, and manual results.

## Review Focus

- Disable must keep the launchd plist and permit re-enable; test state transitions.
- Uninstall copy must say bot/memory data remains by default; test typed purge confirmation.
- Setup rerun must use existing setup behavior and refresh doctor state after completion.
- CLI failures remain visible and do not show false success.

---

### Task 1: Reversible daemon control CLI

**Files:** Modify `crates/herdbot/src/service.rs`, `crates/herdbot/src/main.rs`; tests in service module.

**Interfaces:** Add explicit service disable/enable operations that retain the installed plist and report launchd errors.

- [x] Add tests for disabled/enabled/install state and confirm disable does not remove the plist or CLI link.
- [x] Run `cargo test -p herdbot`; all pass. The salvage arrived with implementation and tests together, so the red step was not reproduced; the rewritten tests pin the `disable`/`enable` call sequences and the `installed but not running` status the salvaged code did not have.
- [x] Implement launchd disable/bootout and enable/bootstrap using existing service path conventions. `disable` sets launchd's persistent override (`launchctl disable`) and then boots the job out, so the daemon stays off across logins; `enable` clears the override and bootstraps the installed plist only when launchd does not already have the job. `service status` reports `installed but not running` for a disabled service, and `install` clears a stale override before bootstrapping.
- [x] Run `cargo test -p herdbot`; expect all tests pass.
- [x] Document service commands and distinctions in README.

### Task 2: Settings lifecycle controls

**Files:** Modify HerdbotKit service/setup CLI protocols and `SetupState.swift`; modify `SetupView.swift` and Settings view; add tests to `SetupTests.swift` and process CLI tests.

**Interfaces:** Add typed state for setup rerun, daemon enable/disable, uninstall, and purge confirmation; reuse existing setup report and install status models.

- [x] Test action states, command arguments, refresh after success, surfaced errors, and purge path confirmation.
- [x] Run Rust and Swift test commands: `cargo test -p herdbot` and `cd ui/macos && ./scripts/test.sh`.
- [x] Connect “Run setup again” to existing SetupView; add separate Disable/Enable and Uninstall actions with confirmation explaining retained bot data. The section lives at the top of the Global scope in `ui/macos/Sources/HerdbotMenu/LifecycleSection.swift`; `SetupView.swift` is reused unchanged, since “Run setup again” opens its window. A disabled daemon is not reported as setup trouble, so it neither warns in the menu nor reopens the setup window.
- [x] Repeat both test commands; expect all tests pass.
- [ ] Manually verify setup rerun, disable/re-enable, uninstall confirmation, and no false success on failure. Deferred by Shane; steps are in `ui/macos/README.md` under “Manual check: setup and daemon lifecycle”.
