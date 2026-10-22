# herdbot menu bar

A SwiftUI menu bar app for herdbot. It follows `herdbot watch --json`, shows bots, running
subagents, cooling harnesses and blocked agents, and posts a notification when an agent becomes
blocked; clicking it runs `herdbot focus <agent>`. Design: `docs/superpowers/specs/2026-10-21-ui-layer-design.md`.

Needs macOS 14+, Swift 6 (Command Line Tools is enough), and a herdbot that has `watch --json`
and `focus` (install with `cargo install --path crates/herdbot`, then restart the daemon).

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
