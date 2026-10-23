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
