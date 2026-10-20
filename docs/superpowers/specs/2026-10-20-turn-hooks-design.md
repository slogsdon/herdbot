# Turn Hooks: Design

Approved 2026-10-20. This is the first piece of making herdbot's agent handling harness-agnostic. Every harness reports "turn ended, here is the reply" and "waiting on you" through its own native hook, instead of herdbot reading the pane.

## Where this fits

The work is split into three pieces, built in order, each with its own spec and plan:

1. **Turn hooks** (this spec): real replies and blocked events from every harness.
2. **OptChat memory per bot**: herdbot logs each bot's prompts and replies (from `<slug>/turns.jsonl`, produced here), builds the OptChat summary tree with a cheap model, serves `zoom` and `date` over herdbot's MCP to every harness, and hands the view to every fresh session, including quota-failover restarts. The reference is Shane's vault notes "optchat and pi-durable" and "Pi Dispatch Durability Plan".
3. **Pi Durable backend**: an experimental, optional driver, per the 2026-10-09 plan's rule of not building on pi-durable until it leaves experimental.

All three stay harness-agnostic. A capability goes behind something every harness can provide, even if a weaker harness falls back to the pane.

## Goals

- Subagent results come from the harness's own last reply when `report_result` isn't called, instead of the last 60 pane lines.
- Main-agent replies are captured for the first time, paired with the prompt the daemon delivered.
- "Waiting on you" comes from the harness itself, which also catches modals herdr reports as idle.
- Your own harness config is never edited or replaced.

## Non-goals

- Quota and auth detection. These keep reading the pane tail (`cooldown::detect`). claude's `StopFailure` hook is a candidate for a later change.
- Replacing herdr's agent status for the "idle, so deliver" decision.
- Tool-call events. OptChat may want them later; no harness-wide hook covers them yet.

## Verified facts (2026-10-20, from installed binaries, not live runs)

| Harness | Turn-end hook and reply | Waiting on you | Per-agent config path |
|---|---|---|---|
| claude 2.1.304 | `Stop`; the payload has `last_assistant_message`, `transcript_path`, `stop_hook_active` | `PermissionRequest`, `Notification` | `--settings <file-or-json>` |
| codex 0.163.0 | `notify` program, last argv is JSON with `"type": "agent-turn-complete"`, `last-assistant-message`, `thread-id`, `turn-id` | `approval-requested` (notification type) | `-c notify=[…]` |
| pi 1.2.0 | extension event `agent_end { messages }` | `ui_prompt_start { kind, title }` | `herdbot-pi.ts` (after the pi-MCP plan) |
| cursor-agent 2026.10.19 | Likely: `hooks.json` with `afterAgentResponse` / `stop` (strings in its bundle); file location and payload unverified | unknown | Likely `<cwd>/.cursor/hooks.json` |

Shane's own config, which must keep working:
- `~/.codex/config.toml` sets `notify = ["…/SkyComputerUseClient", "turn-ended"]`.
- `~/.claude/settings.json` defines hooks including `Stop` and `PermissionRequest`.

## Design

### `herdbot hook <harness>`

A new CLI entry point. It runs as a harness hook:

1. It reads the payload: stdin for claude, cursor and pi; the last argv element for codex.
2. It normalizes the payload to zero or one event, with `parse_hook(harness, payload) -> Option<Event>`, a pure function.
3. It appends the event as one line to `$HERDBOT_HOME/.state/events/<agent>.jsonl`.
4. For codex only: it runs your original `notify` command from `~/.codex/config.toml` with the same payload argument appended, detached, so codex behaves as if herdbot weren't there. If `notify` is absent, nothing is chained.
5. It exits 0 and prints nothing to stdout, always. Its own failures go to `.state/events/hook-errors.log`.

`<agent>` comes from the pane environment: `HERDBOT_RUN` set means `<bot>-<run>`, otherwise `HERDBOT_BOT`. With neither set, the hook does nothing, because it's not a herdbot pane.

Event line:

```json
{"at": 1760990000, "agent": "herdbot-dev-k3x9a1", "kind": "turn_end", "text": "…", "harness": "claude"}
{"at": 1760990000, "agent": "herdbot-dev", "kind": "blocked", "reason": "Bash: rm -rf build", "harness": "claude"}
```

`text` is capped at 64 KB, head and tail kept, with `[… N bytes cut …]` in between.

Payload mapping:

| Harness | Payload | Event |
|---|---|---|
| claude | `hook_event_name: "Stop"`, `stop_hook_active: false` | `turn_end`, `text = last_assistant_message` |
| claude | `hook_event_name: "PermissionRequest"` | `blocked`, `reason` = the tool name plus a short input summary |
| claude | `Notification` whose message asks for permission or input | `blocked` |
| claude | `Stop` with `stop_hook_active: true` | none (another hook is continuing the turn) |
| codex | `type: "agent-turn-complete"` | `turn_end`, `text = last-assistant-message` |
| codex | `type: "approval-requested"` | `blocked` |
| pi | `{event: "agent_end", text}`, which herdbot-pi.ts builds from the last assistant message | `turn_end` |
| pi | `{event: "ui_prompt_start", title}` | `blocked` |
| cursor | as found in the live check | `turn_end` / `blocked`, or no hook |
| any | anything else | none |

### Launch wiring (`route::launch`)

- **claude:** add `--settings <file>` pointing to `.state/launch/<agent>.settings.json`, which contains only `{"hooks": {"Stop": [...], "PermissionRequest": [...], "Notification": [...]}}`. Each entry runs `<herdbot exe> hook claude`. Your `~/.claude/settings.json` hooks must keep firing, and the live check confirms they stack.
- **codex:** add `-c notify=["<herdbot exe>","hook","codex"]`.
- **pi:** `herdbot-pi.ts` subscribes to `agent_end` and `ui_prompt_start`, and pipes the JSON above to `<HERDBOT_EXE> hook pi`.
- **cursor:** if the live check passes, merge a herdbot entry into `<cwd>/.cursor/hooks.json` the way `mcp.json` is merged today: added once, identity from the pane environment. Otherwise cursor has no hook and uses the pane fallback.

### Daemon consumption

Each tick, for each `.state/events/*.jsonl`, the daemon drains the file with the same rename-then-read pattern as `.queue/`, and passes each event to `apply_event`:

- **The agent is a running subagent run:**
  - `turn_end` with the run `running` and its route recorded: set `status = done`, `result = text`, `result_source = "hook"`, and queue `[subagent <id> "<desc>" finished]\n<text>` to the parent.
  - `turn_end` with the run already `done` or `failed`: ignore it.
  - The run's route isn't recorded yet: put the event back, to be handled on a later tick.
  - `blocked`: take the existing blocked path. `requeue_if_cooled` runs first, then the parent is notified once per run (`notified_blocked`).
- **The agent is a bot's main agent:**
  - `turn_end`: append `{at, prompt, reply}` to `<slug>/turns.jsonl`. `prompt` is the text the daemon last delivered to that agent (`MainAgents.prompt`), or `null` if none is pending. The pending prompt is then cleared.
  - `blocked`: log `blocked <agent>: <reason>` only.
- **Any other agent:** drop the event and log a line.

`Run` gains `result_source: "report" | "hook" | "tail"` (`#[serde(default)]`, empty for old runs). `report_result` sets `report`, and the tail harvest sets `tail`.

**Fallback grace:** for a harness whose hook is installed (claude, codex, pi, and cursor if it passes), the tail harvest in `watch_runs` waits until the agent has been settled for `HOOK_GRACE = 5 s` with no `turn_end` consumed. Runs gain `settled_at: Option<i64>`, set when the daemon first sees them settled and cleared when they're working again. A harness without a hook keeps today's behaviour.

Quota detection is unchanged. `requeue_if_cooled` and `watch_bots` still read the tail on the same transitions.

## Failure handling

- A hook never blocks, delays or changes the harness. It exits 0, prints no stdout, and runs the codex chain detached.
- A missing or broken herdbot binary breaks only the hook, and the pane fallback covers it.
- Duplicate `turn_end`s are harmless: runs are idempotent, and main-agent replies log one line each.
- Text typed straight into a main agent's pane produces a `turns.jsonl` line with `prompt: null`.
- Events for vanished agents are dropped with a log line. A malformed event line is skipped with a log line.

## Testing

Unit tests, pure:
- `parse_hook`, against payload fixtures for each harness and kind:
  - claude `Stop` / `PermissionRequest` / `stop_hook_active`
  - codex `agent-turn-complete` / `approval-requested`, plus an unknown type
  - pi `agent_end` / `ui_prompt_start`
- The 64 KB cap.
- `apply_event` on runs: running to done, ignored when done, deferred without a route, blocked once.
- `apply_event` on main agents: paired prompt, `prompt: null`.
- Reading your `notify` array out of `~/.codex/config.toml`.

Live checks (Task 1 of the plan, recorded in the README):
- claude: a subagent's `turn_end` arrives with the reply, and your own `Stop` hook still fires.
- codex: the same, and Codex Computer Use's notify still receives `turn-ended`.
- pi: the same, after the pi-MCP plan.
- cursor: find whether `hooks.json` fires and with what payload. Wire it only if it gives the reply.
- Each harness: a tool approval produces one `blocked` event.
