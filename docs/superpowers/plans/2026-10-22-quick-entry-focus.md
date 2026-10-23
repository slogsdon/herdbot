# Quick-entry focus lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep ⌥Space focused on the quick-entry panel and dismiss it on sustained focus loss without breaking dictation.

**Architecture:** Retain the key-capable panel and isolate a cancellable focus-loss policy in `QuickEntryController`; keep AppKit observers/timers in the controller.

**Tech Stack:** Swift 6, AppKit, SwiftUI, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-10-22-quick-entry-focus-design.md`

## Global Constraints

- Preserve MacWhisper dictation and current Escape/send/toggle behavior.
- Do not add dictation-app bundle-id lists or dependencies.
- Test with `cd ui/menubar && ./scripts/test.sh`; manually exercise macOS focus behavior.
- Implement in a dedicated worktree from `feature/roadmap`; do not commit, push, or merge. Report branch, diff, checks, and manual results.

## Review Focus

- Brief focus loss followed by regain must not close the panel; test cancellation.
- Sustained app deactivation and another window becoming key must close it.
- Showing quick entry must not raise chat; verify manually in the app.
- Dictation HUD focus must retain destination text; verify with MacWhisper.

---

### Task 1: Focus-loss policy

**Files:**
- Modify: `ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift`
- Test: create a pure focus-policy test in `ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift` only if the policy can live in HerdbotKit; otherwise keep AppKit callbacks small and cover via manual checklist.

**Interfaces:** Controller owns observers and a cancellable delayed dismissal; no new public app API.

- [ ] Record current panel ordering and focus notification sequence on macOS with chat open and closed before choosing AppKit notifications.
- [x] Add deterministic tests for grace-period cancellation and sustained focus loss if extracted policy is introduced.
- [x] Adjust activation/window ordering so the panel can become key without leaving the chat window raised; AppKit behavior remains unverified until the manual check.
- [x] Observe app deactivation and panel key loss; schedule dismissal after 300 ms and cancel when the panel regains focus.
- [x] Remove observers and cancel pending dismissal when hiding or tearing down the controller.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.
- [ ] Manually check MacWhisper with chat open/closed, ⌘Tab, click-away, another app window, and Mission Control; verify dictated text arrives.
