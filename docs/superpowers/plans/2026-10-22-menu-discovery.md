# Menu discovery and setup visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the menu's top five bots clickable and hide Setup when the installation is healthy.

**Architecture:** Derive a stable five-bot list from snapshot conversation timestamps and route selections through the existing chat-opening closure. Derive Setup visibility from the existing setup report state.

**Tech Stack:** Swift 6, SwiftUI, AppKit, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-10-22-menu-discovery-design.md`

## Global Constraints

- Top five sort by `lastAt` descending; missing timestamps sort last; ties and missing timestamps sort by display name, then slug.
- Do not add an All bots menu item; all bots remain in chat.
- Use existing CLI/chat-opening paths; no protocol changes.
- Test with `cd ui/menubar && ./scripts/test.sh`.
- Implement in a dedicated worktree from `feature/roadmap`; do not commit, push, or merge. Report branch, diff, checks, and any manual checks.

## Review Focus

- Missing/equal timestamps can produce unstable or surprising rosters; test display-name/slug ordering.
- `setup.problem` with no successful report must keep Setup reachable; test visibility.
- A bot click must select the matching chat recipient; manually check in the built app.

---

### Task 1: Top-five bot selection and click routing

**Files:**
- Modify: `ui/menubar/Sources/HerdbotKit/MenuState.swift` or `Sidebar.swift` (use the existing state owner)
- Modify: `ui/menubar/Sources/HerdbotMenu/MenuContent.swift`
- Test: `ui/menubar/Tests/HerdbotKitTests/StateTests.swift`

**Interfaces:** Produces a deterministic top-five bot collection for the menu; the existing `openChatForBot(slug)` closure handles selection.

- [x] Add tests for 0, 5, and 6+ bots; descending `lastAt`; nil timestamps; ties and missing timestamps sorted by display name then slug.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; confirm new ordering tests fail before implementation.
- [x] Render only the derived top five in the Bots section, with each row a Button calling `openChatForBot(bot.slug)`.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.
- [ ] Manually verify click opens the correct bot and bots outside the top five remain in the chat sidebar.

### Task 2: Setup menu visibility

**Files:**
- Modify: `ui/menubar/Sources/HerdbotKit/SetupState.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/MenuContent.swift`
- Test: `ui/menubar/Tests/HerdbotKitTests/SetupTests.swift`

**Interfaces:** Expose one derived visibility property on `SetupState` if the view condition would otherwise duplicate state logic.

- [x] Test no report/no error, healthy report, attention-needed report, and `problem != nil`; the error and attention cases show Setup.
- [x] Run the focused test target as part of `cd ui/menubar && ./scripts/test.sh`; confirm failure before implementation.
- [x] Hide Setup before initial report and for a healthy report; show “Finish setup…” when checks need attention and “Setup…” on doctor error.
- [x] Run `cd ui/menubar && ./scripts/test.sh`; expect all tests pass.
- [ ] Manually verify healthy launch does not flash Setup and a failing doctor state exposes it.
