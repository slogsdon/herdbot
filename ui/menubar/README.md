# herdbot menu bar

A SwiftUI menu bar app for herdbot. It follows `herdbot watch --json`, shows bots, running
subagents, cooling harnesses and blocked agents, and posts a notification when an agent becomes
blocked; clicking it runs `herdbot focus <agent>`. ⌥Space opens a quick-entry panel that queues a message to a bot or room. Design: `docs/superpowers/specs/2026-10-21-ui-layer-design.md`.

Needs macOS 14+, Swift 6 (Command Line Tools is enough), and a herdbot that has `watch --json`,
`focus` and (for rooms in the picker) `rooms` in `status --json` (install with `cargo install --path crates/herdbot`, then restart the daemon).

```bash
./scripts/test.sh          # unit tests (use this, not bare `swift test`)
./scripts/build-app.sh     # builds build/HerdbotMenu.app, ad-hoc signed
open build/HerdbotMenu.app
```

The app finds herdbot at `$HERDBOT_BIN`, `~/.cargo/bin/herdbot`, `/opt/homebrew/bin/herdbot` or
`/usr/local/bin/herdbot`. To start it at login, add it in System Settings → General → Login Items
(the app does not register itself).

Icon: `○` idle, `●` subagents running, `⚠ N` agents blocked, `⊘` daemon down, `?` herdbot missing.

## Manual check (needs a person: notification permission and a click)

1. `cargo install --path crates/herdbot` and restart the daemon, so `herdbot watch --json` exists.
2. Use a throwaway state dir so nothing real is touched:
   `export HB=$(mktemp -d); mkdir -p $HB/.state/events`
3. Start the app with it: `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`
   Allow notifications when macOS asks. Expect a menu bar icon; its menu shows "daemon not running"
   (nothing listens on the throwaway home's lock socket).
4. Block a real agent so the click has somewhere to go:
   `echo '{"at":1,"agent":"herdbot-dev","kind":"blocked","reason":"approval: demo","harness":"claude"}' >> $HB/.state/events/herdbot-dev.jsonl`
   Expect within ~2 s: the icon shows `⚠ 1`, the menu lists "herdbot-dev: approval: demo", and a
   notification "herdbot-dev is waiting" appears.
5. Click the notification: herdr focuses the herdbot-dev pane.
6. Clear it: append `{"at":2,"agent":"herdbot-dev","kind":"turn_end","harness":"claude"}` to the same file; the icon returns to idle.
7. Quit from the menu; `rm -r $HB`.

## Quick entry

⌥Space (or the menu's "Quick entry") opens a panel: pick a bot or room, type or dictate, press Return
(or ⌘Return). It runs `herdbot send <bot> <text>` or `herdbot room send <room> <text>`, shows "queued"
and closes. Esc cancels. The last recipient is remembered. If the daemon is down the panel warns that
nothing is delivered until it is up. Nothing is ever sent without a keypress, so you can read a
dictated message before it goes.

Dictation (MacWhisper, hold Right ⌘): the hotkey is not a bare modifier, so it can't collide with the
dictation key; the panel activates the app and takes key focus, so dictated text lands in its field; and the
panel stays open if a dictation HUD takes focus for a moment.

### Manual check (needs a person; MacWhisper for steps 6-7)

1. `cargo install --path crates/herdbot`, restart the daemon, and quit any running HerdbotMenu. Then
   `./scripts/build-app.sh`.
2. Throwaway state with two bots and a room:
   `export HB=$(mktemp -d); for b in scratch other; do HERDBOT_HOME=$HB herdbot new $b; done; HERDBOT_HOME=$HB herdbot room new crew --members scratch,other`
3. `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`. Press ⌥Space. Expect a
   floating panel with the text field focused, a picker listing `scratch`, `other` and `Crew (room)`, and a
   "daemon not running" warning. If the menu says "⌥Space is taken by another app", free the key first.
4. Type `hello "quoted" world`, press Return. Expect "queued" and the panel closes. Check
   `cat $HB/.queue/scratch.jsonl` shows the text.
5. Open it again: the picker still shows `scratch`. Choose `Crew (room)`, type `to the room`, press ⌘Return.
   Check `tail -1 $HB/rooms/crew/transcript.jsonl` shows it from `you`. Press ⌥Space, then Esc: it closes and sends nothing.
6. MacWhisper: open the panel, hold Right ⌘ and speak. Expect the text to appear in the field and nothing
   to send. Release; read it; press Return to send. If the text goes elsewhere, the panel is not key: report that.
7. Repeat 6 while another app is frontmost before pressing ⌥Space (the usual case). If the transcript
   lands in the other app, or the panel closes when MacWhisper's HUD appears, report it.
8. Quit from the menu; `rm -r $HB`.
