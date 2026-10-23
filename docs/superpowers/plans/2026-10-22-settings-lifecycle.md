# Settings setup and daemon lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

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

- [ ] Add tests for disabled/enabled/install state and confirm disable does not remove the plist or CLI link.
- [ ] Run `cargo test -p herdbot`; confirm failing cases before implementation.
- [ ] Implement launchd disable/bootout and enable/bootstrap using existing service path conventions.
- [ ] Run `cargo test -p herdbot`; expect all tests pass.
- [ ] Document service commands and distinctions in README.

### Task 2: Settings lifecycle controls

**Files:** Modify HerdbotKit service/setup CLI protocols and `SetupState.swift`; modify `SetupView.swift` and Settings view; add tests to `SetupTests.swift` and process CLI tests.

**Interfaces:** Add typed state for setup rerun, daemon enable/disable, uninstall, and purge confirmation; reuse existing setup report and install status models.

- [ ] Test action states, command arguments, refresh after success, surfaced errors, and purge path confirmation.
- [ ] Run Rust and Swift test commands: `cargo test -p herdbot` and `cd ui/menubar && ./scripts/test.sh`.
- [ ] Connect “Run setup again” to existing SetupView; add separate Disable/Enable and Uninstall actions with confirmation explaining retained bot data.
- [ ] Repeat both test commands; expect all tests pass.
- [ ] Manually verify setup rerun, disable/re-enable, uninstall confirmation, and no false success on failure.
