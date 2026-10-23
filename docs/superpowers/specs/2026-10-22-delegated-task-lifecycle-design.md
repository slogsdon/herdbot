# Delegated Task Lifecycle Design

## Intent

Make delegated work legible while it runs and easier to review when it finishes. A parent should see an immediate acknowledgement, occasional useful progress, a clear blocked or stopped state, and a concise final summary with the complete report available on demand.

## Goals

- Surface task lifecycle events in the chat window without presenting bot-authored updates as user messages. Store them with the owning run and expose them from `herdbot history`; do not inject progress into the parent agent's prompt queue.
- Let a subagent report progress explicitly; coalesce or rate-limit updates in deterministic code.
- Ask subagents for a short completion summary and retain the full report as expandable detail.
- Support optional completion criteria and wall-clock budgets per run; a missing budget creates no new deadline.
- Preserve existing explicit user approval and stop controls; report blocked work with its reason and pane.

## Non-goals

- Periodic polling for progress, model-generated summaries of every intermediate response, automatic cancellation by a default timeout, or a common turn-count budget across harnesses.
- New external action execution or approval semantics.
- Changing harness-native permission modes.

## Approaches

1. **Keep raw tool results hidden and add UI polling.** Small protocol change, but polling cannot recover meaningful progress without adding unnecessary reads and still leaves provenance ambiguous.
2. **Add typed lifecycle events at the existing task/report seam (selected).** Subagents emit deliberate progress and final summary/full report; Herdbot stores and delivers those events for chat. This keeps the existing CLI/MCP boundary and avoids model calls for status formatting.
3. **Stream every pane output line.** More immediate, but noisy, costly to persist, and risks showing partial/internal reasoning as product progress.

## Requirements

- `task` returns an acknowledgement containing run id, description, and resolved route; the app also displays it as a system/status entry.
- A new progress report is optional. Herdbot coalesces bursts and does not generate periodic updates if the subagent has no new report.
- A completion event carries a concise summary and the full result. The full result remains accessible and unchanged for review; the parent agent still receives the report through the existing completion queue.
- Blocked, failed, stopped, expired, and completed outcomes have distinct lifecycle states.
- System notifications use generic copy and never include task prompts, report text, approval-card titles, or action payloads; detailed content is shown only after opening the app.
- Optional `done_when` and `wall_clock_limit` are recorded in the run. When a limit is explicitly supplied and exceeded, the run is stopped and marked expired; without it, current runtime behavior remains.
- A progress or completion update is delivered at most once to the conversation even if the daemon replays the underlying event.
- Tests cover empty/long summaries, duplicate progress/completion delivery, blocked and expired outcomes, and legacy run records without the new optional fields.
- A notification test proves sensitive card/task detail is absent from the lock-screen preview.

## Constraints and safety

- Progress is an explicit status report, not a stream of private reasoning or raw terminal output. Progress appears in chat only and does not wake the parent agent or consume another model turn.
- User-authored prompts, bot messages, and lifecycle notices retain separate source labels.
- Approval decisions remain with the harness and explicit user interaction. No status update implies approval.
- Existing file-backed run records remain readable; new fields are optional and backward-compatible. Store ordered lifecycle events in the run record with stable event IDs; `history` merges them with conversation turns by timestamp.
- Delivery is at-least-once across the filesystem/herdr boundary. Stable IDs make duplicate status entries collapse in history; the design does not claim exactly-once prompt execution where herdr has no idempotency key.
- Progress is recorded at most once per run per 60 seconds; repeated reports inside the window replace the pending progress detail with the latest update.
- An explicit wall-clock budget counts active work only; time blocked on a harness approval pauses the budget so the approval remains available.
- When completion uses hook/tail fallback without a structured summary, show a deterministic first-line preview capped at 120 characters and retain the full text for expansion.
- Additive history fields and new `kind` values retain protocol v1; only a breaking schema change bumps the version.

## Builder calls

- Use typed `RunEvent` fields (`id`, `at`, `kind`, `summary`, optional `detail`) rather than parsing message prefixes.
- Keep progress reports out of OptChat memory; bot-to-bot message bodies remain separately typed and are not user instructions (see the provenance design).

## Resolved decision

Progress updates are chat-only. Completion still reaches the parent through the existing queue.
