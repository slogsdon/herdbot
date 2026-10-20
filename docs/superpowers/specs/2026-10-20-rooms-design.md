# Rooms: Design

Approved 2026-10-20. Rooms are group conversations between herdbot bots, with Shane as a participant. They're herdbot's take on Grok Bot's group bots.

## Goals

- Several bots and Shane share one conversation, with a turn-taking mode set per room.
- It costs nothing beyond the turns that actually happen: no router model, no polling agents.
- It reuses the existing delivery path (`.queue/<agent>.jsonl`, idle-only delivery, bring-up on message, quota failover).
- It stays plain files a future GUI can read.

## Non-goals

- Importing Grok Bot `isGroup` rows. The local roster cache currently has none to learn their shape from. The importer keeps skipping them.
- Panes per member inside a room workspace. Members answer from their own main agents.
- A GUI. The transcript file is the interface the GUI will read later.

## Files

```
$HERDBOT_HOME/rooms/<slug>/room.toml
$HERDBOT_HOME/rooms/<slug>/transcript.jsonl   one JSON object per line, append-only
$HERDBOT_HOME/rooms/<slug>/transcript.md      rendered from the jsonl by the daemon
$HERDBOT_HOME/.state/rooms/<slug>.json        routing state, written only by the daemon
```

`room.toml`:

```toml
name = "Launch crew"
members = ["seo-chief", "herdbot-dev", "writer"]   # bot slugs, at least 2
mode = "channel"                # channel (default) | round_robin | moderated
moderator = "seo-chief"         # required for moderated, must be a member; ignored otherwise
max_turns = 6                   # default 6
member_cooldown_secs = 30       # default 30
turn_timeout_secs = 300         # default 300; round_robin only
```

A transcript line is `{"at": <unix secs>, "speaker": "<you | bot slug | herdbot>", "text": "…"}`. An entry's sequence number is its 1-based line number. Writers (`room send`, `room_post`, the daemon's own notices) only append one line with a single `write` on an `O_APPEND` file and never assign numbers, so concurrent writers can't collide.

`transcript.md` is rewritten from the jsonl whenever the daemon routes new lines. Each entry renders as `**<speaker>** · <HH:MM>` followed by the text.

Routing state (`.state/rooms/<slug>.json`):

| Field | Meaning |
|---|---|
| `cursor` | last line number routed |
| `last_seen` | map slug → last line number delivered to that member |
| `last_post` | map slug → unix time of that member's last post |
| `pending` | members called on but held by their cooldown |
| `bot_turns` | bot posts since Shane's last message |
| `paused` | true after `max_turns`; cleared by Shane's next message |
| `round` | round_robin only: `{ next: index into members, called_at: unix secs }`, or null between rounds |

## Interfaces

CLI:

- `herdbot room new <name> --members a,b[,c…] [--mode M] [--moderator B]`: checks that every member exists, that there are at least two, and that a moderated room has a moderator who is a member.
- `herdbot room send <room> <text…>` appends as `you`.
- `herdbot room list` prints slug, name, mode, members, and paused or active.
- `herdbot room up <room>` opens (or reuses) a herdr workspace `💬 <name>` whose one pane runs `tail -n 50 -f transcript.md`.

Bot tools (MCP and `herdbot tool`):

- `room_post{room, text}` appends as the calling bot. It's an error unless the caller is a member. Subagent runs can't post. Only main agents are members.
- `list_rooms` lists the rooms the caller belongs to, with mode and members.

The main-agent preamble gains one line: room messages arrive as `[room <slug> …]`, and you answer with `room_post`.

## Routing

The daemon, once per tick, for each room: read the lines after `cursor` and pass them with the state to a pure function

```
rooms::route(room: &Room, state: State, new: &[Entry], first_seq: usize, now: i64)
    -> (Vec<Delivery /* member slug */>, Vec<String /* herdbot notices */>, State)
```

Then append the notices, enqueue the deliveries, save the state and re-render `transcript.md`. Notices the daemon appends are `herdbot` entries. On the next tick they're routed like any other entry, but they never count as bot turns and never call anyone.

Entry by entry:

1. **Speaker `you`:** set `bot_turns = 0` and `paused = false`.
2. **Speaker is a bot:** `bot_turns += 1` and record `last_post`. When `bot_turns == max_turns`, emit `[room paused: <N> bot turns since your last message. Post to resume.]` and set `paused`. While paused, bot entries call nobody, but they stay in the transcript and are delivered as context later.
3. **Called members, by mode.** The speaker is never called back.
   - `channel`: members named in the text. A name is `@<slug>`, or the bot's display name as a whole word, case-insensitive. An entry that names nobody calls nobody.
   - `round_robin`: an entry from `you` starts a round (`round = {next: 0}`) and calls `members[0]`. When the called member posts, call `members[next+1]`. After the last member posts, `round = null`. Names are ignored. A `you` entry during a round restarts it.
   - `moderated`: entries from `you` and from non-moderator members call the moderator. Entries from the moderator call the members it names, by the channel rule.
4. **Cooldown:** a called member whose `last_post` is within `member_cooldown_secs` is added to `pending` instead. Each tick, pending members whose cooldown has passed are delivered.
5. **Delivery:** one queued prompt per member per tick, holding every entry after that member's `last_seen` (its own posts excluded). `last_seen` is set to the latest line included.

```
[room launch-crew "Launch crew"]
you: …
writer: …
(Reply with room_post room="launch-crew". Name members with @slug to bring them in. Members: seo-chief, herdbot-dev, writer.)
```

## Failure handling

- **round_robin timeout:** if the called member hasn't posted `turn_timeout_secs` after `round.called_at`, emit `[herdbot: <m> did not answer]` and call the next member.
- **channel / moderated non-answer:** nothing happens. No member is owed a turn.
- **Missing member** (bot deleted after `room new`): emit `[herdbot: <m> is not a bot any more; skipped]` once, and skip it. In round_robin it's skipped in the order.
- **Quota or auth failure in a member's turn:** the existing `watch_bots` failover restarts the member and re-queues the room prompt.
- **`room_post` by a non-member, or from a subagent run:** the tool returns an error and nothing is appended.
- **Malformed transcript line:** skipped with a daemon log line. It still counts toward line numbering.

## Testing

Unit tests on `rooms::route`, pure, with no files:

- channel: naming by `@slug` and by display name, no self-call, no names means no calls
- round_robin: order, advancing on post, end of round, restart on your message, turn timeout
- moderated: relay to the moderator, and the moderator naming members
- `max_turns` pause, herdbot notices not counting as turns, resume on your message
- cooldown deferral, and coalescing several calls into one delivery
- missing member skipped with a single notice

Plus unit tests for parsing `room.toml` with defaults, mention matching, and the delivery prompt text.

Live check: a three-bot room on cheap pi routes in each mode. It confirms naming, the round order, the pause notice after 6 bot turns, and that `transcript.md` renders in the `room up` pane.
