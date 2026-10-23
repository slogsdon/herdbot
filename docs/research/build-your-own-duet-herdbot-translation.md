# Build Your Own Duet: Herdbot Translation

Source: `build-your-own-duet.pdf`, 15 pages, supplied by Shane on 2026-10-22. The PDF describes itself as an independent engineering synthesis. Its eight-layer architecture, schemas, thresholds, examples, and build path are the author's proposals; they are not evidence of Claude Duet's internal implementation. Numerical values in the PDF are illustrative and are not adopted as Herdbot defaults.

## Scope

Herdbot is a local orchestrator for persistent coding agents. It does not currently watch a user's email, calendar, payment, support, or error-tracking accounts, and it does not execute general external actions on their behalf. The translation therefore focuses on delegation, conversation provenance, durable delivery, approval boundaries, and regression evidence. It does not add a generic event watcher or Duet-style severity/notification system.

## Findings mapped to Herdbot

| PDF finding | Herdbot today | Translation |
|---|---|---|
| One durable context with rules distinguished from learned facts (pp. 3, 14) | Per-bot instructions, memory files, turn logs, room transcripts, and chat history already exist. | Preserve the separate sources; make the origin of user, bot, room, and delegated content explicit in chat history. Do not turn inferred facts into instructions. |
| Watch incrementally, deduplicate, record ownership and evidence (pp. 3, 8, 9) | Queue items and run records are file-backed; run completion can arrive through hook, `report_result`, or tail fallback. | Use stable lifecycle event identity and retry-safe delivery. External-source cursors, event ownership, and evidence URLs are out of scope until Herdbot owns source integrations. |
| Cheap deterministic rules around model judgment; batch calls (pp. 4, 11) | Routing, queues, run state, and approval handling are code; model choice is delegated to the router. | Keep lifecycle transitions and policy deterministic. Do not add model calls to routine status updates. Model-based triage is not applicable. |
| Speak only when useful; provide a concise brief and make silence explainable (pp. 5, 9) | The UI consumes chat history and status snapshots; delegated task tool output is not yet a first-class user-facing status stream. | Show a concise acknowledgement, bounded progress updates, and a short completion summary, with full details available on demand. Keep operational notices distinct from user-authored messages. |
| Give work a finish line, budget, and explicit blocked behavior (pp. 6, 13) | `task` accepts a description and prompt; runs record class, route, start time, status, and result; users can check or stop runs. | Add optional task completion criteria and wall-clock budget metadata, clear blocked/expired outcomes, and no arbitrary default timeout. Turn-count budgets are deferred because harnesses do not share a common turn counter. |
| Approval packets identify one exact action and expire; blocked actions stay blocked (pp. 6–7) | Harnesses own their approvals; Herdbot surfaces supported approval cards and blocked states. | Preserve one-click/one-prompt and explicit-user-action invariants. Do not create a generic approval system for external actions Herdbot cannot execute or inspect. |
| Lock-screen previews are an outbound privacy surface (p. 10) | The approval notifier currently uses `card.title` as the notification body, which may expose details before the user opens Herdbot. | Use generic notification copy; keep prompt/action detail inside the app. Add a regression check that preview content never includes the card title or payload. |
| External content is data, credentials stay outside the model, outbound content is checked (pp. 7, 14) | Herdbot launches local harness CLIs, which own their credentials and tool policies; task and inter-bot messages can contain arbitrary text. | Preserve source labels and test that message headers/content do not silently acquire elevated authority. Do not claim that Herdbot itself brokers or isolates harness credentials. |
| Learn from skips/edits, but retain a hard severity floor (pp. 7, 12) | There is no autonomous alert classifier or severity threshold to tune. | Keep explicit settings and human instructions authoritative; do not infer policy or automatically quiet categories from one outcome. Feedback learning is not applicable without a triage feature. |
| Replay history and run an invariant battery before exposure (pp. 11–13) | Rust and Swift unit suites cover many pure behaviors; live harness and macOS checks remain separate. | Add deterministic lifecycle/retry fixtures and regression cases for the observed dispatch mismatch. Keep live checks marked separately from automated tests. |
| Separate attention and work loops (pp. 2–3, 8) | Herdbot routes requested work and manages run completion; it has no unsolicited attention loop. | Keep requested delegation and unsolicited monitoring separate. The latter remains out of scope. |

## Proposed subprojects

1. **Delegated task lifecycle and progress** — user-visible acknowledgement, bounded progress, concise completion summary with expandable full report, optional `done_when` and wall-clock budget, and explicit blocked/expired reporting. This incorporates Prepare/Speak while respecting the existing harness approval boundary.
2. **Conversation provenance and collaboration UX** — typed history sources for bot-to-bot and subagent notices, collapsed clickable system entries, and `@` target selection independent of the selected bot or room. This incorporates the paper's source/evidence distinction and the existing usability backlog.
3. **Retry-safe orchestration and replay** — diagnose the observed wrong-harness task launch, add deterministic routing/lifecycle regression coverage, and make run-result notifications idempotent across daemon retries using the existing file-backed architecture. No external event store or database.

## Dispatch finding

`route::resolve` tries an explicit route first, but if that candidate is rejected it continues through configured routes. This makes silent fallback a plausible explanation for an explicit Pi request starting another harness; it does not establish the cause of the reported run. The reliability plan requires exact-route regression coverage and a clear failure when an explicit route cannot be used.

## Existing work deliberately not duplicated

Approval cards and setup state already implement explicit user gates. Existing run JSON already records status and result source. Existing room transcripts already distinguish user, bot, and Herdbot notice speakers. The proposed work extends these seams instead of adding parallel models. UI-directory renaming and test-suite parallelization remain separate backlog items because they are not findings from this PDF.

## Design decisions to confirm

- Use the existing chat window as the surface for delegation acknowledgements, progress, and results; keep the full report accessible from its status entry.
- Progress should be event-driven and coalesced/rate-limited by code, rather than periodic polling or additional model calls.
- Wall-clock budgets are opt-in per task; omission does not introduce a new timeout. Shared turn-count budgets are out of scope.
- Lifecycle notices and report bodies retain provenance and are rendered as system content, not user messages.
- Keep event/run persistence file-backed and compatible with the existing CLI history protocol; avoid a database migration.

## Non-goals

- External app connectors, event polling, anomaly scoring, urgency tiers, daily alert budgets, or morning briefs.
- Autonomous policy learning, broad content classifiers, or a new credential broker.
- Automatically approving, sending, merging, deleting, paying, or signing anything.
- Copying the PDF's illustrative thresholds, task budgets, or severity labels as defaults.
