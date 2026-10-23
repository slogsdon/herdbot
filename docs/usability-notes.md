# Usability notes

Collected from Shane's own use, newest last. Each item says where it was hit and what is wanted. Nothing here is planned until it is grouped into a batch and approved; status is `open`, `planned` (with the plan) or `done` (with the commit).

## U1. Menu bar bots list: top 5, clickable

- **Where:** menu bar item, the "Bots" section (`ui/menubar/Sources/HerdbotMenu/MenuContent.swift`, the `Section("Bots")` block).
- **Today:** every bot is listed as plain text (`● Name` / `○ Name`), not clickable.
- **Wanted:** the list is capped at the 5 most recently used bots, and each name is clickable; a click opens the chat window with that bot selected.
- **Notes:** the blocked-agent and card entries already open the chat for a bot through `Notifier.openChat`/`WindowOpener`; the same path can serve this.
- **Assumption (not confirmed with Shane):** "most recently used" means most recent activity in the conversation, which is the `last_at` the snapshot already carries per bot (messages sent or answered), not "most recently opened in the app". Bots with no conversation yet sort last by name.
- **Open question:** is there a way to reach the bots beyond the top 5 from the menu (for example an "All bots…" item that just opens the chat window)? Without it, a sixth bot is only reachable by opening the window.
- **Status:** open.

## U2. Create bots and rooms from the chat window ("+" button), and import from a template URL

- **Where:** the chat window's sidebar (`ui/menubar/Sources/HerdbotMenu/SidebarView.swift`).
- **Today:** a bot is created with `herdbot new <name> [--cwd DIR] [--class CLASS]` and a room with `herdbot room new <name> --members a,b[,c] [--mode …] [--moderator BOT]`; the app has no way to create either, and `BOT.md` is edited by hand.
- **Wanted:** an add (+) button in the chat window that creates a new bot or a new room. Stretch ("double win"): accept a Grok Bot template or marketplace URL, and possibly other providers' template URLs if they offer such things.
- **Notes for the bot form:** name, working directory (default: the bot's folder under the herdbot home), task class (routing), and instructions. Creating one from the app should run the same CLI commands, so the CLI stays the only seam; a new `herdbot new` option for instructions and maybe a `--json` result may be needed. The first conversation with a brand-new bot in a directory claude has not trusted shows the approval card, which already works.
- **Notes for the room form:** name, members (pick from existing bots), mode (channel, round robin, moderated) and a moderator for moderated rooms.
- **Template import, what is known (2026-10-22, from search results, mostly third-party; not verified against the official docs):** Grok Bot lets a user copy a bot's share link; the recipient opens a preview on x.ai and chooses "Add to Grok Bot"; a template carries the bot's instructions, relevant memories, skills and chosen plugins, and leaves out credentials, code and scripts. There is also a browsable marketplace. This matters beyond convenience: the README says Grok Bot's schedules and skills are not on disk, "so they'll have to come from an exported bot template", so a template import could be the way to get skills (and possibly schedules) that `import-grok` cannot reach.
- **Unknown (needs a spike before any design):** whether a share link is fetchable without a logged-in session and in what format (HTML preview, JSON, or an app-only deep link); whether skills and memories are in the payload or only referenced; whether other providers have an equivalent. If a link needs a login, the fallback is the user pasting or dropping an exported template, not scraping.
- **Open question:** should an imported template become a normal `source = "local"` bot (herdbot owns it afterwards), as opposed to `source = "grok"` bots whose `BOT.md` the Grok sync overwrites?
- **Status:** open. Suggested split: (a) the + button with a plain bot form and a room form, small and independent; (b) a research spike on template URLs; (c) the import itself, after the spike.

Sources for the template notes: https://x.ai/bot/guides/templates-for-grok-bot, https://www.aibuilderclub.com/blog/grok-bot-templates, https://dragosroua.com/you-can-now-share-your-grok-bots-and-soon-you-could-sell-them/

## U3. Quick entry (⌥Space) also raises the chat window when it is open

- **Where:** `QuickEntryController.show()` in `ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift`.
- **Today:** with the chat window already open, pressing ⌥Space shows the quick-entry panel and also brings the chat window to the front. With the chat window closed, only the panel appears.
- **Wanted:** ⌥Space shows only the quick-entry panel; the chat window stays where it was.
- **Cause (Likely; basis: the code, not reproduced here):** `show()` calls `NSApp.activate(ignoringOtherApps: true)` so the panel can become key. Activating the app raises all of its visible windows, so an open chat window comes forward with it. That also explains why a closed chat window does not appear.
- **Constraint:** the activation is deliberate. The comment in `show()` says dictation tools type into the active app, so the app must be active for dictated text to land in the panel's field. Making the panel non-activating (`.nonactivatingPanel`) would stop the chat window moving but risks breaking dictation, so it must be tested with MacWhisper before it is chosen.
- **Candidate approaches (none tried):** (1) keep activating, then send the chat window behind (`orderBack`) when it was not the key window before the hotkey, restoring nothing else; (2) a non-activating panel, if dictation still works; (3) activate without raising other windows, if AppKit offers a way. Needs a manual check by Shane: dictation into the panel with the chat window open and closed.
- **Status:** open.

## U4. Quick entry should close when focus moves to anything else

- **Where:** `QuickEntryController` / `QuickEntryPanel` in `ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift`.
- **Today:** the panel stays open when you click or type in another app. Shane noticed it still showing while typing directly into herdr + claude.
- **Wanted:** the panel closes by itself as soon as you interact with anything in macOS that is not the quick-entry panel (another app, another window of this app, the desktop).
- **Cause (verified in the code):** `panel.hidesOnDeactivate = false`, and nothing observes the panel losing key status or the app resigning active (no `didResignKey`/`didResignActive` handler). Only Escape, a send or the hotkey closes it.
- **Constraint:** `hidesOnDeactivate = false` is deliberate. The README's dictation note says "the panel stays open if a dictation HUD takes focus for a moment" (MacWhisper, hold Right ⌘). A plain close-on-blur would close the panel under a dictation HUD and lose the dictated text's destination.
- **Candidate approaches (none tried):** close when the panel resigns key AND the app is no longer active, after a short grace period (a few hundred ms) that a returning focus cancels, so a dictation HUD that takes focus briefly does not close it; or close only when the newly active application is not the dictation tool (needs knowing its bundle id, which is brittle); or close on a global mouse-down outside the panel (needs no focus change, but misses keyboard-only switches such as ⌘Tab). Combining the first with the mouse-down is probably the sturdiest; it needs a manual check with MacWhisper, a ⌘Tab away, a click on another app, and a Mission Control swipe.
- **Interaction:** this and U3 both come from how the panel activates the app; they should be designed together.
- **Status:** open.

## U5. Remove the "Setup…" menu item once setup has succeeded

- **Where:** the last block of `ui/menubar/Sources/HerdbotMenu/MenuContent.swift`: `Button(setup.needsAttention ? "Finish setup…" : "Setup…")`.
- **Today:** the item is always in the menu; it reads "Setup…" when every check is ok and "Finish setup…" when something needs attention.
- **Wanted:** after a successful setup the item is gone. It comes back by itself when something needs attention again (for example the daemon is older than the binary after an upgrade, or a harness disappears).
- **Behaviour to decide:** show the item when `setup.needsAttention` is true, and also when `setup.problem != nil` (herdbot missing or `doctor` failing), because that is when the window explains what is wrong. Hide it when the report exists and every check is ok. Before the first report arrives it stays hidden, so a healthy launch never flashes it.
- **Open question:** with the item gone there is no way to open the checklist on a healthy install to see the checks. Acceptable? If a way is wanted, the usual place is the quick-entry gear menu or an option-click on the menu bar item; I would not add one unless asked.
- **Size:** tiny, one condition in one view, plus a view-model test for the visibility rule if it moves into `SetupState` (for example `showsMenuItem`). Good to batch with U1, which edits the same file.
- **Status:** open.
