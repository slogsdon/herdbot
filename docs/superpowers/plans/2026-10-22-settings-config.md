# Settings configuration CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide typed, metadata-backed CLI read/write operations for supported global and per-bot settings.

**Architecture:** Rust owns the setting registry, validation, JSON contract, and comment-preserving TOML edits; Swift consumes the CLI contract.

**Tech Stack:** Rust edition 2024, existing TOML stack plus `toml_edit` only if needed, serde JSON, cargo tests.

**Spec:** `docs/superpowers/specs/2026-10-22-settings-design.md`

## Global Constraints

- Support global `herdbot.toml` and per-bot `bot.toml`; exclude `routing.toml`.
- Preserve comments, ordering, and unrelated TOML values; failed validation leaves the file unchanged.
- Unknown keys and invalid values fail with actionable errors.
- Keep JSON additions versioned/additive and document command usage in README.
- Implement in a dedicated worktree from `feature/roadmap`; do not commit, push, or merge. Report branch, diff, and checks.

## Review Focus

- Comments and formatting survive edits; tests compare preserved source fragments.
- Invalid key/value or bot path cannot mutate files; tests assert byte-for-byte unchanged files.
- Missing optional settings return metadata defaults without writing config.
- Per-bot access cannot escape the configured home; test unknown/traversal bot identifiers.
- Concurrent/failed writes do not leave truncated TOML; test atomic replacement behavior.

---

### Task 1: Registry and read/list commands

**Files:**
- Create: `crates/herdbot/src/config.rs`
- Modify: `crates/herdbot/src/main.rs`
- Test: `config.rs` unit tests
- Modify: `README.md`

**Interfaces:** `herdbot config list [--bot <slug>] [--json]` returns `{"version":1,"scope":"global|bot","settings":[{"key":"approvals.mode","type":"enum","description":"…","default":"ask","value":"ask","choices":["ask","auto"],"apply":"next-bot-start"}]}`. `herdbot config get <key> [--bot <slug>] [--json]` returns the same single-setting fields. `apply` is `immediate`, `next-bot-start`, or `daemon-restart`.

- [x] Inventory every supported global and per-bot key from current parsers and `bot.toml` readers; record enum/range/default/apply timing in tests.
- [x] Add failing CLI/parser tests for list, get, missing/default values, unknown key, and unknown bot.
- [x] Implement `herdbot config list|get` with human-readable and `--json` output.
- [x] Run `cargo test -p herdbot`; expect all tests pass.
- [x] Document supported scopes and output contract in README.

### Task 2: Safe configuration writes

**Files:**
- Modify: configuration module and `crates/herdbot/src/main.rs`
- Test: owning Rust module tests

**Interfaces:** Add `herdbot config set <key> <value> [--bot <slug>]` and `herdbot config unset <key> [--bot <slug>]`; preserve the Task 1 JSON metadata contract and return the updated setting as JSON when `--json` is supplied.

- [x] Add tests for each key's accepted/rejected values and preservation of comments, key order, and unrelated sections.
- [x] Add tests proving invalid writes leave file bytes unchanged and successful writes are atomically replaced.
- [x] Implement using the repository's format-preserving TOML editor; do not deserialize and reserialize whole files.
- [x] Run `cargo test -p herdbot`; expect all tests pass.
- [x] Document set/unset syntax and effective-time values.
