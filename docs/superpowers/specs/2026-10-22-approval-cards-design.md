# Approval cards in the chat: Design

Approved 2026-10-22. Follows the [chat app design](2026-10-22-chat-app-design.md). Replaces its non-goal "Approve or deny. Approvals stay in the harness; the chat only shows a blocked banner and a Jump to pane button" with: an approval is answered from the chat only by an explicit click, bound to the exact prompt that is showing.

## Problem

On a machine where claude has not been told to trust `~/Bots/<bot>`, the first launch shows claude's folder dialog ("Is this a project you created or one you trust?"). Its default choice is "No, exit". herdr reports the agent as blocked during startup; `run::up` fails, closes the workspace and retries on the next tick, so the bot's first message stays queued forever (reproduced 2026-10-22; reported by Shane for the default `helper` bot).

Answering the dialog automatically would make a security decision for the user, so herdbot must not. The dialog has to reach the user, readable, and only the user's click may answer it.

## Goals

- Show a blocked approval as a card in the owning bot's chat, with the dialog's real text and one button per choice.
- Keep the blocked agent alive and waiting instead of closing and retrying it.
- Make the card a general component: new card kinds later are new shapes of the same record, not new plumbing.

## Non-goals (v1)

- Other harnesses' dialogs (codex trust, claude per-tool permission prompts).
- Remembering an answer, or any rule that answers on the user's behalf.
- Answering from the quick-entry panel.
- Cards for anything except approvals (the model allows it; no other kind is built).

## Decisions

| Question | Decision | Why |
|---|---|---|
| Scope | Claude's startup trust dialog, on a pending-card seam built for more adapters | A generic key-sending mechanism could answer the wrong thing: this very dialog's default is the destructive "No, exit". One adapter per prompt keeps each key sequence checked by hand. |
| Who may answer | Only an explicit user action: a click in the app, or the user typing the CLI command | No code path in the daemon calls `answer`. |
| UI | A card component keyed by `kind` | Extensible; an unknown kind still renders. |
| Where cards live | Live state, not history | They are not part of `turns.jsonl`; they exist while the prompt is on screen. |

## The card record

```json
{"id": "9f3a…", "kind": "approval", "bot": "helper", "agent": "helper",
 "title": "Trust this folder?", "body": "<the dialog text as shown in the pane>",
 "options": [{"id": "trust", "label": "Yes, I trust this folder", "style": "default"},
             {"id": "exit",  "label": "No, exit", "style": "destructive"}],
 "at": 1792712026}
```

- `kind` is a free string. v1 emits `approval`. `style` is `default` or `destructive`.
- `id` is a hash of the agent name and the dialog text, so it changes when the prompt changes.
- `body` is the pane text between the dialog's first and last lines, trimmed. It is the thing being approved, so the user reads it before clicking.

## Architecture

```
pane text ──> adapter (claude trust) ──> .state/cards/<agent>.json   (daemon)
                                              │
                    herdbot status|watch --json  { …, "cards": [ … ] }
                                              │
                         app: CardView(kind) in the owning bot's chat
                                              │ click
            herdbot card answer <card-id> <option-id>   (re-reads the pane first)
                                              │
                         adapter key table ──> herdr agent.send_keys
```

### Detection and holding (Rust)

- An adapter is a pure function from pane text to `Option<Card>` plus a fixed table `option id -> keys`. The v1 adapter, `claude-trust`, requires both phrases `Is this a project you created or one you trust` and `Yes, I trust this folder`. Keys: `trust` -> `Down`, `Enter`; `exit` -> `Enter` (the dialog's default is "No, exit", verified 2026-10-22; `Down`, `Enter` accepted it).
- `run::start` catches `agent_blocked` on a claude launch, reads the pane tail, and runs the adapters. On a match it returns a typed "blocked on a card" result carrying the resolved route, instead of an error. `run::up` then keeps the workspace, saves the state with that route (so quota failover still sees the agent) and writes the card file. With no adapter match, behaviour is exactly as today (error, workspace closed).
- Queued messages wait: `flush_queues` already skips agents that are not idle or done.
- Each daemon tick prunes `.state/cards/<agent>.json` when the agent is no longer blocked, the pane no longer matches, or the id has changed (and rewrites it for a changed prompt that still matches an adapter). An agent that exits (the user chose "No, exit") has its card removed and its bot state cleared, so the next queued message brings the bot up again and the card returns; the user sees the choice they made recur, not a silent loop.

### Answering (CLI)

- `herdbot card list [--json]` prints pending cards.
- `herdbot card answer <card-id> <option-id>`:
  1. loads the card file for that id (fails if there is none),
  2. reads the agent's pane now and re-runs the adapter; refuses unless the same card id still matches ("the prompt changed; nothing was sent"),
  3. sends only the keys in the adapter's table for that option,
  4. appends `{at, card, kind, option, via: "cli"}` to `.state/cards.log`,
  5. removes the card file.
- The app calls this command (the app's only seam stays the CLI). It never sends keys itself.

### Seam to the app

- `status --json` and `watch --json` gain a top-level `cards` array (additive; `"v": 1` stays). The Swift decoder treats a missing `cards` as empty.
- The owning bot is the card's `bot` field.

### App (SwiftUI)

- `HerdbotKit`: `Card` and `CardOption` decode with unknown fields and unknown `kind`/`style` values tolerated. `ChatState` exposes `cards(for: Recipient)`. A `CardAnswerState` per card tracks `idle`, `answering(option)` and `failed(message)`.
- `CardView` switches on `kind`: `approval` renders `ApprovalCardView` (title, monospaced selectable body, option buttons; the destructive option is marked and is not the default button); any other kind renders a generic card (title, body, **Jump to pane**). All cards are accessible: the body is readable by VoiceOver and the buttons carry their labels.
- The chat shows the bot's pending cards after its last message and above the composer. A click calls `herdbot card answer`; the card shows a spinner until it disappears from the next snapshot; a failure shows its message on the card and keeps the buttons.
- A new card id posts a notification once ("<bot> needs your approval"), and clicking it opens that bot's chat, through the same tracker pattern as blocked notifications. The menu bar shows the "waiting on you" state while any card is pending, and the menu lists it.

## Security properties

- No automatic answer exists anywhere; the only senders of keys are `card answer`, which a user runs or clicks.
- The answer is bound to the prompt that was shown (card id = hash of agent + text) and re-verified against the live pane immediately before sending.
- The keys come from a fixed per-adapter table; nothing derived from pane text is ever sent as keys.
- `cards.log` records every answer.
- Unknown prompts get no card and no keys: they behave as today.

## Testing

- Adapter: the dialog captured on 2026-10-22 matches; codex's trust text, the claude ready prompt and an empty string do not; the id changes when the path in the dialog changes.
- Card files: write, prune on unblocked, prune on changed text, keep on same text.
- `card answer`: stale id refused; a changed pane refused; a matching pane sends exactly `Down`, `Enter` for `trust` and `Enter` for `exit`; the log line is written. The pane reader and key sender are injected so no herdr is needed.
- `run::start`: a blocked claude launch with a matching pane returns the typed result; a non-matching one returns the error.
- Swift: decoding (missing `cards`, unknown kind, unknown style), per-bot filtering, notify-once per card id, answer-state transitions, the generic card for an unknown kind.
- Live, in a temp `HERDBOT_HOME` with a real untrusted directory (the reproduction from 2026-10-22): the card appears, nothing is sent while it is unanswered, `answer` with a wrong id is refused, `answer` with the right id and `trust` brings the agent to idle and the queued message is delivered.

## Risks and open items

- **Assumption:** claude's dialog wording and its default choice stay as seen on 2026-10-22; a wording change makes the adapter stop matching, which falls back to today's behaviour, not to a wrong answer.
- **Unverified:** that herdr reports `blocked` for the trust dialog on every launch path (`up` and `restart`); the reproduction covered `up`.
- A bot whose agent keeps exiting on "No, exit" re-shows the card each time a message is queued; that is the intended consequence of declining.
- The daemon's existing recent restart of agents (`restart` path in `run::up`) needs the same typed result; the plan must cover both call sites.
