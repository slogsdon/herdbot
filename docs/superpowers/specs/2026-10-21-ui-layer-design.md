# UI Layer: Design

Approved 2026-10-21 (phases 0–2). Phase 3 is specified in [2026-10-22-chat-app-design.md](2026-10-22-chat-app-design.md).

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
- Add it as a login item by hand (System Settings → Login Items); registering one in code is deferred. Follows `watch --json` only (its first line is a snapshot and it repeats on every change), reconnecting with a capped backoff.

**Phase 2: quick-entry** (refined 2026-10-22).
- ⌥Space (Carbon `RegisterEventHotKey`, no Accessibility permission) opens a Spotlight-style floating panel (redesigned 2026-10-22): a large full-width message field, then a row with a gear menu (hotkey, Quit) on the left and a borderless "To" bot/room menu on the right, preselected to the last recipient. There are no buttons, 8 pt padding and a hairline border.
- Return submits through `herdbot send` / `herdbot room send`. A success or error overlay covers the whole panel: success closes it after a moment; an error stays until a click or a keystroke, and the text is kept. Esc closes. Nothing is sent without a keypress, so dictated text can be reviewed first.
- The panel uses the system material (`NSGlassEffectView` on macOS 26+, `NSVisualEffectView` before) and semantic colours, so Reduce Transparency, Increase Contrast, Reduce Motion, light/dark and the accent colour follow the system.
- Dictation (MacWhisper: hold Right ⌘; it types or pastes into the focused field): the hotkey is not a bare modifier; the panel activates the app and becomes key; it stays open if a dictation HUD takes focus; the field is a standard text field. Unverified until a person tests it with MacWhisper.
- If `daemon` is `down`, the panel warns that messages won't deliver until it's up.
- `status --json` and `watch --json` gain `rooms` (`slug`, `name`, `mode`, `paused`); `v` stays 1 because the field is additive, and the Swift app reads a missing `rooms` as none.

**Phase 3:** a chat window with a sidebar of bots and rooms and a scrollable chat per conversation. Now SwiftUI inside the menu bar app, over a new `herdbot history` command, not gpui; see [the chat app design](2026-10-22-chat-app-design.md).

## Errors

- **Daemon down:** `status` still works (it reads files). The icon dims; `daemon: "down"` is found by connecting to the lock socket as `take_lock` does.
- **Missing or corrupt files:** skipped and reported in `warnings`; never a panic.
- **No herdr, or focus unsupported:** the notification still posts; the click copies the reason.

## Testing

- Phase 0: unit and golden tests as above; `cargo build` and `cargo test -p herdbot` pass.
- Phases 1–2: the Swift agent runs against a mocked CLI that returns canned JSON. The notification and the hotkey get a manual live pass.

## Risks and assumptions

- **Focus** is `agent.focus` on herdr's socket (verified 2026-10-21, protocol 22).
- **Assumption:** the Swift package lives in `ui/menubar/` and builds with Command Line Tools (`./scripts/build-app.sh`); login-item registration is deferred (add the app in System Settings → Login Items).
- **Assumption:** about 1 s polling for `watch` is enough; the event volume is tiny.
- The README's open question (gpui-kit vs SwiftUI) is answered by this design: SwiftUI for the menu bar and quick-entry, gpui for the phase 3 app. Update the README when phase 0 lands.
