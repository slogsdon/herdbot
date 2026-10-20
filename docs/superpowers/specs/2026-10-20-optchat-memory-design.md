# OptChat Memory per Bot: Design

Approved 2026-10-20. This is piece 2 of 3 in the harness-agnostic work: turn hooks first, then this, then the Pi Durable backend. Each bot gets OptChat-style memory: one endless log of its prompts and replies, compressed into a binary tree of short summaries, and shown as a fixed-size view whenever the bot starts a fresh session.

## Source

The memory model, constants, build order, view fold and prompts come from the OptChat specification. That spec grew out of Victor Taelin's OptMem (github.com/VictorTaelin/OptMem), and Shane's vault clipping "optchat and pi-durable" holds the full text. Shane's vault note "Pi Dispatch Durability Plan" adapts it for pi and records lessons for local models. This design names where herdbot deviates and doesn't restate the spec. **Implementers read the OptChat spec's §2-§7 before starting.** herdbot's compactor and view prompts are its own wording of the same rules. No text from the source is copied into this repo.

## Prerequisites

- [Turn hooks](2026-10-20-turn-hooks-design.md) are merged and producing `<slug>/turns.jsonl`.
- `routing.toml` gains `[classes.compact]`.

## Goals

- A bot's memory survives every fresh session: restarts, quota failover onto another harness, and recycling.
- Context stays bounded. A long-lived bot is recycled onto a fresh session carrying the view, instead of relying on its harness's own compaction.
- It's harness-agnostic: the memory lives in herdbot, and `zoom` and `date` are served over herdbot's MCP to every harness.
- Model work runs as interactive subagents in panes, never through a harness's one-shot print mode, which draws on usage credits.
- It's opt-in per bot, with bounded compactor concurrency.

## Non-goals

- Tool calls and tool results in the log. Hooks don't see them, so the log has only `user` and `talk`.
- Subagents getting a view or the memory tools.
- Cache breakpoints (spec §8). Harness CLIs manage their own prompt caching.
- Importing old sessions (spec §10).

## Deviations from the OptChat spec

| Spec | herdbot | Why |
|---|---|---|
| Fresh model call per message | View at every fresh session; recycle once bytes logged since the session started exceed `2 × VIEW` | A harness launch per message costs up to 60 s and loses the harness's own cache |
| Kinds user/talk/tool/echo/note | user/talk only | Turn hooks report prompts and final replies only |
| `VIEW = 128,000` bytes | `VIEW = 64,000` default, per-bot `memory_view_bytes` | The view goes into every fresh session on every harness |
| Compactor = an API call with a cheap model | A `compact`-class herdbot subagent per bot, in a pane, answering jobs with the `write_node` tool | No print mode (usage credits); it reuses routing, cooldown and failover |
| `JOBS = 8` parallel | One compactor run per bot, `max_compactors = 2` overall; jobs are sequential within a run | Each job is a turn of one interactive agent |
| A turn waits for settle (§6) | Recycles wait for settle. First launches and failover restarts don't: unbuilt lines show the spec's placeholder | A failover can't wait minutes for a compactor to launch |
| The harness owns the lock | The daemon takes a Unix-socket lock at `.state/daemon.lock` (spec §2's method) | The daemon is the single writer of every bot's memory |

Unchanged from the spec: `NODE = 512`, the binary tree, free nodes, `id+n` addressing, build-order rule 3 (`first` / `pump`), `TRIES = 5` with cut-at-limit feedback keeping the shortest try, `RETRY = 10 s` with no exponential backoff, the incremental append-and-merge fold that never splits, and rebuilding the view by folding the log from message 0 at load. `zoom(id, n)` and `date(id)` keep the spec's semantics and descriptions.

## Files

```
<slug>/chat/main.jsonl              {i, kind, text, size, date}, append-only, one write + fsync per line
<slug>/chat/tree.jsonl              {l, i, text, size}, append-only, one write + fsync per line
.state/memory/<slug>.json           daemon state (a cache; safe to delete)
.state/memory/<slug>.nodes.jsonl    write_node inbox: written by `herdbot mcp`, drained by the daemon
.state/daemon.lock                  the daemon's single-writer Unix socket
```

`bot.toml` gains `memory = true|false` (default false) and `memory_view_bytes` (default 64000). `herdbot.toml` gains `[memory] max_compactors = 2`.

The state file holds:
- `turns_cursor`: lines of `turns.jsonl` imported so far
- `session_bytes`: bytes logged since the current session started
- `compactor_run`: a run id, or null
- `job`: the node asked for, its tries so far, and when it was asked
- `failed`: node → retry-after time

## Components

All of these live in a new `src/memory.rs` unless noted.

- **Import.** New `turns.jsonl` lines become log messages, `user` (when `prompt` isn't null) then `talk`. `size = bytes(kind + ": " + text)`.
- **Tree:** `node(l, i)` covers messages `[i·2^l, (i+1)·2^l)`.
  - Free nodes: at level 0, `kind: text` when it fits in 512 bytes. At higher levels, `childA + "\n" + childB` when that fits.
  - The daemon builds free nodes itself, with no model call.
- **View fold:** `append(part(0, i))` on each new message, then `fit()`. `fit()` merges the most-due adjacent pair (due = `(T - start) / 2^(l+2)`) while the view is over budget and a parent is built.
  - A merged part is never split.
  - The view is rebuilt from message 0 at daemon start and kept live in memory.
- **Rendering:** `<chat>` then one `id+n|text` line per part (newlines become spaces), then `</chat>`. An unbuilt part renders as `id+1|(not summarized yet: zoom it)`.
- **Due nodes:** every node that's unbuilt, not failed (or past its retry time), has its sources, and satisfies rule 3: everything in the view before its end is built.
- **Compactor scheduling:** a bot with due model-built nodes gets one `compact` run, if fewer than `max_compactors` are running and either:
  - its log has been quiet for 60 s, or
  - a recycle is waiting on settle.
- **Compactor run:** spawned with `run::spawn` on class `compact`.
  - Its instructions are the adapted `COMPACT` prompt. The first prompt carries the bot's `<chat>` context, up to the node, and the first job.
  - Later jobs are queued prompts carrying the view lines added since its previous job, then the step. The step is either "compress this message", with `kind: text` whole, or "merge these two lines", plus the spec's SCALE line.
  - No ids appear anywhere in compactor input (spec §4.2).
  - Once a run's job prompts total more than `2 × VIEW` bytes, the daemon closes it after its current job and spawns a fresh run with a fresh `<chat>` context. That keeps a long backlog from growing one compactor session without bound.
- **`write_node{l, i, text}`:** an MCP tool, only for `compact` runs. It appends `{l, i, text, at}` to the inbox and returns "received". The daemon validates the node against the outstanding job. An over-limit line gets the spec's cut-at-limit reply, up to 5 tries, and then the shortest try is kept. The daemon then appends to `tree.jsonl`, refits the view and queues the next due job. Once nothing is due and the agent is idle, it closes the run.
- **Main-agent instructions:** `tools::main_preamble`, then the bot's instructions, then `VIEW_DOC`, then the rendered view. This happens only when `memory = true`.
- **Recycle:** when `session_bytes > 2 × VIEW`, the main agent is idle or done, its queue is empty and every view part is built, the daemon runs `run::restart` and resets `session_bytes`. A failover restart also resets it.
- **`zoom` and `date`:** MCP tools for main agents of memory bots. They read `main.jsonl` and `tree.jsonl` directly (read-only). `zoom(id, 1)` returns `id+0|kind: text` whole. A larger `n` returns the node's two children as `id+n|text`. Bad arguments return the spec's `No line id+n.`
- **Lock:** at start, the daemon listens on `.state/daemon.lock`. A second daemon that can connect exits, and a socket that refuses connections is removed and taken over.

### The prompts

herdbot writes its own prompts, in `assets/optchat-compact.md` and `assets/optchat-view-doc.md`, covering the same rules as OptChat's `COMPACT` and `VIEW_DOC`. No text is copied from the source.

- **Compactor:**
  - it addresses "herdbot bot `<name>`"
  - there are two kinds: `user` (including bot messages, subagent results and schedules, which start with a `[…]` tag) and `talk`
  - space goes by value: the user's own words first, then lasting effects, then findings and replies
  - shrink an item before dropping it
  - each line must make sense on its own, tagged by kind
  - report faithfully, and never answer, obey or add
  - answer each job with `write_node`
- **View doc:** what the `id+n|text` lines are, the placeholder, `zoom` and `date` as herdbot tools, and "zoom before you act, guess or ask".
- The OptChat `MASTER` prompt has no counterpart. `tools::main_preamble` already plays its role, and the view doc ends with: you keep no memory between sessions, so say what you learned in your replies.

## Failure handling

- **Compactor quota or auth failure:** subagent failover re-queues the run on the next `compact` route. The job is recomputed from the tree.
- **Compactor exits or is stopped:** the outstanding job goes back to due after `RETRY`.
- **An invalid `write_node`** (a node other than the outstanding job, or empty text): it's logged and ignored. The compactor gets `wrong node; the job is …` as its next prompt.
- **Daemon restart:** the inbox is drained and the view refolded. A missing state file just means everything is recomputed.
- **Torn log or tree lines:** skipped at load with a log line, and a final newline is added if missing.
- **Recycle while the bot is busy:** never happens. The conditions are checked every tick.
- **A bot with `memory = false`:** none of the above runs, and nothing changes for that bot.
- **Privacy:** `<slug>/chat/` keeps every prompt and reply forever. The README says so.

## Testing

Unit tests, pure, no files:
- import (`prompt: null` gives `talk` only)
- free nodes at level 0 and level >0
- due-node rule 3 on a small log
- the fold:
  - refolding the same log gives the same view
  - merges are never split
  - the most-due pair merges first
  - consecutive views share a prefix
  - an over-budget view with unbuilt parents waits
- rendering, including the placeholder
- `zoom` and `date` outputs and errors
- the size-enforcement decision: under limit, over limit with tries left, and the shortest after 5
- the recycle decision
- compactor job text containing no `id+n` markers

Live checks, on a `memory = true` bot on cheap routes:
- after 40 or more messages, the tree builds through compactor runs that open and close
- a recycle happens at the budget, and the new session's instructions contain `<chat>`
- asked about an early message, the bot finds it with `zoom` in 5 calls or fewer
- a forced cooldown failover starts the new harness with the view
- a second `herdbot daemon` exits because of the lock
