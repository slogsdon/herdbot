# Terminal focus and attach spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve terminal scripting and permission behavior before implementing “Jump to pane” terminal activation/attach.

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

- [ ] Inspect installed app scripting dictionaries and record app/version.
- [ ] In a disposable terminal window/tab only, verify app activation and new tab/window with `herdr session attach default` for each supported terminal.
- [ ] Inspect herdr client/session status output and determine whether a herdbot-created local client can be identified after relaunch; document remote-client limits.
- [ ] Verify Apple Events permission and hardened-runtime/notarization requirements using a scratch signed build or authoritative local tool output.
- [ ] Review the report against the design's no-duplicate and fallback goals; if an app cannot meet them, propose a scoped fallback in the report before implementation planning.
- [ ] No product implementation or commit in this spike.
