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

## U6. A Settings area for every herdbot setting, with setup re-run, daemon control and a herdr session

- **Where:** a new Settings option in the main app (the chat window), reached from the sidebar and from a "Settings…" item in the menu bar that deep-links straight to it.
- **Wanted:**
  1. All herdbot settings editable in the app. They keep living in the TOML files we already have; the app edits them and does not replace them.
  2. A way to run the setup process again from Settings.
  3. A way to disable or uninstall the daemon from Settings.
  4. A herdr session setting, so herdbot can run in a named herdr session and stop putting its workspaces in the user's default session. (This is also the README's open question "Should bots get their own herdr session (`--session herdbot`)? They currently land in your default session.")
- **What "all settings" covers today (verified in the README and `home.rs`):** `herdbot.toml` has `[paths]` (instruction rewrites), `[cua]` (`host`, `command`), `[approvals]` (`mode = ask|auto`), `[codex]` (`sandbox = "none"`), `[memory]` (`max_compactors`); `routing.toml` is the routing table (task classes to ordered `harness:model:effort` candidates, in `~/Bots`); each bot's `bot.toml` has `class`, `cwd`, `approvals`, memory options and schedules. The app's own preferences (hotkey, dismissed setup version) are separate again. Open question: is "all settings" `herdbot.toml` only, or also the routing table and per-bot settings? The routing table is the largest and most error-prone of them.
- **Seam:** the app should keep using the CLI only. That implies a `herdbot config` command group (get, set, list with a description/type/default for each key, `--json`), so the app does not parse TOML or duplicate the key list. Today every `herdbot.toml` section is read with its own hand-written parser (`parse_cua`, `parse_memory_config`, `parse_approvals_default`, `parse_codex_unsandboxed`), so there is no single list of settings to build a form from.
- **Editing must not destroy the file:** a user's comments and ordering in `herdbot.toml` (the `setup` template is mostly commented-out options) have to survive an edit; the `toml` crate round trip drops them. A format-preserving editor (for example `toml_edit`) is likely needed (Likely: it is the standard answer, not checked here).
- **Applying changes:** some settings are read per launch (path rewrites: "takes effect on the bot's next start") and some by the daemon on a tick. The UI should say which; changes that need a daemon restart should offer one.
- **Daemon control:** "disable" and "uninstall" are different. `herdbot service uninstall` removes the plist; a disable that keeps the plist would be a launchd `disable`/`bootout` (not built). Uninstall already exists as `herdbot uninstall [--purge]` and removes only the service and the CLI link unless `--purge`, which asks for the typed path. The Settings version must keep that confirmation, and must say plainly that bots and memory under `~/Bots` are kept by default.
- **Re-run setup:** the setup window already exists (`SetupView`), so Settings can open it. This also answers U5's open question: with the menu item hidden on a healthy install, Settings is the place to open the checklist again.
- **herdr session (verified 2026-10-22):** herdr has named sessions (`herdr --session <name>`, `herdr session list|attach|stop|delete`). `herdr session list` prints name, status, directory and socket per session; only `default` exists here, at `~/.config/herdr/herdr.sock`. herdbot finds herdr through `HERDR_SOCKET` or that default path (`crates/herdr-client/src/lib.rs`), and `service install` copies `HERDR_SOCKET` into the plist, so a session setting would mainly set which socket herdbot talks to.
- **Unknown (needs a spike):** how to create a named session without attaching an interactive client (`herdr --session X` appears to launch the UI), whether a session's server starts on demand, how to resolve a session name to its socket reliably (parse `herdr session list`, or a `--json` form if it has one), and what happens to existing bots' workspaces when the session changes (they live in the old session; `up` would have to tolerate that or move them). Also whether the user can see the herdbot session in their normal herdr UI.
- **Answered (Shane, 2026-10-22):** per-bot settings are in scope; the routing table is not yet. The `default` herdr session is fine for now, so the session setting ships defaulting to `default` and nobody is moved. (The session spike below still has to happen before the setting can be offered.)
- **Size:** large; this needs a brainstorm and a spec, then probably four plans: a settings registry plus `config get/set` in the CLI, the Settings UI, daemon disable/uninstall and setup re-run, and the herdr session support (after its spike).
- **Status:** open, needs design.

## U7. "Jump to pane" should also bring the terminal forward, opening herdr in a new tab if needed

- **Where:** every "Jump to pane" button (chat banner, approval and generic cards) and the menu entries that focus an agent; all call `herdbot focus <agent>`, which is only `herdr.agent_focus(agent)` over the herdr socket (`crates/herdbot/src/main.rs`, the `"focus"` arm).
- **Today:** herdr selects the pane inside its session, but the terminal window that shows herdr stays in the background (or does not exist), so nothing visibly happens.
- **Wanted:** in addition to telling herdr to focus the pane, (1) bring the user's terminal window to the front, and (2) if no herdr client is showing the session, open one: a new tab in the active terminal window running herdr attached to the right session.
- **Facts (verified 2026-10-22):** herdr runs as a server with clients; `herdr session attach <name>` attaches a client to a session and `herdr session list` shows name, status, directory and socket. On this machine the clients seen in `ps` are children of `mosh-server`, i.e. Shane attaches over mosh from another machine at least some of the time. So "the active terminal window" may be on another computer, and nothing local can raise it.
- **What a design must settle:**
  - *Which terminal?* herdbot does not know. Options: detect the app that hosts a local herdr client by walking its parent processes (fails for remote/mosh clients and for tmux-like indirection), use the frontmost terminal-like app, or a setting ("Terminal app") in the new Settings area (U6). A setting is the most predictable.
  - *Raising the window* is the easy part for any app: activate it by bundle id (for example `NSWorkspace` open/activate). It does not need automation permission.
  - *Opening a tab running a command* is per terminal and needs the terminal's own automation: AppleScript for Terminal.app and iTerm2, remote-control for kitty and WezTerm, and for others unknown (Ghostty and Alacritty were not checked). A first version could support one or two terminals and fall back to just activating the app plus a message.
  - *Permissions:* sending Apple events makes macOS ask for Automation permission, and a hardened-runtime, notarized app needs the Apple Events entitlement and a usage string in `Info.plist`, which the app does not have today (Likely: this is how the hardened runtime works; not tested here). That is a release-signing change.
  - *Is a client already showing the session?* If one is, do not open another tab; just raise the window. A check needs `herdr status client`/the session list (format not yet checked).
- **Answered (Shane, 2026-10-22):** supported terminals to start: Terminal.app, iTerm2 and Ghostty. Shane uses Ghostty locally on every machine and has iTerm2 installed on the MacBook as well. Open a new window only if herdbot has not already opened one for the session (skip the duplicate, as agreed). Asked whether the macOS default terminal can be used.
- **Default terminal (verified 2026-10-22, this Mac):** macOS has no single "default terminal" setting. LaunchServices does have default handlers per content type: `NSWorkspace.urlForApplication(toOpen: .unixExecutable)` and `.shellScript` both return `/Applications/Ghostty.app` here (Shane's choice), while `.command` files and `ssh://` still resolve to Terminal.app. So the handler for `public.unix-executable` is a good proxy for "the terminal I chose", and herdbot can read it without any permission. It should be the default value of a "Terminal app" setting (U6) that the user can override among the three supported apps.
- **AppleScript support (verified 2026-10-22 from the installed apps):** all three declare a scripting dictionary: Terminal 2.15 (`Terminal.sdef`), iTerm2 3.7.6 (`iTerm2.sdef`) and Ghostty 1.3.1 (`Ghostty.sdef`). Ghostty's has windows, tabs and terminals with `name`, `working directory` and `id` properties, commands `new window`, `new tab`, `focus` ("bringing its window to the front"), `activate window` and `select tab`, and a surface configuration with a `command` property, so `new tab` with command `herdr session attach <name>` is expressible. The command syntax for each app was not exercised; only the dictionaries were read.
- **Skipping a duplicate:** with the dictionaries above, "is a tab already showing this session" can be checked by looking at tab or terminal names (herdr's terminal title is unverified) or by remembering the window/tab id herdbot created last time and checking that it still exists. Remote (mosh) clients are invisible to this check.
- **Related:** U6 (the herdr session setting decides which session to attach; the terminal app lives in Settings too).
- **Status:** open; the activate-only half (raise the configured terminal) is small and could ship first.
