# Delegated Task Lifecycle Implementation Plan

> **For agentic workers:** Use the repository's herdbot delegation workflow. Execute one task at a time in a dedicated worktree. Each step is independently testable; do not commit or push.

**Goal:** Show delegated work in chat from acknowledgement through progress to a concise, reviewable result.

**Architecture:** Persist typed run events in existing `.runs/<id>.json` records and expose them through `herdbot history`. Progress is UI-only; final results continue to reach the parent agent through its queue. Render summaries by default and full details on demand.

**Tech Stack:** Rust 2024, serde JSON, Swift 6, SwiftUI, existing NDJSON CLI protocol.

**Spec:** `docs/superpowers/specs/2026-10-22-delegated-task-lifecycle-design.md`

## Global Constraints

- New run fields are optional and old `.runs` JSON remains readable.
- No arbitrary default task timeout; an explicit wall-clock limit pauses while the run is blocked on a harness approval.
- Progress is an explicit chat-only report, coalesced to at most one visible update per run per 60 seconds; it does not wake the parent model.
- A completion summary never replaces the complete result; raw hook/tail output uses a deterministic 120-character first-line preview.
- Notification previews contain generic copy only; prompt, result, approval title, and action details remain inside the app.
- Never commit, push, merge, install, or launch the running daemon.

## Review Focus

- Legacy run JSON has no summary/events/limit fields: decode it with current behavior.
- Repeated progress reports inside one minute: keep one visible entry with the newest update.
- Run is blocked at its budget deadline: pause its active-time clock and preserve the approval card.
- Completion comes from hook/tail instead of `report_result`: retain full details and show a deterministic preview.
- Long/private card title: notification body remains generic.

---

### Task 1: Run event and task budget model

**Files:**
- Modify: `crates/herdbot-core/src/home.rs`
- Modify: `crates/herdbot/src/tools.rs`
- Modify: `crates/herdbot/src/daemon.rs`
- Test: unit tests in the same modules

**Interfaces:**
- Consumes: stable queue IDs from `2026-10-22-orchestration-reliability.md` Task 2.
- Produces: `RunEvent { id, at, kind, summary, detail }`; optional `done_when`, `wall_clock_limit`, `active_seconds`, `summary`, and ordered `events` on `Run`.

- [ ] Add tests for legacy Run deserialization, opt-in budget metadata, blocked-time pause/resume, expiration only after active-time limit, and stable event IDs.
- [ ] Add tool schema tests for optional `done_when` and `wall_clock_limit` on `task`, `summary` on `report_result`, and the new `report_progress` tool.
- [ ] Add the minimal serialized `RunEvent` and optional Run fields with serde defaults.
- [ ] Record `started` and `progress` events in the run; coalesce progress inside 60 seconds by retaining the newest detail.
- [ ] Enforce explicit wall-clock budgets in `watch_runs`; pause elapsed active time while herdr reports `blocked`, and mark an expired run distinctly before closing its pane.
- [ ] Ensure `report_progress` writes only to the run record and does not enqueue a parent prompt; keep `report_result` queue delivery unchanged except for a concise summary plus the full result.
- [ ] Run `cargo test -p herdbot` and `cargo clippy -p herdbot -- -D warnings`; expect pass.

### Task 2: History projection for run events

**Files:**
- Modify: `crates/herdbot-core/src/history.rs`
- Modify: `crates/herdbot/src/main.rs`
- Test: `crates/herdbot-core/src/history.rs` tests

**Interfaces:**
- Consumes: `RunEvent` and Run metadata from Task 1.
- Produces: history items with stable ID, `kind: "notice"`, typed `source`, concise `text`, and optional full `detail`.

- [ ] Add a fixture with two runs, progress events, a completed result, and ordinary turns; assert history merges by timestamp and deduplicates event IDs.
- [ ] Project runs whose `parent_agent` matches the requested bot into its history; do not add these events to `turns.jsonl` or OptChat memory.
- [ ] Use the explicit `report_result` summary; when no summary exists, use the first nonblank line capped at 120 characters while preserving full detail.
- [ ] Preserve history protocol v1 for additive optional fields and new string kind values; add decoding tests for absent fields.
- [ ] Run `cargo test -p herdbot` and inspect `herdbot history <bot> --json` output in a disposable home.

### Task 3: Chat lifecycle entries and safe notifications

**Files:**
- Modify: `ui/menubar/Sources/HerdbotKit/Models.swift`
- Modify: `ui/menubar/Sources/HerdbotKit/Timeline.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/MessageViews.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/Notifier.swift`
- Test: `ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift`, `ChatTests.swift`, and notifier tests

**Interfaces:**
- Consumes: Task 2 history item fields.
- Produces: collapsed status row with source/state/summary and a click disclosure of full detail.

- [ ] Add Swift decode/view-model tests for missing optional fields, completed/blocked/failed/expired states, long details, and ID-stable timeline entries.
- [ ] Render `notice` and run lifecycle entries as compact centered system rows; add an accessible disclosure control for full detail.
- [ ] Change blocked/card notification bodies to generic text; verify neither card title nor event reason appears in notification content.
- [ ] Run `cd ui/menubar && ./scripts/test.sh` and `./scripts/build-app.sh`; expect all tests and the app target to pass.
- [ ] Leave live notification/UI inspection for Shane and document the exact manual cases.
