# herdbot — a local Grok Bot on herdr

A desktop app (Rust + [gpui-kit](https://github.com/longbridge/gpui-kit)) that lets you run a team of persistent bots. Grok Bot runs each bot in a cloud SandBox pod. Here each bot gets its own **herdr workspace**, and its subagents run in **herdr panes** that the bot opens itself. Every agent turn runs on a local harness (`claude`, `codex`, `pi`, or `cursor-agent`). A router picks the harness and model for each task.

Status: MVP scope approved. `crates/herdr-client` and its live spike (`cargo run -p herdr-client --example spike -- --agent pi`) verify the herdr lifecycle end to end, including a real pi turn on opencode-go. The orchestrator, MCP bridge, and UI are next.

---

## 2. Scope (needs sign-off)

| Feature | MVP | Later | Never (cloud-only) |
|---|:-:|:-:|:-:|
| Bots: name, role, avatar, default harness | ✅ | | |
| One herdr workspace per bot | ✅ | | |
| Subagents as panes (Task / Check / Message / Stop / WakeParent) | ✅ | | |
| Harness + model routing | ✅ | | |
| Transcript / activity feed per bot | ✅ | | |
| Tasks (bot-level and user-level) | ✅ | | |
| Skills and memory as files in the bot workspace | ✅ | | |
| Automations (cron → prompt the bot) | ✅ | | |
| Approval cards for `blocked` panes | ✅ | | |
| Bot-to-bot DM (`SendToAgent`) | | ✅ | |
| Rooms (multi-bot turn-taking) | | ✅ | |
| Templates / export bot as a bundle | | ✅ | |
| Email inbox, Slack/Teams, meetings, voice, phone | | maybe via MCP | |
| SandBox pods, virtual cards, marketplace, team admin | | | ❌ |

---

## 3. Architecture

```
herdbot daemon ──── NDJSON/UDS ────► herdr server (~/.config/herdr/herdr.sock)
  · delivers queued prompts            │ workspace "🤖 <Bot>" per bot
    when the target is idle            ├─ root pane: main agent "<slug>" (claude|codex|pi|cursor)
  · watches subagent runs              ├─ split pane: subagent "<slug>-<run>"
  · fires schedules                    └─ split pane: subagent "<slug>-<run>"
  · re-imports the Grok Bot roster          │
                                            │ each agent launches
herdbot mcp --bot B [--run R] ◄─────────────┘ (claude, codex, cursor: stdio MCP;
herdbot tool <name> …          ◄──────────────  pi: herdbot-pi.ts extension)
      │ reads/writes
      ▼
$HERDBOT_HOME (~/Bots): bot.toml, BOT.md, .runs/, .queue/, .state/
```
The gpui-kit app comes later. It reads the same files and calls the same herdr socket.

### Crates
| Crate | Responsibility |
|---|---|
| `herdr-client` | Async NDJSON client for `herdr.sock`. Checks `protocol == 22` in `ping`. Subscribes with `events.subscribe`. |
| `herdbot` | One binary: CLI, daemon, stdio MCP server, the `tool` entry point for pi, the router, and the Grok Bot importer. State is plain files under `$HERDBOT_HOME`. There is no orchestrator socket and no database: every process reads and writes the same files, and only the daemon delivers prompts. |
| `app` | gpui-kit UI (not started). Uses `crates/shell` and `examples/` as the shell reference. |

### herdr integration (verified this session)
- Socket `~/.config/herdr/herdr.sock` speaks newline-delimited JSON. `{"id","method","params"}` → `{"id","result":{...}}`. Ping returned `version 0.9.1, protocol 22`.
- Methods used: `workspace.create{cwd,label,env}`, `pane.split`, `agent.start{name,kind,pane,args}`, `agent.prompt{target,text,wait}`, `agent.get`, `agent.read`, `agent.send_keys`, `agent.focus`, `pane.close`, `events.subscribe` (`pane.agent_status_changed`, `pane.exited`).
- **One request per connection.** The server closes the socket after replying, so `Client::call` dials for each request. Only `events.subscribe` stays open, streaming `{"event","data"}` lines.
- **`agent.start` does not wait over the socket.** It returns `launch_pending: true` and the name is not promptable yet. The client polls `agent.get` until `interactive_ready` (the CLI hides this).
- **Status events are per-pane.** `pane.agent_status_changed` needs a `pane_id`. Subscribe per subagent pane after the split. Global events: `pane.created`, `pane.agent_detected`, `pane.exited`, `pane.closed`.
- **A fresh pane rejects `agent.start` with `agent_pane_busy`** until its shell reaches a prompt. The client retries inside the start deadline.
- **`agent.prompt --wait` can report `agent_prompt_stalled`** when a turn finishes (or errors) inside herdr's 5 s activity gate. That is not a delivery failure. The orchestrator uses `report_result` plus status events, not the prompt wait.
- Agent kinds `claude`, `codex`, `pi`, `cursor` exist. `cursor` aliases `cursor-agent` (from `agent-detection/remote/cursor.toml`).
- Constraints:
  - Agent names must match `[a-z][a-z0-9_-]{0,31}` and be unique among live agents.
  - `agent.start` needs a shell sitting at its prompt.
  - `blocked` means the agent is waiting on an approval or a question.

### Bot layout (`$HERDBOT_HOME`, default `~/Bots`)
```
routing.toml          # written from the repo default on first use; edit freely
herdbot.toml          # [paths] "/workspace/obsidian-vault" = "~/…"  (applied to imported BOT.md)
<slug>/bot.toml       # name, title, cwd?, class?, source = grok|local, grok_id?, [[schedules]]
<slug>/BOT.md         # instructions; overwritten by Grok sync when source = "grok"
<slug>/BOT.local.md   # local additions, appended, never overwritten
<slug>/memory/  skills/
.state/<slug>.json    # workspace_id, root_pane, agent
.state/launch/<agent>.md / .mcp.json   # instructions + MCP config handed to the harness
.runs/<id>.json       # one subagent run
.queue/<agent>.jsonl  # prompts waiting for that agent to be idle
```
The agent learns its instructions from a herdbot preamble plus `BOT.md` + `BOT.local.md`:
- claude: `--append-system-prompt-file`
- pi: `--append-system-prompt <file>`
- codex and cursor (no verified flag): sent as the first prompt

**Run from an installed binary.** Every MCP config and pane env records the path of the running `herdbot` binary. If the daemon runs from `target/debug`, a bot that rebuilds herdbot swaps the binary out from under its own subagents. Install a stable copy first (`cargo install --path crates/herdbot`) and run `herdbot daemon` from `~/.cargo/bin`.

`cwd` defaults to the bot folder. A bot can point it at a repo instead: `herdbot new herdbot-dev --cwd ~/Code/herdbot`.

### Delivery: everything goes through the queue
**A prompt sent to a busy agent steers its turn, and the in-flight answer is lost** (verified with pi on 2026-10-20). `agent.prompt` sent mid-turn was injected as a steer, and the first task's reply never appeared. So nothing prompts an agent directly except the agent's very first prompt at launch. Messages, subagent results and schedule firings are appended to `.queue/<agent>.jsonl`. The daemon flushes a queue only when the agent is `idle`/`done`, joins pending items into one prompt, and then waits 15 s before delivering to that agent again. A queued message for a bot that isn't running brings it up. The daemon is the only process that launches bots, so two senders can't race to start the same bot.

**Known limitation:** while a trust or warning modal is up (codex's first run, for example), herdr can report the agent as idle. The daemon then types into the modal and drops the queued item. Accept the modal once in that pane and resend.

### Subagent lifecycle
1. A bot calls `task{description, prompt, subagent_type?, model?}`.
2. The router resolves `(subagent_type, bot override, explicit model)` to `harness:model[:effort]` (§4).
3. `pane.split` right of the bot's root pane, `focus:false`, with env `HERDBOT_BOT/RUN/TOOLS/EXE/HOME`.
4. `agent.start name=<slug>-<run> kind=<harness>`, then one prompt: the task plus a footer asking for `report_result`.
5. `task` returns the run id at once. Tool calls never block on a subagent, because harness tool timeouts are short.
6. Completion:
   - **`report_result`** marks the run done and queues `[subagent <id> "<desc>" finished]
<result>` for the parent. This is Grok's WakeParent.
   - **No report:** if the child is `idle`/`done` more than 20 s after start, the daemon queues its last 60 lines instead.
   - **Exited:** if the child is gone more than 90 s after start, the parent gets a failure notice.
   - **`blocked`:** the parent is told once that the child is waiting on an approval in pane P.
7. Once a run is done, the daemon closes its pane as soon as the child settles. `check_subagent` and `stop_subagent` cover the rest.
8. Approvals stay with each harness's normal behaviour (no `--yolo`). Only herdbot's own tools are pre-approved: claude `--allowedTools mcp__herdbot`, codex `default_tools_approval_mode="approve"`. Whether cursor prompts per tool call under `--approve-mcps` is untested.

### Bot tools
`task`, `check_subagent`, `stop_subagent`, `send_to_agent{bot, message}`, `list_bots`, and `report_result` (runs only).

`send_to_agent` brings the target bot up and queues `[message from <bot>] …`. The reply comes back the same way.

### Harness launch (verified live 2026-10-20 unless marked)
| Harness | Model / effort | Tools | Instructions |
|---|---|---|---|
| claude | `--model`, `--effort` | `--mcp-config <file> --allowedTools mcp__herdbot` ✔ | `--append-system-prompt-file` ✔ |
| codex | `-m`, `-c model_reasoning_effort=` | `-c mcp_servers.herdbot.{command,args,env}=…` and `default_tools_approval_mode="approve"` (herdbot tools only; valid values `auto|prompt|writes|approve`) ✔. Runs embedded (codex warns that `-c` disables its shared server). The first run hit codex's trust prompts for plugin hooks, which ate the prompts. | first prompt |
| cursor-agent | `--model` (effort is in the id) | merges a `herdbot` entry into `<cwd>/.cursor/mcp.json` (identity from pane env), `--approve-mcps`. Not yet run live. | first prompt |
| pi | `--no-extensions -e <herdbot-pi.ts> --provider opencode-go --model`, `--thinking` | herdbot-pi.ts registers the tools and shells out to `herdbot tool` ✔ | `--append-system-prompt <file>` ✔ |

herdr rejects agent args it can't quote safely (`invalid_agent_argument`, triggered by newlines), so prompts and MCP config are passed as files.

**pi guardrail** (verified in the spike):
1. Always pass `--no-extensions --provider opencode-go --model <id>`. Without `--no-extensions`, one of your pi extensions overrode the CLI model **at startup**, before any message, to `opencode-go/qwen3.8-max`. Later it switched to `openai-codex/gpt-6-astra`. Likely: the second switch was a failover after an opencode-go 401. `--no-extensions` also drops MCP (pi's docs claim `-e builtin:mcp` restores it; that wasn't retested, since the extension route is per-agent and needs no project trust).
2. The router rejects any pi model not listed for `opencode-go` by `pi --list-models`.
3. Still to do: audit the pi session `.jsonl` at runtime for any non-opencode-go `provider`.

### Grok Bot sync (one-way)
- **What's on disk.** Grok Bot keeps its roster in `~/Library/Application Support/Grok Bot/sand-client-persistence/<base32 key>.blob`, where the key decodes to `sand.client.slice.account.<acct>.roster.last-roster`.
- **What gets imported.** Each row's `description` is the bot's full system prompt (checked: the long ones end cleanly). `herdbot import-grok` (and the daemon, whenever the file changes) creates or refreshes one bot per row and rewrites paths from `herdbot.toml`.
- **Not on disk.** Schedules (`GrokBotAgentAutomation{automation_id, record_json}`), skills and memory live in Grok Bot's server-side store (`/home/box/sand-data/agents/<id>/store`). The local cache only holds the roster, the last 200 transcript messages per bot, and UI state. They have to come in another way: a template JSON export from the app (`botTemplateJsonShareV1`), or asking each Grok bot to dump its routines. herdbot does not call Grok Bot's private API with the app's tokens.
- **Running both apps at once.** Imported schedules start `enabled = false`, so a routine never fires in both apps. Switch a schedule on in herdbot only when it's switched off in Grok Bot. Writes never go back to Grok Bot.
- **Not yet imported.** Group bots (`isGroup`, rooms) are skipped for now.

### Computer use (cua, not built)
The plan is to route the `computer_use` class to cua across machines on the LAN. From cua.ai docs, summarised by a recon agent (unverified):
- **A Space is a desktop**: a local macOS VM (Lume), or Linux.
- **`cua-driver mcp`** drives the desktop of the machine it runs on, over stdio. To reach a remote host: `ssh -T user@host cua-driver mcp`.
- **`cua-spacesd`** serves Spaces over gRPC/HTTP on :3211 with a bearer token.
- **`cua daemon mcp`** is an MCP server.
- **Lean:** a `cua` route that launches a claude subagent whose MCP config points at `cua-driver mcp` on the chosen host over SSH. `herdr --machine` can also run whole bots on another machine.
- **Status:** nothing cua is installed yet.

---

## 4. Routing

The routing table is in [`routing.toml`](routing.toml). Resolution order:
1. An explicit `model` arg on `Task`. It is checked against the harness allowlist.
2. `[bots.<slug>]` override.
3. `[classes.<subagent_type>]`: an ordered list of `harness:model[:effort]`. The first available harness wins. A harness counts as available when its binary is present and it has not hit a recent quota or auth failure.
4. `[default]`

**Quota and auth cooldown.** When a subagent settles or blocks, and when a main agent finishes a turn, the daemon scans the last 15 non-empty lines of its pane for its harness's failure text:

| Harness | Quota | Auth |
|---|---|---|
| claude | a line starting `You've hit your … limit`; the reset time comes from `resets 12:30am` | `Please run /login`, `authentication_error` |
| codex | a line starting `You've hit your usage limit`; the reset time comes from `try again at 3:45 PM` or `… at Oct 21st, 2026 3:45 PM` | `Your access token could not be refreshed`, `OAuth refresh token was rejected`, `api_key_expired` |
| cursor | not detected (see below) | `Your stored authentication is invalid`, `Error: Authentication required`, ``run `cursor-agent login` to re-authenticate`` |
| pi (opencode-go) | — | `api_key_expired` |

The strings were checked against the installed CLIs on 2026-10-20: claude 2.1.304, codex 0.163.0, cursor-agent 2026.10.16. cursor's quota text comes from its server rather than the CLI, so cursor has no quota detection.

A match writes `.state/cooldown/<harness>.json` (`{until, reason}`). `until` is the parsed reset time plus one minute, or one hour from now when the output gives no time. `route::available` skips the harness until then. A subagent that failed this way is closed, and its task is re-queued with the same class on the next route; its parent is told the new run id. A main agent is restarted in a new pane on the next route, and the queued message it could not answer is delivered again with a note that the conversation is new. Text typed straight into the pane is not re-sent. When no other route is available, the old agent keeps running. To lift a cooldown early, delete the file.

Task classes are the recovered `SubagentType` values (`explore, shell, bash, browser_use, computer_use, debug, media_review, custom`) plus `code, review, plan, research`, which Grok Bot handles through `custom`.

All model IDs in `routing.toml` were listed by the installed CLIs on 2026-10-19. The choices of which model fits which task are **Assumptions** and are meant to be tuned.

---

## 5. Pins
- gpui-kit `0.8.0` uses `gpui-pre-*` Zed snapshots. Its README warns that snapshots break the API, so pin a **git rev** and copy its `Cargo.lock` entries for the gpui crates.
- herdr protocol `22`. The app refuses to start on a mismatch.

## 6. Open questions
1. **Grok schedules, skills, memory:** export template JSON from the app, or ask each bot to dump its routines? (They aren't on disk; see §3 Grok Bot sync.)
2. **cua setup:** cua-driver over SSH, or Lume Spaces?
3. **Approvals:** should any bot run unattended (`--yolo` / `--dangerously-skip-permissions` / codex `--full-auto`)? The default is no.
4. **herdr session:** one herdr session for all bots (`--session herdbot`), or the user's default session? Bots currently land in the default session.
5. **`~/Bots` as `$HERDBOT_HOME`:** Assumption. Change it with the env var.
