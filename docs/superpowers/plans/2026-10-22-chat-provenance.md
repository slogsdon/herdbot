# Chat Provenance and Collaboration Implementation Plan

> **For agentic workers:** Use the repository's herdbot delegation workflow. Execute one task at a time in a dedicated worktree. Each step is independently testable; do not commit or push.

**Goal:** Show who authored each chat item and let a submitted message name bot/room collaborators independently of the open conversation.

**Architecture:** Extend typed source metadata through queue, inflight, turn, history, and memory records. Reuse the lifecycle disclosure row for bot-to-bot and subagent system entries. Resolve `@` mentions as the sole recipient at the existing CLI seam, independent of sidebar selection.

**Tech Stack:** Rust 2024, JSONL, Swift 6, SwiftUI, existing CLI protocol.

**Spec:** `docs/superpowers/specs/2026-10-22-chat-provenance-design.md`

## Global Constraints

- Queue source and message ID survive delivery into inflight and `turns.jsonl`; new prompts are never implicitly classified as `user`.
- Known legacy wrappers are recognized exactly; other old messages are marked `legacy` rather than asserting a source.
- User-authored text, bot messages, and Herdbot notices remain separate data kinds in chat and memory.
- A mention sends nothing until submit; the mention-selected bot or room is the sole recipient, independent of the active sidebar conversation.
- Never commit, push, merge, install, or launch the running daemon.

## Review Focus

- User text begins with a string that resembles a generated message prefix: only exact Herdbot wrapper syntax is classified as legacy system content.
- Mention references a missing or ambiguous slug/title: show an error and do not send.
- The active sidebar target differs from the mention-selected target: send only to the mention-selected target and leave the active sidebar selection unchanged.
- Room and bot are both mentioned: preserve each target type and existing room rules.
- Mention-selected target rejects delivery: retain the draft and report the delivery failure.

---

### Task 1: Typed message provenance end to end

**Files:**
- Modify: `crates/herdbot-core/src/home.rs`
- Modify: `crates/herdbot-core/src/history.rs`
- Modify: `crates/herdbot/src/daemon.rs`
- Modify: `crates/herdbot/src/hook.rs`
- Modify: `crates/herdbot/src/tools.rs`
- Modify: `crates/herdbot/src/memory.rs`
- Test: Rust history, hook, and memory tests

**Interfaces:**
- Consumes: queue IDs/source and `HistoryItem.source` from the reliability plan.
- Produces: typed `source` (`user`, `bot_message`, `subagent`, `herdbot`, `legacy`) on persisted prompt/turn/history records; room bot posts keep their existing `speaker` slug.

- [ ] Add tests proving `send_to_agent` prompt source remains `bot` through queue, inflight, turn logging, history, and memory extraction.
- [ ] Add tests for exact legacy `[message from …]` and `[subagent …]` wrappers, plus ordinary user text containing similar words.
- [ ] Carry source and stable message ID through queue decode, inflight marker, `MainAgents` delivery state, `turn_to_log`, and `turns.jsonl`.
- [ ] Make `history` emit agent-authored messages as system/agent items; mark unmatched legacy prompts `legacy`.
- [ ] Update OptChat extraction so agent-authored prompts are not emitted as `user`; retain bot-message bodies as a distinct `agent` kind and exclude lifecycle-only status entries.
- [ ] Run `cargo test -p herdbot` and `cargo clippy -p herdbot -- -D warnings`; expect pass.

### Task 2: Mention suggestions and selected collaborators

**Files:**
- Modify: `crates/herdbot/src/main.rs`
- Modify: `crates/herdbot/src/tools.rs` or add a focused collaboration module
- Modify: `ui/menubar/Sources/HerdbotKit/ProcessCLI.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/ChatView.swift`
- Test: CLI and composer tests

**Interfaces:**
- Consumes: typed bot/room roster from existing `list_bots`/`list_rooms` JSON or a new versioned CLI response.
- Produces: mention completion for `@bot` and `@room`, and a submit request addressed only to the mention-selected target, without changing sidebar selection.

- [ ] Add pure parser tests for `@` completion, case-insensitive lookup, bot title/slug resolution, room references, duplicate targets, unknown targets, and plain email addresses.
- [ ] Add CLI tests proving a submitted mention addresses only the selected mention target (bot and room cases), with stable agent-message provenance and no change to active sidebar selection.
- [ ] Implement keyboard-navigable mention suggestions and selected-target chips without changing the active sidebar selection.
- [ ] Submit the message only to the mention-selected bot or room with bot-to-bot provenance; render it using the collapsed system/status entry. If delivery fails, keep the draft and report the target failure.
- [ ] Run `cd ui/menubar && ./scripts/test.sh`, `./scripts/build-app.sh`, and `cargo test -p herdbot`; expect pass.
- [ ] Add live manual checks that mention-selected bot/room receives the message, the active sidebar conversation does not, and the received message is a collapsed system/status entry.

### Task 3: Documentation and protocol review

**Files:**
- Modify: `ui/menubar/README.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: documented mention, provenance, legacy-labeling, and memory behavior.

- [ ] Document mention syntax and submit semantics in the app README.
- [ ] Document message source fields and legacy compatibility in the CLI protocol section.
- [ ] Run `git diff --check` and verify no unrelated files changed.
