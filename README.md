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
┌──────────── herdbot (gpui-kit app) ────────────┐
│ Sidebar: bots │ Bot view: transcript, tasks,   │
│               │ subagent panes, approvals      │
└───────┬────────────────────────────────────────┘
        │ in-process
┌───────▼────────── orchestrator ────────────────┐
│ bot registry · router · subagent lifecycle ·   │
│ scheduler · sqlite store                       │
└───┬──────────────────────────┬─────────────────┘
    │ NDJSON over UDS          │ UDS (local RPC)
┌───▼──────────────┐   ┌───────▼──────────────────┐
│ herdr server     │   │ herdbot-mcp (stdio MCP)  │◄── launched by each harness
│ ~/.config/herdr/ │   │ Task, CheckSubagent, …   │
│ herdr.sock       │   └──────────────────────────┘
└───┬──────────────┘
    │ workspace w<N> per bot
    ├─ root pane: bot's main agent (claude|codex|pi|cursor)
    ├─ split pane: subagent "<bot>-explore-1"
    └─ split pane: subagent "<bot>-code-2"
```

### Crates
| Crate | Responsibility |
|---|---|
| `herdr-client` | Async NDJSON client for `herdr.sock`. Types generated from `herdr api schema` (JSON Schema → `typify`). Checks `protocol == 22` in `ping`. Subscribes with `events.subscribe`. |
| `orchestrator` | Bot CRUD, router, subagent state machine, scheduler, store (`rusqlite`) |
| `herdbot-mcp` | Stdio MCP server binary for claude, codex, and cursor (pi uses a `-e` extension instead; see §3). It is registered with each of them, carries no state, and forwards calls to the orchestrator over `~/.herdbot/orch.sock`. |
| `app` | gpui-kit UI. Uses `crates/shell` and `examples/` as the shell reference. |

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

### Bot workspace layout
```
~/Bots/<slug>/
  BOT.md            # role + standing instructions (source of truth)
  CLAUDE.md -> BOT.md
  AGENTS.md -> BOT.md   # codex, cursor-agent, pi
  memory/*.md       # RecallMemory / UpdateMemory = file ops
  skills/<name>/SKILL.md
  tasks.json        # mirrored to sqlite
```
`herdr workspace.create --cwd ~/Bots/<slug> --label <name> --env HERDBOT_BOT_ID=<id>` creates the workspace. Its root pane runs the bot's default harness with `herdbot-mcp` registered.

Likely (from each harness's documented convention, not tested here): pi and cursor-agent pick up `AGENTS.md`. Verify each one during the first spike.

### Subagent lifecycle
1. A bot calls the MCP tool `Task{description, prompt, subagent_type, model?, async}`. This mirrors the recovered `TaskArgs`.
2. The orchestrator routes `(subagent_type, bot.harness_pref, override)` → `(harness, model, effort)`.
3. `pane.split` from the bot's root pane. Direction comes from `pane.layout` (wide → right, tall → down), with `--no-focus`.
4. `agent.start name=<slug>-<type>-<n> kind=<harness> -- <model flags>`
5. `agent.prompt` with the task prompt plus a footer: *"When done, call `report_result` with your summary."*
6. Completion:
   - **sync**: the `Task` MCP call blocks until `report_result` arrives (with a timeout). The summary goes back as the tool result.
   - **async**: `Task` returns `subagent_id` right away. When `report_result` arrives, the orchestrator runs `WakeParent`, which is `agent.prompt` on the parent pane, queued until the parent is `idle`/`done`.
   - Fallback: if the pane reaches `done` without calling `report_result`, the orchestrator reads `agent.read recent-unwrapped` and marks the result `inferred`.
7. `CheckSubagent` → `agent.get`. `MessageSubagent` → `agent.prompt`. `StopSubagent` → `send_keys ctrl+c`, then `pane.close`.
8. `blocked` (from the event stream) → an approval card in the UI with the pane's recent output. Approve or deny sends keys. The default is **not** to pass `--yolo` / `--dangerously-skip-permissions`.

### Harness launch flags (verified from each `--help`)
| Harness | Model | Effort | MCP |
|---|---|---|---|
| claude | `--model <id>` | — | `--mcp-config <json> --strict-mcp-config` |
| codex | `-m <slug>` | `-c model_reasoning_effort=<lvl>` | `-c mcp_servers.herdbot.command=...` (Assumption: config-key syntax is unverified) |
| cursor-agent | `--model <id>` | encoded in the model ID (`-high`, `-xhigh`) | `cursor-agent mcp` / `.cursor/mcp.json` in the workspace |
| pi | `--no-extensions --provider opencode-go --model <id>` | `--thinking <lvl>` | `-e <path>/herdbot.ts` (MCP is unavailable under `--no-extensions`) |

**pi guardrail** (verified in the spike):
1. Always pass `--no-extensions --provider opencode-go --model <id>`. Without `--no-extensions`, one of your pi extensions overrode the CLI model **at startup**, before any message, to `opencode-go/qwen3.8-max`. Later it switched to `openai-codex/gpt-6-astra`. Likely: that second switch was a failover after an opencode-go 401 (the qwen turn's error status wasn't checked). Which installed package does this hasn't been identified. With the flag, the 2026-10-20 run held `opencode-go/deepseek-v4-flash` for the whole session and answered `PONG`.
   **`--no-extensions` also drops MCP.** Asked to list its tools, pi named only `read, bash, edit, write`. Your configured servers (`radius`, `rea`) did not load, and adding `-e builtin:mcp` didn't help. So for pi, the herdbot bridge ships as a **herdbot pi extension** (`-e <path>/herdbot.ts`) that registers the Task-family tools natively and calls the orchestrator socket. claude, codex, and cursor use the stdio MCP server.
2. Reject any pi model not in the allowlist from `pi --list-models` (provider `opencode-go`), refreshed at startup.
3. Audit at runtime: herdr reports the pi session file (`agent.agent_session.value`, a `.jsonl`). Tail it. Any `model_change` or assistant message whose `provider` is not `opencode-go` → stop the subagent and flag the bot. An assistant `stopReason: "error"` surfaces as a failed subagent.

---

## 4. Routing

The routing table is in [`routing.toml`](routing.toml). Resolution order:
1. An explicit `model` arg on `Task`. It is checked against the harness allowlist.
2. `[bots.<slug>]` override.
3. `[classes.<subagent_type>]`: an ordered list of `harness:model[:effort]`. The first available harness wins. A harness counts as available when its binary is present and it has not hit a recent quota or auth failure.
4. `[default]`

Task classes are the recovered `SubagentType` values (`explore, shell, bash, browser_use, computer_use, debug, media_review, custom`) plus `code, review, plan, research`, which Grok Bot handles through `custom`.

All model IDs in `routing.toml` were listed by the installed CLIs on 2026-10-19. The choices of which model fits which task are **Assumptions** and are meant to be tuned.

---

## 5. Pins
- gpui-kit `0.8.0` uses `gpui-pre-*` Zed snapshots. Its README warns that snapshots break the API, so pin a **git rev** and copy its `Cargo.lock` entries for the gpui crates.
- herdr protocol `22`. The app refuses to start on a mismatch.

## 6. Open questions
1. Where should bot workspaces live? (`~/Bots` is an Assumption.)
2. One herdr session for all bots (`--session herdbot`), or the user's default session? A separate session keeps the user's own panes clean.
3. Should approvals ever auto-allow per bot? Grok Bot has `ResolveGrokBotLocalToolPermission` plus "always allow".
