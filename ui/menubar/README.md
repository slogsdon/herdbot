# herdbot menu bar

A SwiftUI menu bar app for herdbot. It follows `herdbot watch --json`, shows bots, running
subagents, cooling harnesses and blocked agents, and posts a notification when an agent becomes
blocked; clicking it runs `herdbot focus <agent>`. ⌥Space opens a quick-entry panel that queues a message to a bot or room. A chat window (menu: Open herdbot) lists your bots and rooms and shows each conversation. Design: `docs/superpowers/specs/2026-10-21-ui-layer-design.md`.

Needs macOS 15+, Swift 6 (Command Line Tools is enough), and a herdbot that has `watch --json`,
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

⌥Space (or the menu's "Quick entry") opens a Spotlight-style panel: a large message field, and beneath
it a gear menu on the left and a "To" menu (bots, then rooms) on the right. Return sends it with
`herdbot send <bot> <text>` or `herdbot room send <room> <text>`; a "Queued to ..." overlay covers the
panel and it closes. A failure shows its error over the panel; click it or start typing to edit
(your text is kept). Esc closes the panel. The last recipient is remembered. If the daemon is down a
note says messages wait until it is up. Nothing is ever sent without a keypress, so you can read a
dictated message before it goes. The gear menu shows the hotkey and has Quit.

The panel uses the system material (Liquid Glass on macOS 26 and later, the HUD blur before that), so
Reduce Transparency, Increase Contrast, Reduce Motion, light/dark and your accent colour apply to it.

Dictation (MacWhisper, hold Right ⌘): the hotkey is not a bare modifier, so it can't collide with the
dictation key; the panel activates the app and takes key focus, so dictated text lands in its field; and the
panel stays open if a dictation HUD takes focus for a moment.

### Manual check (needs a person; MacWhisper for steps 7-8)

1. `./scripts/build-app.sh`, and quit any running HerdbotMenu first. Use a herdbot with rooms in
   `status --json` (`cargo install --path crates/herdbot`, restart the daemon).
2. Throwaway state with two bots and a room:
   `export HB=$(mktemp -d); for b in scratch other; do HERDBOT_HOME=$HB herdbot new $b; done; HERDBOT_HOME=$HB herdbot room new crew --members scratch,other`
3. `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`. Press ⌥Space. Expect a
   translucent rounded panel near the top of the screen with a hairline border, the cursor in a large
   field, a gear at the lower left, a "daemon not running" note, and `To ⌃ scratch` (borderless) at the
   lower right. If the menu says "⌥Space is taken by another app", free the key first.
4. Type `hello "quoted" world`, press Return. Expect a "Queued to scratch" overlay over the whole panel,
   then the panel closes. `cat $HB/.queue/scratch.jsonl` shows the text.
5. Reopen several times (send, then Esc, then send again): the cursor is in the field every time. Open the
   "To" menu, pick `Crew (room)`, send `to the room`; `tail -1 $HB/rooms/crew/transcript.jsonl` shows it
   from `you`. Reopen: the picker still shows `Crew (room)`.
6. Make a send fail: `chmod 000 $HB/.queue`, send, and expect an error overlay that stays; click it (or type) and the
   text is still there. `chmod 755 $HB/.queue` afterwards. Open the gear: it lists the hotkey and Quit.
   Type enough text (or paste several lines) to see the field grow up to 6 lines while the panel's top
   edge stays put.
7. MacWhisper: open the panel, hold Right ⌘ and speak. The text appears in the field and nothing sends;
   press Return to send. Repeat with another app in front before pressing ⌥Space.
8. System Settings → Accessibility → Display: turn on Reduce Transparency, then Increase Contrast, then
   Reduce Motion, one at a time, reopening the panel each time. Expect an opaque panel, a stronger border,
   and no fade, respectively. Switch to light mode: the panel stays readable. Switch them back.
9. Quit from the gear menu; `rm -r $HB`.

## Chat window

The menu's "Open herdbot" opens a window: a sidebar of bots and rooms (newest activity first, with a
filter field) and the selected conversation. It reads history through `herdbot history`, so it needs a
herdbot that has it (otherwise the chat says "update herdbot"). Your messages are on the right, the bot's
on the left; rooms name each speaker. A message you sent that the bot has not answered shows lighter,
"queued" until the daemon delivers it and then "working". Scroll up to load older messages; "jump to latest"
appears when you are not at the bottom. The header's "Jump to pane" opens the bot's herdr pane, and a banner
appears when the bot is blocked on you. This window is read-only; sending is the quick-entry panel for now.
The Dock icon shows only while the window is open.

### Manual check (needs a person; install the new herdbot and restart the daemon first)

1. `export HB=$(mktemp -d); for b in alpha beta; do HERDBOT_HOME=$HB herdbot new $b; done; HERDBOT_HOME=$HB herdbot room new crew --members alpha,beta`
   Give alpha a history: append a few JSON lines like `{"at":<unix seconds>,"prompt":"hi","reply":"Hello **there**\n\n```rust\nfn x() {}\n```"}` to `$HB/alpha/turns.jsonl` (use times from today and from two days ago), and a line to `$HB/rooms/crew/transcript.jsonl` such as `{"at":<now>,"speaker":"you","text":"hi crew"}`.
2. `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`. No window appears at launch. Menu → "Open herdbot": the window opens and a Dock icon appears.
3. The sidebar lists Bots and Rooms with avatars, previews and times; type in the filter field and rows narrow. Click alpha: bubbles, bold text, the code block in monospace, "Yesterday"/"Today" separators, scrolled to the bottom.
4. `HERDBOT_HOME=$HB herdbot send alpha "queued one"`: within a second a lighter "queued" bubble appears (the daemon is not delivering for this home). Click crew: speaker names in colour.
5. Add more than 50 items to alpha's `turns.jsonl` and scroll up: older messages load and your place holds. Scroll up, then add a line: "jump to latest" appears, and the view does not jump while you are scrolled up.
6. Close the window: the Dock icon goes away and the menu bar item stays. Reopen from the menu.
7. System Settings → Accessibility → Display: Reduce Transparency and Increase Contrast change the sidebar and window; Reduce Motion removes the jump button's animation. Light mode stays readable.
8. Quit from the menu; `rm -r $HB`.
