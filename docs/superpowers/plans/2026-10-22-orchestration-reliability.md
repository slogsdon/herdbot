# Orchestration Reliability Implementation Plan

> **For agentic workers:** Use the repository's herdbot delegation workflow. Execute one task at a time in a dedicated worktree. Each step is independently testable; do not commit or push.

**Goal:** Ensure explicit model requests do not silently change harness, and recover safely from duplicate or interrupted lifecycle delivery.

**Architecture:** Keep the single-daemon, file-backed design. Resolve explicit routes exactly when available; if a valid explicit route is unavailable, use configured fallback and surface the requested route, actual route, and reason. Add stable IDs and source metadata to queue/event records, and recover interrupted event batches. Do not claim exactly-once execution across herdr.

**Tech Stack:** Rust 2024, serde JSONL files, Tokio, herdr client.

**Spec:** `docs/superpowers/specs/2026-10-22-orchestration-reliability-design.md`

## Global Constraints

- Existing file-backed state stays readable; new serialized fields use serde defaults.
- An explicit `harness:model[:effort]` launches that route when available. If unavailable, configured fallback is allowed with a visible acknowledgement/history entry naming requested route, actual route, and reason. Malformed or invalid routes return an actionable error.
- Configured fallback remains enabled when no explicit model is supplied.
- Visible lifecycle history deduplicates stable event IDs; delivery to herdr is at-least-once where the API has no idempotency key.
- Never commit, push, merge, install, or launch the running daemon.

## Review Focus

- Explicit route is valid but unavailable because a harness is absent from `PATH` or the requested model is absent: launch configured fallback and expose requested route, actual route, and reason in acknowledgement and history.
- A model ID contains a colon or effort suffix: parse harness/model/effort without silently changing the model identifier.
- Daemon errors after queue rename: preserve the `.taking` batch for recovery.
- Repeated queue/event ID: do not add duplicate visible items or duplicate lines to the joined prompt.
- A bot tries to inspect or stop a run owned by another bot: reject without reading or changing it.

---

### Task 1: Exact explicit routing and run ownership

**Files:**
- Modify: `crates/herdbot/src/route.rs`
- Modify: `crates/herdbot/src/tools.rs`
- Test: `crates/herdbot/src/route.rs` and `crates/herdbot/src/tools.rs` unit tests

**Interfaces:**
- Consumes: `route::resolve(table, class, bot, explicit) -> Result<Route>` and the MCP `task`, `check_subagent`, and `stop_subagent` handlers.
- Produces: explicit route resolution returns the requested `Route` when available, otherwise resolves configured fallback with a reason; malformed or invalid routes return an error.

- [ ] Add tests for an explicit Pi/DeepSeek request selecting exactly Pi/model/effort, explicit unavailable route selecting configured fallback and recording the reason, non-explicit class routing retaining fallback, and `model` without `subagent_type` not invoking the class picker.
- [ ] Run `cargo test -p herdbot route::tests` and verify the new route cases fail before changing the resolver.
- [ ] Change `route::resolve` so malformed or invalid explicit candidates return an error; when a valid explicit candidate is unavailable, preserve the configured candidate chain and attach requested route, actual route, and fallback reason to the acknowledgement and run evidence.
- [ ] Add a parser test for model IDs containing `:`; parse the final component as effort only when it is a supported effort value, otherwise preserve it as part of the model ID.
- [ ] Add tests proving `check_subagent` and `stop_subagent` reject a run whose `bot` differs from `Ctx.bot`.
- [ ] Run `cargo test -p herdbot` and `cargo clippy -p herdbot -- -D warnings`; expect pass.

### Task 2: Stable queue/event identity and recovery

**Files:**
- Modify: `crates/herdbot-core/src/home.rs`
- Modify: `crates/herdbot-core/src/events.rs`
- Modify: `crates/herdbot/src/daemon.rs`
- Modify: `crates/herdbot/src/hook.rs`
- Test: unit tests in the same modules

**Interfaces:**
- Consumes: Task 1's exact-route contract.
- Produces: queued items have `{id, source, text, at}` with optional defaults for legacy records; hook events have stable IDs when supplied by the harness; duplicate IDs are filtered before delivery batching.

- [ ] Add queue tests for stable serialization, legacy `{text, at}` decoding, and repeated-ID deduplication before prompt joining.
- [ ] Add an event-consumer test that injects an error after rename to `.taking`, restarts consumption, and proves the batch is recovered and processed once in visible history.
- [ ] Add tests for duplicate hook event IDs and repeated completion enqueue; verify one stable lifecycle notice ID is produced.
- [ ] Implement optional queue/event IDs and source fields with serde defaults; use harness turn/event IDs when present and a documented deterministic fallback otherwise.
- [ ] Preserve renamed batches on failure and restore/re-read them on the next tick; delete only after successful processing.
- [ ] Sort snapshot run views by `started_at` (stable ID as the tie-break), not by random run ID.
- [ ] Run `cargo test -p herdbot` and `cargo clippy -p herdbot -- -D warnings`; expect pass.

### Task 3: Dispatch smoke and documentation

**Files:**
- Modify: `README.md`
- Test/check: disposable herdbot home and live Pi harness, if available

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: documented explicit-route behavior and durable regression evidence.

- [ ] Run `cargo build` and `cargo test -p herdbot`; record exact output.
- [ ] With an isolated home, run one explicit Pi/DeepSeek task and inspect its run record for the requested and actual routes; if Pi is unavailable, verify configured fallback is used and the acknowledgement/history explains why.
- [ ] Update README routing and task behavior only where current wording is inaccurate; state at-least-once delivery honestly.
- [ ] Review `git diff --check` and confirm only the plan's paths changed.
