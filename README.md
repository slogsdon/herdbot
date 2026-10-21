# herdbot

herdbot runs a team of persistent AI bots on your own machine, using the coding-agent subscriptions you already pay for. Each bot lives in its own [herdr](https://herdr.dev) workspace. When a bot delegates work, the subagent opens in a pane next to it, so you can watch every agent and step in when one needs you. A router decides which harness (`claude`, `codex`, `cursor-agent` or `pi`) and which model takes each task. When one subscription hits its limit, the work moves to the next.

I built it because I had a roster of bots in Grok Bot and wanted the same setup running locally and spread across my subscriptions. herdbot can import that roster and run alongside Grok Bot, but it doesn't need it.

## Status

This is early, and it's macOS only for now (the daemon runs under launchd). What works so far, verified live on 2026-10-20:

- bots with their own workspace, instructions, memory and skills folders
- subagents in panes, with results delivered back to the bot that asked
- bot-to-bot messages, which wait until the receiving bot is idle
- cron schedules
- routing across claude, codex and pi, plus failover when a harness hits a quota or auth limit
- a one-way import of the Grok Bot roster
- herdbot working on its own repo through a `herdbot-dev` bot
- rooms (group bots) with channel, round-robin and moderated turn-taking, verified live on 2026-10-20

Not built yet: importing Grok Bot's schedules and skills (they aren't stored on disk), computer use through [cua](https://cua.ai), and the desktop UI. cursor-agent launches, but its tools haven't been seen working end to end.

## Requirements

- macOS
- [herdr](https://herdr.dev) running. Tested with 0.9.1, socket protocol 22.
- Rust with edition 2024 support
- at least one harness CLI on your `PATH`, signed in: `claude`, `codex`, `cursor-agent` or `pi` (pi is restricted to the `opencode-go` provider)

## Quick start

```bash
cargo install --path crates/herdbot
herdbot service install
herdbot new helper
$EDITOR ~/Bots/helper/BOT.md
herdbot send helper "What can you do?"
```

`service install` runs the daemon as a launchd agent, and the daemon is what delivers messages, so `send` needs it. The bot's workspace opens in herdr the first time a message arrives.

Other commands:

```
herdbot list | runs                    bots and subagent runs
herdbot up <bot> | down <bot>          start or stop a bot's workspace
herdbot room new | send | list | up    group conversations between bots and you
herdbot task <bot> <class> <prompt…>   start a subagent from the shell
herdbot route <class> [--bot BOT]      which harness:model a task class resolves to
herdbot import-grok                    import the Grok Bot roster once
herdbot service status | uninstall
```

Set `HERDBOT_HOME` to keep bots somewhere other than `~/Bots`.

---

## How it works

```
herdbot daemon ──── NDJSON/UDS ────► herdr server (~/.config/herdr/herdr.sock)
  · delivers queued prompts            │ workspace "🤖 <Bot>" per bot
    when the target is idle            ├─ root pane: main agent "<slug>" (claude|codex|pi|cursor)
  · watches subagent runs              ├─ split pane: subagent "<slug>-<run>"
  · fires schedules                    └─ split pane: subagent "<slug>-<run>"
  · re-imports the Grok Bot roster          │
                                            │ each agent launches
herdbot mcp --bot B [--run R] ◄─────────────┘ (stdio MCP, all harnesses)
      │ reads/writes
      ▼
$HERDBOT_HOME (~/Bots): bot.toml, BOT.md, .runs/, .queue/, .state/
```

There are two crates. `herdr-client` is an async client for the herdr socket. `herdbot` is a single binary that serves as the CLI, the daemon, the stdio MCP server the harnesses call, the `tool` entry point for shell debugging, the router, and the Grok Bot importer. There's no orchestrator socket and no database. Every process reads and writes the same plain files, and only the daemon prompts agents.

### Bot layout

```
routing.toml          # written from the repo default on first use; edit freely
herdbot.toml          # [paths] rewrites, e.g. "/workspace/obsidian-vault" = "~/…"
<slug>/bot.toml       # name, title, cwd?, class?, source = grok|local, grok_id?, [[schedules]]
<slug>/BOT.md         # instructions; overwritten by Grok sync when source = "grok"
<slug>/BOT.local.md   # local additions, appended, never overwritten
<slug>/memory/  skills/
<slug>/turns.jsonl    # main-agent prompt/reply pairs, from turn hooks
.state/<slug>.json    # workspace_id, root_pane, agent, route
.state/launch/<agent>.md / .mcp.json / .settings.json   # instructions, MCP config and hook settings handed to the harness
.state/cooldown/<harness>.json         # set when a harness hits a quota or auth limit
.state/events/<agent>.jsonl            # hook events waiting for the daemon (see Turn hooks)
.state/rooms/<slug>.json               # the daemon's routing state for the room
.state/fm/            schemas handed to fm
.state/daemon.log     # daemon output when it runs as the launchd service
.runs/<id>.json       # one subagent run
.queue/<agent>.jsonl  # prompts waiting for that agent to be idle
rooms/<slug>/room.toml                 # a room: members, mode, limits (see rooms.rs)
rooms/<slug>/transcript.jsonl / .md    # the conversation (append-only) and its rendering
```

A bot's instructions are a short herdbot preamble followed by `BOT.md` and `BOT.local.md`. claude gets them through `--append-system-prompt-file` and pi through `--append-system-prompt`. codex and cursor have no flag I could verify, so they get the instructions as their first prompt.

A bot's `cwd` defaults to its own folder. You can point it at a repo instead, for example `herdbot new herdbot-dev --cwd ~/Code/herdbot`.

**Path rewrites.** `[paths]` in `herdbot.toml` maps strings in the instructions to local paths. `~` expands to `$HOME` and the longest match wins. Rewrites happen when a bot launches, so `BOT.md` stays exactly as Grok Bot wrote it and an edit takes effect on the bot's next start. Grok Bot's `/workspace/obsidian-vault` becomes the bot's `cwd` if you don't map it.

**Run from an installed binary.** Every MCP config and pane records the path of the running `herdbot`. If the daemon ran from `target/debug`, a bot that rebuilds herdbot would swap the binary out from under its own subagents. That's why `service install` refuses to run from `target/`. After a `cargo install`, restart the daemon onto the new binary with `launchctl kickstart -k gui/$(id -u)/dev.herdbot.daemon`.

**The service.** `herdbot service install` writes `~/Library/LaunchAgents/dev.herdbot.daemon.plist` and loads it. The daemon starts at login and restarts 10 s after any exit, which includes the times herdr isn't running yet. launchd starts agents with a bare `PATH` that can't find the harness CLIs, so the plist records the installing shell's `PATH`, `HERDBOT_HOME` and `HERDR_SOCKET`.

### Delivery goes through a queue

A prompt sent to a busy agent steers its current turn, and the answer it was working on is lost. I confirmed this with pi on 2026-10-20 (`crates/herdr-client/examples/busy.rs` reproduces it). So nothing prompts an agent directly except its very first prompt at launch. Messages, subagent results and schedule firings go into `.queue/<agent>.jsonl`. The daemon flushes a queue only when the agent is `idle` or `done`, joins the pending items into one prompt, and waits 15 s before delivering to that agent again. A message for a bot that isn't running brings it up. Since the daemon is the only process that launches bots, two senders can't race to start the same one.

Two known limitations:

- While a trust or warning modal is up (codex's first run in a directory, for example), herdr can report the agent as idle. The daemon then types into the modal and the queued item is lost. When `fm` is available, the daemon holds the delivery (up to 8 retries, about 2 min) while fm reads a prompt in the pane, so accepting the modal lets it through. Without fm it is lost as before. I haven't seen this hold fire on a real modal: codex showed no trust prompt in the two fresh directories I tried.
- Reading a claude pane's history once returned text without the latest reply, and it left the pane scrolled up. Claude subagent results no longer depend on that read, since they come from the Stop hook (see [Turn hooks](#turn-hooks)). Quota detection still reads the pane, so claude's quota detection is unverified. Only cursor's has been seen working on a real limit so far.

### Subagent lifecycle

1. A bot calls `task{description, prompt, subagent_type?, model?}`.
2. The router resolves the task class, any bot override and any explicit model to `harness:model[:effort]` (see Routing).
3. herdbot splits a pane to the right of the bot's root pane, with `HERDBOT_BOT/RUN/TOOLS/EXE/HOME` in its environment.
4. It starts the agent as `<slug>-<run>` and sends one prompt, which is the task plus a footer asking for `report_result`. If the agent is still idle 15 s later, the prompt goes out once more. If that fails too, the run fails with "did not start a turn". I've seen the lost prompt once, with codex, and haven't reproduced it, so the resend path is untested. A main agent that fails to start this way has its workspace closed instead of being left running with no recorded route.
5. `task` returns the run id once the agent has taken the task, usually within a few seconds. Tool calls never wait on a subagent, because harness tool timeouts are short.
6. The run finishes one of four ways:
   - **`report_result`** marks the run done and queues `[subagent <id> "<desc>" finished]` with the result for the parent.
   - **No report.** The daemon sends the parent the harness's own last reply, from its turn-end hook (claude `Stop`, codex `notify`, pi `agent_end`). It falls back to the last 60 pane lines only for a harness without a hook, or when a hooked agent has settled for 5 s with no hook event. The pane fallback also waits until the agent has been idle more than 20 s after it started. It only judges a run once the route is recorded, so a slow start isn't mistaken for a finished turn. Each run records its source in `result_source`: `report`, `hook` or `tail`.
   - **Exited.** If the agent is gone more than 90 s after start, the parent gets a failure notice.
   - **`blocked`.** The parent is told once that the subagent is waiting on an approval in pane P. Both sources are live: a harness hook block and herdr's `blocked` status can each raise it, whichever comes first, and the once-only flag dedupes them. Live, only claude's `PermissionRequest` produced a hook block; codex has no hook source for it.
7. The daemon closes a finished run's pane once the agent settles. `check_subagent` and `stop_subagent` cover everything else.
8. Approvals stay with each harness's normal behaviour, with no `--yolo`. Only herdbot's own tools are pre-approved, through `--allowedTools mcp__herdbot` for claude and `default_tools_approval_mode="approve"` for codex. I haven't tested whether cursor asks for each tool call under `--approve-mcps`.

### Bot tools

`task`, `check_subagent`, `stop_subagent`, `send_to_agent{bot, message}` and `list_bots`. Subagent runs also get `report_result`. `send_to_agent` queues `[message from <bot>] …` for the target bot, bringing it up if needed, and replies come back the same way.

Main agents also get `room_post{room, text}` and `list_rooms`: a bot can post to rooms it belongs to, and room messages reach it as `[room <slug> …]`.

### Rooms

Rooms are group conversations between you and any number of bots. Each room has its own folder under `~/Bots/rooms/<slug>/` with a `room.toml`, an append-only `transcript.jsonl`, and a rendered `transcript.md`.

Turn-taking modes:

- `channel` (default): members speak only when named, either with `@<slug>` or by their display name as a whole word. A message that names nobody calls nobody.
- `round_robin`: each message from you starts a round in which every member speaks once, in order.
- `moderated`: everything goes to the moderator; the moderator names who speaks next.

Limits: `max_turns` defaults to 6 bot posts in a row before the room pauses until you post again; `member_cooldown_secs` defaults to 30 seconds before a member can be called again; `turn_timeout_secs` defaults to 300 seconds for round-robin turns.

Members answer from their own main agents using the `room_post` tool. `herdbot room up <room>` opens a workspace that tails `transcript.md`, and a future desktop UI would read the same file.

### Harness launch

Verified live on 2026-10-20 unless the table says otherwise.

| Harness | Model / effort | Tools | Instructions |
|---|---|---|---|
| claude | `--model`, `--effort` | `--mcp-config <file> --allowedTools mcp__herdbot` | `--append-system-prompt-file` |
| codex | `-m`, `-c model_reasoning_effort=` | `-c mcp_servers.herdbot.{command,args,env}=…` with `default_tools_approval_mode="approve"`. On the first run in a directory, codex's trust prompts for plugin hooks ate the prompts. | first prompt |
| cursor-agent | `--model` (effort is part of the id) | merges a `herdbot` entry into `<cwd>/.cursor/mcp.json` (identity comes from the pane env), plus `--approve-mcps` and `--trust`. Without `--trust`, the first run in a new cwd opens a trust modal that swallows the first prompt. The tools haven't been seen working yet because the test account hit its weekly limit. | first prompt |
| pi | `--no-extensions -e builtin:mcp -e builtin:codemode -e <herdbot-pi.ts> --provider opencode-go --model`, `--thinking` | `-e builtin:mcp -e builtin:codemode -e herdbot-pi.ts`, which registers the herdbot stdio MCP server with `pi.registerMcpServer` at code-mode exposure; scripts use `searchTools()` (`--mcp-config` comes from a third-party extension that `--no-extensions` drops) | `--append-system-prompt <file>` |

Approvals: `[approvals] mode = "auto"` in `herdbot.toml`, or `approvals = "auto"` in a `bot.toml`, launches that bot's main agent and subagents with claude's `--permission-mode auto` and codex's `--approve-for-me`. pi doesn't ask for approvals. cursor has no automatic-review mode, so it still asks. `"ask"` is the default and adds no flags.

herdr rejects agent arguments it can't quote safely (newlines trigger `invalid_agent_argument`), so prompts and MCP config are passed as files.

pi needs a guardrail. Without `--no-extensions`, a user extension replaced the model at startup with a different opencode-go model, and later failed over to another provider entirely. So herdbot always passes `--no-extensions --provider opencode-go --model <id>`, enables built-in MCP and code mode and registers its server through its own extension, and rejects any pi model that `pi --list-models` doesn't list for `opencode-go`. It doesn't yet audit pi's session log for calls to other providers. Built-in MCP also reads `~/.pi/agent/mcp.json`. Code mode exposes enabled servers there to bot scripts, including the global `radius` and `rea` servers; a global server set to `"exposure": "direct"` is declared directly.

### Turn hooks

Each harness's own hook reports two things to herdbot: a finished turn with its reply, and a block waiting on you. Live on 2026-10-20, claude, codex and pi subagent runs each reached `done` with `result_source: hook`.

| Harness | Turn end (reply) | Blocked | Installed by |
|---|---|---|---|
| claude | `Stop`, reply from `last_assistant_message` | `PermissionRequest` (seen live). `Notification` for permission or input is wired but not seen live | `--settings .state/launch/<agent>.settings.json` |
| codex | `notify` `agent-turn-complete`, reply from `last-assistant-message` | none. `approval-requested` never reached `notify` in a live capture, so herdr's status is used | `-c notify=[<herdbot>, hook, codex]` |
| pi | `agent_end`, reply from the last assistant text | `ui_prompt_start` is wired but not seen live | `herdbot-pi.ts`, which pipes each event to `herdbot hook pi` |

cursor-agent is not wired: herdbot installs no hook for it, so its results come from the pane fallback.

- `herdbot hook` always exits 0 and prints nothing. Its own errors go to `.state/events/hook-errors.log`. Each payload becomes at most one line in `.state/events/<agent>.jsonl`, which the daemon drains on each tick. Text is capped at 64 KB, keeping the head and tail.
- codex side turns are filtered. Codex sends `agent-turn-complete` for its own title-generation turn too, and herdbot skips those.
- codex: herdbot's hook re-runs your own `notify` from `~/.codex/config.toml` with the same payload appended, detached. Whether that chained notify still receives its payload is not checked live.
- claude's `--settings` hooks stack with your own. Checked live with one user `Stop` hook, which ran alongside herdbot's. Your `~/.claude/settings.json` is never edited.
- Main-agent turns are logged to `<slug>/turns.jsonl` as `{at, prompt, reply}`. `prompt` is the text the daemon last delivered, or `null` if you typed into the pane. Not checked live: no `turns.jsonl` line had been written when this was documented.
- With `approvals = "auto"`, a blocked event is rarely reachable, because the harness seldom asks for approval.

### Routing

The table is in [`routing.toml`](routing.toml). For each task herdbot tries, in order:

1. an explicit `model` argument on `task`
2. the bot's override in `[bots.<slug>]`
3. `[classes.<subagent_type>]`, an ordered list of `harness:model[:effort]`
4. `[default]`

The first available candidate wins. A harness is available when its binary is on `PATH` and it isn't cooling down after a quota or auth failure. Task classes are Grok Bot's subagent types (`explore, shell, bash, browser_use, computer_use, debug, media_review, custom`) plus `code, review, plan, research`. The model IDs were listed by the installed CLIs on 2026-10-19. Which model suits which task is my judgment, so tune it.

Every launched agent gets an explicit effort. A candidate without one takes its class's `effort`, then `[default] effort`, then `medium`, so a harness never falls back to your global setting.

**Cooldown and failover.** When a subagent settles or blocks, and when a main agent finishes a turn, the daemon scans the last 15 non-empty lines of its pane for its harness's failure text.

| Harness | Quota | Auth |
|---|---|---|
| claude | a line starting `You've hit your … limit`, with the reset from `resets 12:30am` | `Please run /login`, `authentication_error` |
| codex | a line starting `You've hit your usage limit`, with the reset from `try again at 3:45 PM` or `… at Oct 21st, 2026 3:45 PM` | `Your access token could not be refreshed`, `OAuth refresh token was rejected`, `api_key_expired` |
| cursor | a line starting `Error:` that contains `usage limit reached`, with the reset from `unifiedSkuWeeklyLimitResetDays: N` | `Your stored authentication is invalid`, `Error: Authentication required`, ``run `cursor-agent login` to re-authenticate`` |
| pi (opencode-go) | none | `api_key_expired` |

These strings were checked against claude 2.1.304, codex 0.163.0 and cursor-agent 2026.10.16. cursor's quota text comes from its server, and the pattern above is from one real capture, so other wordings may slip through.

A match writes `.state/cooldown/<harness>.json`, and routing skips that harness until the reset time plus a minute (or for an hour when no time is given). A subagent that failed this way is closed and its task re-queued on the next route, and its parent gets the new run id. A main agent restarts in a new pane on the next route, and the message it couldn't answer is delivered again with a note that the conversation is new. Text typed straight into the pane isn't re-sent. When no other route is available, the old agent keeps running. Delete the cooldown file to lift it early.

For subagent runs, when none of the patterns match and `fm available` succeeds, the daemon asks Apple's on-device model to read the same 15 lines. It counts a quota or auth verdict only when the line fm cites sits in the last 12 rows of the pane (a narrow subagent pane wraps one message over several rows) and contains a failure word, and that cooldown lasts 10 minutes. Main agents never act on fm, because a misread would restart them and lose the conversation. Daemon log lines start `fm:`.

---

## Grok Bot compatibility

herdbot started as a local take on Grok Bot, so it borrows Grok Bot's concepts (bots, subagent types, WakeParent-style results, bot-to-bot messages) and can import its roster.

**Import is one-way.** Grok Bot caches its roster in `~/Library/Application Support/Grok Bot/sand-client-persistence/`. Each row's `description` is the bot's full system prompt. `herdbot import-grok` creates or refreshes one bot per row, and the daemon re-imports whenever that file changes. `BOT.md` is written verbatim, and nothing is ever written back to Grok Bot.

**Schedules, skills and memory aren't on disk.** Grok Bot keeps them server-side, so they'll have to come from an exported bot template or from asking each bot to list its routines. herdbot doesn't call Grok Bot's private API.

**Running both apps at once** works because imported schedules start disabled, so a routine never fires in both. Turn a schedule on in herdbot only after you've turned it off in Grok Bot. Grok Bot's group bots aren't imported. Make the room with `herdbot room new`.

## Roadmap

- **Computer use through cua.** The plan is to route the `computer_use` class to a claude subagent whose MCP config runs `cua-driver mcp` on the chosen machine over SSH (`ssh -T user@host cua-driver mcp`). cua Spaces (Lume macOS VMs) are the alternative. `herdr --machine` can also run whole bots on another host.
- **Schedule and skill import** from Grok Bot templates.
- **A desktop UI that feels native to the Mac.** Federico Viticci's early look at his "Apple Bots" app sets a good bar: a global quick-entry window for messaging any bot, a menu bar item, and split views with one agent per pane. For herdbot that means a hotkey that queues a message to a bot, and a menu bar item showing which bots are up, which subagents are running, which harnesses are cooling down, and anything `blocked` and waiting on you. Blocked approvals should become notifications you can act on. The UI only reads the same files and calls the same herdr socket, so it can be built in either [gpui-kit](https://github.com/longbridge/gpui-kit) or SwiftUI. gpui-kit 0.8.0 builds on Zed snapshots that break their API, so it would be pinned to a git rev.
- **pi code mode: verified on pi 1.2.0 (2026-10-20).** Three unisolated `deepseek-v4-flash` runs called `list_bots` from scripts and exited 0 in 5.48, 6.25, and 9.80 seconds; `kimi-k2.7-code` also passed in 13.08 seconds. Disabling global `rea` was unnecessary. Code mode is enabled; model-generated script errors were recovered within the gate.
- **A durable harness on Pi Durable.** Earendil's [Pi Durable](https://earendil.com/posts/pi-durable/) (experimental, TypeScript) checkpoints every model call and tool call, so a new process picks up unfinished work after a crash. It also makes submissions exactly-once, lets a conversation fork without copying its transcript, and lets several clients watch and steer one conversation. Those cover herdbot's weakest spots, which are scraping pane text for status and quota errors, and losing an answer when a busy agent gets prompted. A `durable` harness, a small Node host per bot that runs in a herdr pane, would hand herdbot real status and results for pi-driven bots instead of screen reads. The API may still change, so this stays an experiment next to the pane-based harnesses.
- **More uses for Apple's on-device model.** `fm` now backs up quota and auth detection for subagents and holds deliveries behind modals. Picking a task class when a bot names none is not built; bots rarely omit it. Summarizing subagent output was left out on purpose: the parent gets the raw tail, which loses nothing, while on-device summaries of long output would.

Open questions:

- Should the desktop UI be gpui-kit (Rust, shares crates with herdbot) or SwiftUI (every native Apple framework, as in Viticci's app)?

- Should any bot run unattended? Automatic review is available through `approvals = "auto"`; full bypass (`--yolo`, `--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox`) is still never used.
- Should bots get their own herdr session (`--session herdbot`)? They currently land in your default session.

## License

[MIT](LICENSE)
