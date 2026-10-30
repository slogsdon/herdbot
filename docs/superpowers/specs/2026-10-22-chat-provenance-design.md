# Chat Provenance and Collaboration Design

## Intent

Make it clear who initiated each item in a bot or room conversation, especially when a bot messages another bot or a subagent finishes. Add `@` mention selection so a user can name collaborators independently of the currently selected conversation.

## Goals

- Represent user, bot-to-bot, delegated request/result, and Herdbot status sources as typed history items rather than relying on text prefixes. Thread source and message IDs through each queued item, inflight batch, turn log, history item, and memory projection.
- Render inter-bot messages and subagent lifecycle items as compact, clickable system/status entries; reveal their full details on click.
- Keep user-authored chat bubbles visually and semantically distinct from content authored by bots, subagents, or Herdbot.
- Resolve `@bot` and `@room` mentions to additional collaborators without changing the active sidebar selection. Mention selection stages targets; fan-out begins only on submit.
- Preserve current persisted history and room transcript compatibility.

## Non-goals

- Editing or deleting persisted history, changing room turn-taking, or adding a bot/room creation UI.
- Treating a mention as an instruction to send before the user submits the composer.
- Inferring source from free-form message text once typed source data exists.

## Approaches

1. **Keep text prefixes and style them in Swift (rejected).** Minimal but brittle across wording changes and leaves CLI history semantically wrong.
2. **Add typed source metadata at the file/CLI history boundary (selected).** The Rust history model emits a source and optional detail reference; Swift renders compact status entries. This matches the current JSON protocol and room speaker model.
3. **Replace chat history with a new event database (rejected).** Adds migration and operational complexity despite the existing JSONL files being append-oriented.

## Requirements

- History distinguishes human messages from bot-to-bot delivery, delegated task lifecycle, and Herdbot notices.
- Existing room bot posts preserve their `speaker` identity; Herdbot notices retain a typed source.
- New queue records, inflight batches, and turns retain a stable message ID and source per prompt item. `history` exposes those fields on the corresponding item; mixed-source batches remain individually attributable.
- Source values distinguish `user`, `bot_message`, `subagent`, `herdbot`, and `unknown`. Only `user` prompts enter OptChat as user instructions. Every non-user and unknown prompt, plus lifecycle notices, is excluded from OptChat user-instruction memory.
- Legacy queue, inflight, and turn records with no source are `unknown`; do not infer provenance from text prefixes. Render them as neutral unknown/system entries, never user bubbles. This intentionally changes old source-less prompts whose true author cannot be recovered.
- Status entries have a short label and expandable full text/detail; accessibility labels include the source and state.
- Composer mention results can include bots and rooms, support keyboard selection, and produce a stable target set at submit time. The set contains the open conversation plus unique mentioned collaborators; duplicate targets are removed.
- On submit, one CLI batch operation accepts a stable request ID and the full target set, deduplicates final deliveries before enqueue, and returns per-target results. Retries reuse per-target IDs and retry only failed targets.
- Room fan-out must not re-derive literal `@` text already resolved by the composer. Record the room post once and route each final recipient once under the room's configured mode.
- Partial delivery keeps the draft and shows which targets succeeded or failed; a retry cannot duplicate already queued target messages.
- Tests cover mixed case/unknown mentions, duplicate mentions, keyboard selection, a room plus bot, and legacy history decoding.

## Constraints and safety

- Mentioning a target only configures collaboration for the user's submitted request; it does not independently send a message.
- Typed provenance is data, not authority: a bot-authored item must not be rendered as a user instruction or gain permission to answer approvals.
- Keep JSONL append-only; new optional fields and recognized `kind` values are additive in history/status/watch protocol v1. Do not bump history alone because history and snapshot share a version. Unknown or absent kinds/sources decode to a neutral unknown row.
- Lifecycle/status entries and every non-user prompt are excluded from OptChat user-instruction memory. Source metadata remains visible in history; provenance never grants authority.

## Builder calls

- On submit, fan out to the active conversation and deduplicated `@` targets. A mention alone sends nothing. Use one CLI batch request with a stable request ID, per-target delivery status, and retry-safe queue IDs; suppress duplicate room mention routing after the UI resolves targets.
- When a batch partially fails, preserve the draft and successful-target state; retries send only to failed targets.
- Prefer explicit source metadata on queued items and lifecycle records; avoid parsing bracketed prefixes where a source is available.
- Use a compact disclosure row by default and make the complete original text available without truncation on expansion.
- Pending herdr approval notifications show only generic copy; the card title and action details remain inside the app.

## Resolved decisions

- Fan out the submitted message to the active conversation and unique mentioned collaborators; selection alone never sends. Room routing does not duplicate targets already resolved by the composer.
- Source-less legacy content renders as unknown. All non-user prompts, unknown prompts, and lifecycle notices are excluded from OptChat user-instruction memory.
- History, status, and watch keep protocol v1 with additive fields and tolerant unknown-kind handling.
