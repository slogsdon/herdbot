# Chat App (UI phase 3): Design

Approved 2026-10-22. Follows the [UI layer design](2026-10-21-ui-layer-design.md); this replaces its deferred "Phase 3" paragraph. The toolkit changed from gpui to SwiftUI (see Decisions).

## Goals

- A window for talking to herdbot's bots and rooms that feels like a modern chat app (Grok Bot, Muse, OpenAI), not a copy of herdr's pane UI: a scrollable sidebar of bots and rooms, and a scrollable chat for the selected one.
- Read each conversation's history and send into it. See that a bot is working, queued or blocked.
- Follow the OS: system materials (Liquid Glass), light/dark, accent colour, and the Reduce Transparency, Increase Contrast and Reduce Motion settings.

## Non-goals

- Token streaming. herdbot sees only finished turns, so replies appear when a turn ends.
- Approve or deny. Approvals stay in the harness; the chat only shows a blocked banner and a Jump to pane button.
- Editing or deleting messages, attachments, search inside history, room management (creating, pausing), multiple windows.
- Other operating systems. The data seam (below) is OS-neutral, so another shell can be written later; this phase is macOS only.

## Decisions

| Question | Decision | Why |
|---|---|---|
| Scope | Read history and send | A chat client over the existing queue; read-only would not feel like chat. |
| Toolkit | SwiftUI, in the existing `ui/menubar` package | Native Liquid Glass and accessibility settings come free and match the quick-entry panel. gpui could match them only through per-OS shims, and no screen-reader support was found for it (checked 2026-10-22). A port to another OS later gets its own shell over the same CLI. |
| App shape | A window inside the menu bar app | One process already follows `watch`, owns the hotkey and the notifications. |
| In-flight prompt | The daemon records it | `turns.jsonl` gets a line only when a turn ends (prompt and reply together), so between delivery and turn end the message is in no file. |

## Architecture

```
files: <bot>/turns.jsonl, rooms/<slug>/transcript.jsonl, .queue/<agent>.jsonl,
       .state/inflight/<bot>.json (new), runs, events
                         |
                   herdbot-core  ---- history(), snapshot()   (Rust)
                         |
   herdbot CLI:  history --json | status/watch --json | send | room send | focus
                         |
        ui/menubar Swift package  (HerdbotKit models + state, HerdbotMenu views)
```

The Swift app never reads herdbot's files itself; its only seam is the CLI, as in phases 1 and 2. Every new JSON field is additive and `"v": 1` stays. The Swift decoder treats a missing new field as empty, so an older herdbot degrades instead of failing.

## Data seam

### `herdbot history <target> --json [--before N] [--limit L]`

`<target>` is a bot slug or `room:<slug>`. It prints one JSON object:

```json
{"v":1,"target":"alpha","total":42,"items":[
  {"n":40,"at":1792600000,"kind":"user","text":"..."},
  {"n":41,"at":1792600020,"kind":"bot","text":"..."},
  {"n":null,"at":1792600100,"kind":"user","text":"...","state":"inflight"},
  {"n":null,"at":1792600200,"kind":"user","text":"...","state":"queued"}]}
```

- Items are in time order. `--limit` defaults to 50 and `--before N` returns the page ending before item `n` (paging from the end; the app asks for older pages when you scroll up).
- A bot's items come from `turns.jsonl`: each line's `prompt` becomes a `user` item (absent when typed in the pane) and its `reply` a `bot` item. `n` is the 0-based item index across the whole log, so it is stable as new turns append.
- A room's items come from `transcript.jsonl`: `kind` is `"user"` for speaker `you`, `"bot"` for a bot (with `speaker` set), and `"notice"` for `herdbot`.
- Pending items are appended last and have `n: null`: `state:"inflight"` from `.state/inflight/<bot>.json`, then each `state:"queued"` line of the bot's `.queue/<agent>.jsonl`. Rooms have no pending items: a room post is in the transcript at once.
- Malformed lines are skipped and counted in a `warnings` array.
- `ponytail:` the whole file is read each call; read from the tail if turn logs grow past a few MB.

### `status` and `watch`: counts, preview, working

`BotView` and `RoomView` gain `items` (the total, same count as `history`'s `total`), `last_at`, `preview` (up to 120 characters of the last message, one line) and, for bots, `working`. `working` is true while an in-flight marker exists. The app re-fetches only the open conversation when its `items` changes, and computes unread as `items` minus the count last seen, stored in app preferences.

### Daemon: the in-flight marker

When the daemon delivers a queued prompt to a main agent, it writes `.state/inflight/<bot>.json` (`{"prompt","at"}`); when it logs that agent's turn (`append_turn`) it deletes the file. The launch-turn `READY` handling that already exists (`turn_to_log`) decides when a turn is logged and must not delete a marker for a delivery that belongs to a later turn. A stale marker (no turn logged for 1 hour) is ignored by readers, so a crash can't leave a bot "working" forever.

## The window

- **Sidebar.** A filter field, then two sections, Bots and Rooms. A row has a coloured initial avatar (colour derived from the slug), the name, a one-line preview, the time, an unread dot, and a spinner while `working`. Down bots are dimmed. It is a `NavigationSplitView`, so it takes the system sidebar material.
- **Chat.** A scrollable column. Your messages are right-aligned bubbles and the bot's left-aligned; rooms show the speaker's name and colour on each message and `notice` rows as small centred text. Day separators; a "jump to latest" button when scrolled up; it follows new messages only while you are at the bottom. Pending items show as a lighter bubble with "Queued" or a trailing spinner for in-flight.
- **Rendering.** Replies render inline markdown, with fenced code blocks in monospace. Text is selectable. Long histories load by page; older pages load on scroll-up.
- **Header.** Name, state (working, idle, down), chips for running subagents (from the snapshot's runs; clicking one runs `focus`), and a Jump to pane button.
- **Blocked.** A banner above the chat shows the reason from the blocked event and a Jump to pane button.
- **Composer.** Pinned at the bottom, multi-line like the quick-entry field (grows to a few lines). Return sends via `herdbot send` or `herdbot room send` and the message appears at once as a queued bubble; a newline is the system's own for a multi-line field. Never auto-sends, so dictated text can be read first. A failed send keeps the text and shows the error inline.
- **App wiring.** The menu gets "Open herdbot". Clicking a blocked notification opens that bot's chat (it still can focus the pane from there). The Dock icon appears only while the window is open.

## Errors

- **Daemon down:** history still works (files). The composer warns that messages wait until the daemon is up, as the quick-entry panel does.
- **No herdbot, or an older herdbot with no `history`:** the sidebar still works from `watch`; the chat shows "update herdbot".
- **Corrupt or missing logs:** skipped and reported in `warnings`; the chat shows what it can.
- **A bot with no turns yet:** an empty state with the composer.

## Testing

- **Rust:** fixture directories for `history` (bot with and without prompts, room, malformed lines, queued and in-flight items, paging, a stale marker); a golden test for the new `status` fields; a daemon test that delivery writes the marker and a logged turn clears it.
- **Swift:** the models decode the new JSON, and an older snapshot without the new fields; view-model tests for paging, appending, unread counts and pending reconciliation against a fake CLI; no view tests.
- **Manual (needs a person):** Liquid Glass and the accessibility settings on the window; Return sends in the composer; markdown on long agent replies; scroll position and performance on a long history; the notification opening the right chat. A live check on a real bot: send a message, watch it go queued, in-flight, then reply.

## Build order (each is its own plan)

1. **Data seam (Rust):** `history`, the snapshot fields, the in-flight marker.
2. **Window and read-only chat:** the window, sidebar, chat, markdown, paging, the menu item.
3. **Composer, unread and notifications:** sending, pending bubbles, unread counts, notification to chat.

## Risks and open items

- **Markdown:** SwiftUI's inline markdown does not cover blocks. The plan is a small splitter for fenced code blocks and inline markdown elsewhere; lists and headings render as plain text lines until a later pass.
- **Assumption:** pending reconciliation by marker is enough, so the app does not match pending bubbles to turns by text. If two prompts are delivered before a turn ends, only the latest is in flight; the earlier one shows when its turn is logged.
- **Unverified:** how the chat performs with a very long history, and Return-to-send in a large composer, until a person runs it.
