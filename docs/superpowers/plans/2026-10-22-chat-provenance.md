# Chat Provenance and Collaboration Implementation Plan

> **For agentic workers:** Use the repository's herdbot delegation workflow. Execute one task at a time in a dedicated worktree. Each step is independently testable; do not commit or push.

**Goal:** Show who authored each chat item and let a submitted message name bot/room collaborators independently of the open conversation.

**Architecture:** Extend typed source metadata through queue, inflight, turn, history, and memory records. Reuse the lifecycle disclosure row for bot-to-bot and subagent system entries. On submit, fan out to the active conversation and unique `@` targets without changing sidebar selection.

**Tech Stack:** Rust 2024, JSONL, Swift 6, SwiftUI, existing CLI protocol.

**Spec:** `docs/superpowers/specs/2026-10-22-chat-provenance-design.md`

## Global Constraints

- Queue source and message ID survive delivery into inflight and `turns.jsonl`; new prompts are never implicitly classified as `user`.
- Source-less legacy prompts are marked `unknown`; no prefix heuristic is used.
- User-authored text, bot messages, and Herdbot notices remain separate data kinds in chat; non-user prompts and lifecycle notices are excluded from OptChat user-instruction memory.
- A mention sends nothing until submit; submit fans out to the active conversation and unique mentioned bots/rooms.
- History and snapshot protocol versions remain `v:1`; new fields and kinds are additive and unknown values decode to a neutral row.
- Never commit, push, merge, install, or launch the running daemon.

## Review Focus

- Legacy source-less text resembles a generated message prefix: it remains `unknown`, regardless of its text.
- Mention references a missing or ambiguous slug/title: show an error and do not send.
- The active sidebar target differs from mention targets: send once to the active target and each unique mentioned target, leaving sidebar selection unchanged.
- Room and bot are both mentioned: preserve each target type and existing room rules.
- A fan-out target rejects delivery: retain the draft, show per-target results, and retry only failed targets.

---

### Task 1: Typed message provenance end to end

**Files:**
- Modify: `crates/herdbot-core/src/home.rs`
- Modify: `crates/herdbot-core/src/history.rs`
- Modify: `crates/herdbot-core/src/events.rs`
- Modify: `crates/herdbot/src/daemon.rs`
- Modify: `crates/herdbot/src/hook.rs`
- Modify: `crates/herdbot/src/tools.rs`
- Modify: `crates/herdbot/src/memory.rs`
- Test: Rust history, hook, and memory tests

**Interfaces:**
- Consumes: queue IDs/source and `HistoryItem.source` from the reliability plan.
- Produces: typed `source` (`user`, `bot_message`, `subagent`, `herdbot`, `unknown`) and stable IDs on persisted prompt/turn/history/event records; room bot posts keep their existing `speaker` slug.

- [x] Add tests proving `send_to_agent` prompt source remains `bot_message` through queue, inflight, turn logging, history, and memory exclusion.
- [x] Add tests proving source-less legacy prompts decode as `unknown`, including wrapper-shaped text and ordinary text.
- [x] Carry source and stable message ID through queue decode, inflight marker, `MainAgents` delivery state, `turn_to_log`, and `turns.jsonl`.
- [x] Project lifecycle notices from the persisted run-event records into bot history; keep room notices in their existing transcript model.
- [x] Make `history` emit agent-authored messages and notices as system/status items; mark every source-less legacy prompt `unknown`.
- [x] Update OptChat extraction to exclude all non-user prompts, unknown prompts, and lifecycle notices from user-instruction memory.
- [x] Run `cargo test -p herdbot` and `cargo clippy -p herdbot -- -D warnings`; expect pass.

### Task 2: Mention suggestions and selected collaborators

**Files:**
- Modify: `crates/herdbot/src/main.rs`
- Modify: `crates/herdbot-core/src/home.rs` for retry dedupe across queue, inflight, and turns
- Modify: `crates/herdbot-core/src/rooms.rs` and `crates/herdbot/src/rooms.rs` for additive resolved-mention and idempotency metadata
- Modify: `ui/menubar/Sources/HerdbotKit/ProcessCLI.swift`
- Modify: `ui/menubar/Sources/HerdbotKit/ChatState.swift` and `MentionParser.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/ChatView.swift`
- Test: Rust room/collaboration tests and Swift parser, ProcessCLI, and composer tests

**Interfaces:**
- Consumes: typed bot/room roster from existing `list_bots`/`list_rooms` JSON or a new additive v1 CLI response.
- Produces: mention completion for `@bot` and `@room`, and a submit request fanning out to the active target plus mentioned targets, without changing sidebar selection.

- [x] Add pure parser tests for case-insensitive lookup, bot title/slug resolution, room references, duplicate targets, unknown targets, and plain email addresses.
- [x] Add CLI and state tests for one batch request, stable IDs, per-target outcomes, fan-out to the active target plus unique bot/room targets, and unchanged sidebar selection.
- [x] Implement keyboard-focusable mention suggestions and selected-target chips without changing the active sidebar selection.
- [x] Submit using typed user provenance and stable per-target IDs; preserve drafts on partial failure and retry failed targets only. Suppress room text re-parsing and deduplicate room-mode deliveries against directly targeted bots.
- [x] Run `cd ui/menubar && ./scripts/test.sh`, `./scripts/build-app.sh`, `cargo build`, `cargo test -p herdbot`, and `cargo clippy -p herdbot -- -D warnings`; expect pass.
- [ ] Manually verify active and mentioned targets each receive one message, room routing does not duplicate delivery, and status rows render correctly. Deferred by Shane; automated and disposable-home CLI checks are recorded separately.

**EasyUI design tracking:** Adapted EasyUI's **Accessible & Type-Safe** principle with native, keyboard-focusable SwiftUI controls and accessible labels; the implementation remains source-owned in this app. Suggestion buttons, collaborator chips, per-target failure text, and the colocated **Retry failed** action keep state and feedback with the composer. Spring motion was skipped because this interaction is selection and delivery feedback, where animation would not add information. Runtime visual hierarchy, keyboard traversal, VoiceOver, visible focus, and Reduce Motion remain manual checks.

### Task 3: Documentation and protocol review

**Files:**
- Modify: `ui/menubar/README.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: documented mention, provenance, legacy-labeling, and memory behavior.

- [x] Document mention syntax and submit semantics in the app README.
- [x] Document message source fields and legacy compatibility in the CLI protocol section.
- [x] Run `git diff --check`; reviewed the worktree paths and preserved the pre-existing cumulative Task 1 and prerequisite-plan edits.

### Task 4: Collapsed provenance status entries

**Files:**
- Modify: `crates/herdbot-core/src/history.rs`
- Modify: `ui/menubar/Sources/HerdbotKit/Models.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/MessageViews.swift`
- Test: Rust history tests and Swift model/view-presentation tests

**Constraints:** Reuse the lifecycle disclosure row. Typed `bot_message` items may use their documented sender header to form a concise summary; source-less legacy items remain `unknown` and must never be classified by inspecting their text. Keep history protocol v1 and do not alter memory extraction.

**EasyUI design tracking:** Applies **Accessible & Type-Safe** through native SwiftUI `DisclosureGroup` controls with explicit labels. Compact status text is the default state; full message text is disclosed on demand. Loading and save states do not apply to persisted history; unknown legacy authors remain unknown. No custom motion is added because these historical rows do not benefit from animation. Keyboard traversal, VoiceOver, focus visibility, Reduce Motion, and visual hierarchy remain deferred for Shane's live review.

- [x] Add tests for a typed bot message summary/detail pair and an unknown legacy prompt that stays neutral even when its text resembles a sender header.
- [x] Render typed bot messages and unknown legacy prompts as compact centered status rows; show full text only through an accessible disclosure control.
- [x] Keep delegated lifecycle start/progress/completion rows and their summaries/details unchanged.
- [x] Run `cargo test -p herdbot-core` (33 unit tests, 2 integration tests) and `cd ui/menubar && ./scripts/test.sh` (156 tests, 17 suites); both pass.
- [x] Build the app with `./scripts/build-app.sh`; Info.plist validation passes.
- [x] Record that live visual, keyboard, VoiceOver, focus, and Reduce Motion checks remain deferred.
