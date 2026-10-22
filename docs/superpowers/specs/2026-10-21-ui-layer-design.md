# UI Layer: Design

Approved 2026-10-21 (phases 0–2). Phase 3 gets its own spec.

## Goals

- A glanceable view of what herdbot is doing: bots up, subagents running, harnesses cooling down, agents blocked on you.
- Notifications for `blocked` agents that take you to the pane.
- A global hotkey to queue a message to a bot or room.
- One read layer that every front-end shares, so no UI re-parses herdbot's files.

## Non-goals

- Approve/deny from the UI. Hooks are observe-only and stay that way; approving happens in the harness.
- A second control path. The only writes are `enqueue` (what `herdbot send` already does) and pane focus.
- The full chat/workspace app (phase 3).
- Any daemon API. The daemon's socket remains a single-instance lock.

## Architecture

```
files (~/Bots, .runs, queue, .state/events)     herdr socket
              \                                     /
               +--------- herdbot-core ------------+     new crate
                  snapshot()  tail_events()  enqueue()  focus_pane()
                     |                          |
             herdbot CLI (--json)         gpui app (phase 3, links core)
                     |
             Swift menu bar agent (phases 1–2)
```

- **herdbot-core** is extracted from `home.rs`, `run.rs`, `hook.rs` (`Event` is already `Serialize`) and `cooldown.rs`.
  - `Snapshot`: bots (up/down), subagent runs, per-harness cooldowns with time left, unresolved `blocked` events, `daemon: up|down`, `warnings: []`.
  - Writers: `enqueue(bot, text)` and `focus_pane(agent)`.
- **The Swift agent never links Rust.** Its seam is the CLI.
  - `herdbot status --json`: one snapshot.
  - `herdbot watch --json`: NDJSON tail of `.state/events/<agent>.jsonl` (written by `hook::append_event`) merged with run-state changes. Polls about once a second.
  - `herdbot send` and `herdbot room send` for quick-entry.
  - `herdbot focus <agent>` for notification clicks.
- Every JSON payload carries `"v": 1`. The agent shows "update herdbot" on a version it doesn't know.
- The daemon is unchanged and remains the only process that launches bots and delivers queues.

## Phases

**Phase 0: core crate and CLI contract (no UI).**
- Extract the read layer; add `status --json`, `watch --json`, `focus`.
- Focus spike resolved: `herdr-client` calls `agent.focus` on herdr's socket (verified 2026-10-21, protocol 22).
- Tests: fixture directories (blocked event, cooling harness, dead daemon, corrupt jsonl, malformed state), a golden test of the `status --json` shape, and the existing `fixtures/hooks/*.json` for the blocked path. Live check of `focus` against real herdr.

**Phase 1: Swift menu bar agent.**
- `MenuBarExtra` whose icon shows idle, running, or blocked (with a count).
- The menu lists bots, running subagents and cooling harnesses with time left.
- A `blocked` event posts a user notification; clicking it runs `herdbot focus <agent>`.
- Runs as a login item. Polls `status --json` about every 2 s and tails `watch --json` for blocked events.

**Phase 2: quick-entry.**
- A global hotkey opens a floating panel with a bot/room picker and a text field.
- Submit calls `herdbot send` / `herdbot room send`; the panel shows "queued".
- If `daemon` is `down`, the panel warns that messages won't deliver until it's up.

**Phase 3 (deferred):** gpui app with per-bot and per-room views over `transcript.md`, `turns.jsonl` and run history, linking `herdbot-core`. Separate spec.

## Errors

- **Daemon down:** `status` still works (it reads files). The icon dims; `daemon: "down"` is found by connecting to the lock socket as `take_lock` does.
- **Missing or corrupt files:** skipped and reported in `warnings`; never a panic.
- **No herdr, or focus unsupported:** the notification still posts; the click copies the reason.

## Testing

- Phase 0: unit and golden tests as above; `cargo build` and `cargo test -p herdbot` pass.
- Phases 1–2: the Swift agent runs against a mocked CLI that returns canned JSON. The notification and the hotkey get a manual live pass.

## Risks and assumptions

- **Focus** is `agent.focus` on herdr's socket (verified 2026-10-21, protocol 22).
- **Assumption:** the Swift target lives in `ui/menubar/` in this repo and builds separately from cargo. A login item needs a signed app bundle, so signing is an open item. If it moves to its own repo, the CLI contract still holds.
- **Assumption:** about 1 s polling for `watch` is enough; the event volume is tiny.
- The README's open question (gpui-kit vs SwiftUI) is answered by this design: SwiftUI for the menu bar and quick-entry, gpui for the phase 3 app. Update the README when phase 0 lands.
