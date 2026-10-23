# Settings app Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Settings destination to the chat app backed only by `herdbot config` CLI operations.

**Architecture:** HerdbotKit decodes typed CLI metadata and manages load/save state; SwiftUI presents global and per-bot settings without parsing TOML.

**Tech Stack:** Swift 6, SwiftUI, AppKit, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-10-22-settings-design.md`

## Global Constraints

- Depends on `2026-10-22-settings-config.md` and its published version-1 JSON contract. Herdr session controls are deferred until the named-session spike and session-setting plan produce their contract.
- No direct TOML parsing or filesystem edits in Swift.
- Per-bot settings included; routing table excluded; herdr session defaults to `default` and named choices depend on the session spike.
- Test with `cd ui/menubar && ./scripts/test.sh`.
- Implement in a dedicated worktree from `feature/roadmap`; do not commit, push, or merge. Report branch, diff, checks, and manual results.

## Review Focus

- Older/missing CLI fields fail with an update message rather than silently changing settings.
- Load/save errors retain the user's current form values.
- Per-bot selection cannot save settings to a different bot than the selected slug.
- Each form explains when the value takes effect.

---

### Task 1: Settings models and CLI seam

**Files:** Modify `ui/menubar/Sources/HerdbotKit/Stream.swift`, `ProcessCLI.swift`; tests `DecodingTests.swift`, `ProcessCLITests.swift`, `Fixtures.swift`.

**Interfaces:** `SettingsCLI` exposes `listSettings(bot: String?) async throws -> [SettingDescriptor]`, `getSetting(key: String, bot: String?) async throws -> SettingDescriptor`, `setSetting(key: String, value: String, bot: String?) async throws -> SettingDescriptor`, and `unsetSetting(key: String, bot: String?) async throws -> SettingDescriptor`. `SettingDescriptor` decodes the version-1 fields in `2026-10-22-settings-config.md`.

- [x] Add fixture-based decode tests for global/per-bot metadata, values, and apply timing; add process argument tests for list/get/set/unset.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; confirm failures.
- [x] Implement typed settings models and ProcessCLI calls; preserve actionable stderr errors.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.

### Task 2: Settings navigation and forms

**Files:** Create `ui/menubar/Sources/HerdbotKit/SettingsState.swift`; modify `ui/menubar/Sources/HerdbotMenu/ChatView.swift`, `SidebarView.swift`, `MenuContent.swift`, `App.swift`; tests in `ui/menubar/Tests/HerdbotKitTests/`.

**Interfaces:** Settings state loads global and selected-bot settings, edits values, and reports per-key save status; navigation accepts an optional bot slug.

**Map control:** Show existing `paths` entries and support adding/replacing one `FROM=TO` entry through `config set paths`; provide a reset-all action through `config unset paths`. Per-entry deletion is deferred because the current CLI contract does not expose it.

- [x] Test load, select bot, save, validation error, load error, and preserving unsaved values after failure.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; confirm failures.
- [x] Add sidebar Settings destination and menu “Settings…” deep link; render controls by metadata type and show description/default/apply timing.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.
- [ ] Manually verify global and per-bot forms and navigation from both entry points.
