# Herdr named-session support spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status (2026-11-09):** Historical spike salvaged; installed 0.9.1 discovery contract reverified read-only. Viable with a per-home socket ownership guard. Interactive lifecycle checks remain manual, and historical creation/cleanup is not current proof.

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

- [x] Inspect `herdr session list|attach|stop|delete` help and available JSON output.
- [x] Historical: create disposable session via non-TTY attach; server starts, terminal attach fails. Visible TUI attach remains manual.
- [x] Verify socket resolution from supported output and confirm `herdr-client` connection feasibility.
- [x] Historical: observe default workspace while disposable session runs; not repeated during read-only recheck.
- [x] Historical: stop/delete only the disposable fixture and verify absence; not repeated during recheck.
- [x] Review results against the Settings spec; propose a separate design if a setting remains viable.
- [x] No product implementation or commit in this spike.
