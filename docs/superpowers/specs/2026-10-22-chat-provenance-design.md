# Chat Provenance and Collaboration Design

## Intent

Make it clear who initiated each item in a bot or room conversation, especially when a bot messages another bot or a subagent finishes. Add `@` mention selection so a user can name collaborators independently of the currently selected conversation.

## Goals

- Represent user, bot, room, delegated request/result, and Herdbot status sources as typed history items rather than relying on text prefixes. Thread source metadata through queue records, inflight records, and turns so it survives delivery.
- Render inter-bot messages and subagent lifecycle items as compact, clickable system/status entries; reveal their full details on click.
- Keep user-authored chat bubbles visually and semantically distinct from content authored by bots, subagents, or Herdbot.
- Resolve `@bot` and `@room` mentions to explicit recipients without changing the active sidebar selection; the target chosen from mention suggestions is the recipient.
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
- Existing room bot posts and Herdbot notices preserve their current identities.
- New queue records, inflight records, and turns retain source and stable message ID. `history` exposes the source on the corresponding item; the same source metadata is used by OptChat ingestion so an inter-bot prompt is never a `user` message.
- For legacy content, recognize only Herdbot's exact generated first-line wrappers (`[message from …]`, `[subagent …]`); label those as system/agent content. Keep unmatched legacy items marked `legacy` so their origin is not asserted.
- Status entries have a short label and expandable full text/detail; accessibility labels include the source and state.
- Composer mention results can include bots and rooms, support keyboard selection, and produce a stable collaborator list at submit time.
- A submitted message addressed with `@` goes only to the bot or room selected from the mention suggestions; the currently open conversation is ignored as a recipient. Queue it as bot-to-bot/agent-authored content so it appears as the collapsed system/status entry. Do not fan out to the open conversation.
- Tests cover mixed case/unknown mentions, duplicate mentions, keyboard selection, a room plus bot, and legacy history decoding.

## Constraints and safety

- Mentioning a target only configures collaboration for the user's submitted request; it does not independently send a message.
- Typed provenance is data, not authority: a bot-authored item must not be rendered as a user instruction or gain permission to answer approvals.
- Keep JSONL append-only; new optional fields and recognized `kind` values are additive in history protocol v1. Bump the version only for a breaking change.
- Lifecycle/status entries are not imported into OptChat memory. Retain bot-to-bot message bodies as a distinct agent-message kind, never as `user`.

## Builder calls

- On submit, send the message only to the bot or room selected from the `@` mention suggestions. The active sidebar conversation is not a recipient. A mention alone sends nothing. Store the message as agent-authored provenance and render it as a collapsed system/status entry with full text on disclosure.
- Prefer explicit source metadata on queued items and lifecycle records; avoid parsing bracketed prefixes where a source is available.
- Use a compact disclosure row by default and make the complete original text available without truncation on expansion.
- Pending herdr approval notifications show only generic copy; the card title and action details remain inside the app.

## Resolved decisions

- Retain bot-to-bot message bodies in OptChat memory as a distinct agent-message kind; never classify them as `user`. Exclude lifecycle/status-only notices.
- `@` mention selection chooses the sole recipient for the submitted message and does not change the active sidebar selection. Do not also send to the currently open conversation. Render the received message as a collapsed system/status entry with full text on disclosure.
