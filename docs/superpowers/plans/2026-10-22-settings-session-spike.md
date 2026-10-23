# Herdr named-session support spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Determine whether herdbot can safely offer an optional named herdr session without moving existing bot workspaces.

**Architecture:** Read-only inspection and disposable-session experiments establish session creation, socket resolution, and client attachment behavior. The default session remains in use; this spike does not change production configuration.

**Tech Stack:** herdr CLI and socket protocol, shell fixtures, Rust client inspection.

**Spec:** `docs/superpowers/specs/2026-10-22-settings-design.md`

## Global Constraints

- No product-code changes and no migration of existing bot workspaces.
- Use a disposable session name and clean up only that fixture after recording results.
- Shipped behavior stays on `default` until a separate design approves a named-session setting.

## Review Focus

- Determine whether a server/session starts without an interactive UI attaching.
- Resolve socket paths from supported machine-readable output, not undocumented parsing if avoidable.
- Existing workspaces remain attached to their current session when configuration changes.
- Confirm whether the named session is visible/usable from the normal herdr UI.

---

### Task 1: Session lifecycle and socket-resolution spike

**Files:** Create report `docs/superpowers/research/2026-10-22-herdr-session-spike.md`.

**Interfaces:** Report the supported CLI/API sequence, socket discovery contract, daemon launch behavior, workspace compatibility, and recommendation for a future setting.

- [ ] Inspect `herdr session list|attach|stop|delete` help and available JSON output.
- [ ] Create and attach to a disposable named session; record whether creation starts a server or UI.
- [ ] Verify socket resolution from supported output and confirm `herdr-client` connection feasibility.
- [ ] Observe an existing default-session workspace while the disposable session runs; do not move or modify it.
- [ ] Stop/delete only the disposable session and verify it is gone.
- [ ] Review results against the Settings spec; propose a separate design if a setting remains viable.
- [ ] No product implementation or commit in this spike.
