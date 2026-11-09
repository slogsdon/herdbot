# Terminal focus and attach spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve terminal scripting and permission behavior before implementing “Jump to pane” terminal activation/attach.

**Status (2026-11-09):** static spike salvaged, re-verified, and extended in the salvage worktree; report at `docs/superpowers/research/2026-10-22-terminal-focus-spike.md` includes the follow-up implementation recommendation. Live gates 1–4 in that report remain for Shane before an implementation plan is written.

**Architecture:** Use a disposable, non-product spike to exercise LaunchServices selection and the declared scripting interfaces for Terminal.app, iTerm2, and Ghostty. Record exact commands, duplicate detection feasibility, and signing requirements; do not ship automation in this task.

**Tech Stack:** macOS, herdr session attach, AppleScript / terminal scripting dictionaries.

**Spec:** `docs/superpowers/specs/2026-10-22-terminal-focus-design.md`

## Global Constraints

- No product-code changes in the spike.
- Do not send commands to or close the user's existing terminal sessions.
- Supported apps are Terminal.app, iTerm2, Ghostty; herdr session remains `default`.
- Record results and exact manual steps in the spike report under `docs/superpowers/`.

## Review Focus

- Existing session client detection must not claim remote/mosh clients are discoverable.
- Apple Events denied or unavailable must produce a safe, understandable fallback.
- Verify whether each app can create a tab running `herdr session attach default` and activate that tab.
- Establish whether app signing needs Apple Events entitlements and usage strings.

---

### Task 1: Verify supported terminal commands and permission behavior

**Files:**
- Create report: `docs/superpowers/research/2026-10-22-terminal-focus-spike.md`

**Interfaces:** Report exact per-app commands/API, app activation behavior, permission prompt, duplicate-detection mechanism and limitations, and release-signing requirements.

- [x] Inspect installed app scripting dictionaries and record app/version.
- [ ] In a disposable terminal window/tab only, verify app activation and new tab/window with `herdr session attach default` for each supported terminal.
  Blocked for the agent spike: any Apple Events send would raise a TCC permission prompt, which the spike directive forbids. Expected command syntax per app is recorded in the report (dictionary-verified); this is report gate 1–3, to be run manually by Shane.
- [x] Inspect herdr client/session status output; its JSON exposes no client identity/inventory, so a Herdbot-created client cannot be identified from that output after relaunch. Documented remote-client limits.
- [ ] Verify Apple Events permission and hardened-runtime/notarization requirements using a scratch signed build or authoritative local tool output.
  Done in part: hardened-runtime/notarization and entitlement requirements are verified against live Apple docs, `codesign` output for Ghostty 1.4.0/iTerm2, and `ui/macos/scripts/build-app.sh` + `scripts/release.sh` at HEAD (the `ui/menubar` paths in the original capture were renamed). The permission-prompt behavior itself still needs a signed scratch build: report gate 4.
- [x] Review the report against the design's no-duplicate and fallback goals; proposed a scoped fallback where client identity is uncertain. Live permission behavior remains open.
- [x] No product implementation or commit in this spike.
