# Orchestration Reliability and Replay Design

## Intent

Make task dispatch and completion predictable under retries and daemon restarts, and catch regressions with deterministic fixtures before live harness checks.

## Goals

- Diagnose why an MCP `task` request specifying Pi/DeepSeek started Codex Luna, then fix the actual dispatch boundary and add a regression test. `route::resolve` currently tries an explicit route first but falls through after rejection; an unavailable or unvalidated explicit route can therefore select another harness. Treat this as the leading cause to verify, not a proven incident reconstruction.
- Persist stable identifiers for lifecycle events so history can deduplicate visible acknowledgements, progress, and results. Do not claim exactly-once delivery across Herdbot and herdr; the latter has no idempotency key.
- Preserve enough run lifecycle evidence to explain what was requested, which route was selected, what state changed, and how the result was obtained.
- Add deterministic replay/invariant tests for prompt dispatch, route selection, duplicate hooks, blocked events, and completion delivery.
- Keep automated checks distinct from live harness, daemon, and macOS verification.

## Non-goals

- A database, generic external event store, or new network service.
- Claiming exactly-once processing across filesystem writes and herdr; implement idempotent effects and recoverable delivery instead.
- Reworking unrelated test parallelization in this design; that remains a separate performance task.

## Approaches

1. **Patch only the observed model-selection symptom (rejected).** Fast but risks leaving a second dispatch path inconsistent and provides no evidence against recurrence.
2. **Trace and test both MCP and CLI dispatch, then add idempotent lifecycle delivery on the existing file store (selected).** Targets the root cause, covers retries, and fits Herdbot's existing architecture.
3. **Move runs and messages to a database-backed event system (rejected).** Offers transactions but is disproportionate and would require migration of established JSONL state.

## Requirements

- Trace class selection, explicit model parsing, route resolution, and launch arguments from MCP request through `run::spawn`.
- The regression test asserts the exact harness/model/effort passed to launch for a Pi/DeepSeek request.
- Lifecycle events have stable IDs stored in the run record; `history` deduplicates visible entries by ID.
- Queue records carry an optional stable message ID and typed source; duplicate IDs are removed before daemon batching, and IDs move through `.queue`, `.taking`, inflight state, and turn logs.
- Persist status/result before queueing the parent-agent notification. If queue delivery fails, a retry must recover from the saved run; if a prompt can be repeated after a crash at the herdr boundary, document the at-least-once behavior and ensure the UI still displays one event.
- Event-file processing must recover a renamed `.taking` batch after an error instead of leaving it permanently unprocessed.
- Hook events carry a stable ID where the harness provides a turn/event ID; duplicate event IDs are ignored. For harness payloads with no stable ID, use a documented deterministic fallback and test its collision/duplicate behavior.
- Preserve existing hook, `report_result`, and tail fallback semantics; source of completion remains visible in run evidence.
- Add replay fixtures for duplicate completion, duplicate hook delivery, blocked notice once-only, route mismatch, and recovery of a `.taking` event batch.
- Verify `check_subagent` and `stop_subagent` reject run IDs owned by another bot.
- Sort user-visible run lists by `started_at` rather than the random run ID.
- If `model` is explicit, launch that exact harness/model/effort when available. If the explicit route is valid but unavailable, use the configured fallback and make both the requested and actual route visible to the user; malformed or invalid routes return a clear error. Configured route fallbacks remain for non-explicit model selection.
- When an explicit model is present without `subagent_type`, do not invoke the model-based class picker first.

## Constraints and safety

- Keep the current file-backed single-daemon-writer architecture unless tests show a correctness gap it cannot address.
- Never infer a route from a display label; test the resolved route and actual launch arguments.
- Reject malformed or invalid explicit model routes. If a valid requested harness/model is unavailable, allow configured fallback but record and surface the requested route, actual route, and fallback reason in the task acknowledgement and run history. Never switch harness silently.
- Do not alter worktree, test, or user data outside the dedicated fixtures.

## Builder calls

- Prefer an append-only outbox/journal only if the existing run record plus queue cannot provide recoverable idempotency; document the minimal chosen state shape.
- Add optional fields with serde defaults so old run records continue to load.
- Prove dispatch with unit tests first, then one live disposable task on the requested harness when available.

## Resolved decision

A valid explicit route that is unavailable in the current process environment may use the configured fallback. The task acknowledgement and run history must identify the requested route, the actual route, and why fallback occurred. Invalid route syntax or unsupported values remain errors.
