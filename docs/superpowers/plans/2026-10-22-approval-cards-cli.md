# Approval cards, CLI and daemon (plan 1 of 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A claude launch that stops at claude's folder-trust dialog is kept alive and surfaced as a pending *card* in `status`/`watch --json`; only `herdbot card answer <card-id> <option-id>` can answer it, and only if the same dialog is still on screen.

**Architecture:** Spec: `docs/superpowers/specs/2026-10-22-approval-cards-design.md` (read all of it, especially "Security properties"). Data and file I/O in `herdbot-core` (`cards.rs`); the claude-trust adapter, the answer logic and the pruning in `herdbot`. Plan 2 (`2026-10-22-approval-cards-app.md`) renders the cards.

**Tech Stack:** Rust 2024; no new dependencies (the card id is a small FNV-1a hash written in-tree). Tests are inline and pure; no environment mutation.

## Global Constraints

- **No automatic answer.** Nothing in the daemon, `up`, `restart` or any tick may call `card answer` or send the dialog's keys. The only sender is the `card answer` command.
- Keys come from the adapter's fixed table only, never from pane text. Claude trust: `trust` -> `Down`,`Enter`; `exit` -> `Enter`. (The dialog's default is "No, exit"; verified on 2026-10-22 that `Down`,`Enter` accepts it.)
- `card answer` re-reads the live pane, re-runs the adapter and refuses unless the recomputed card id equals the given one.
- The adapter requires BOTH phrases `Is this a project you created or one you trust` and `Yes, I trust this folder`; otherwise it returns None and behaviour is exactly as before this plan.
- JSON additions are additive: `Snapshot` gains `cards`; `"v": 1` stays.
- `herdbot-core` has no herdr dependency: it holds types and file I/O only.
- Live checks must never run `service install|uninstall`, `setup` without `--only`, `uninstall`, `launchctl`, or touch the running daemon; use a temp `HERDBOT_HOME` and the branch binary's own `up`/`down`/`card` commands, and remove everything created. Never kill a process by name.
- Commits: conventional messages, no AI attribution, explicit `git add` paths, no push.

**Branch/worktree:** `git -C ~/Code/herdbot worktree add .worktrees/cards-cli -b feature/cards-cli main`.

## File map

- Create `crates/herdbot-core/src/cards.rs`; export in `crates/herdbot-core/src/lib.rs`; modify `crates/herdbot-core/src/snapshot.rs`.
- Create `crates/herdbot/src/adapters.rs`, `crates/herdbot/fixtures/panes/claude-trust.txt`, `crates/herdbot/fixtures/panes/claude-ready.txt`.
- Modify `crates/herdbot/src/run.rs`, `crates/herdbot/src/daemon.rs`, `crates/herdbot/src/main.rs`.
- Modify `README.md` and `docs/superpowers/specs/2026-10-22-chat-app-design.md` (one line).

---

### Task 1: The card record and its files (`herdbot-core`)

**Files:** Create `crates/herdbot-core/src/cards.rs`; modify `lib.rs`, `snapshot.rs`.

**Interfaces:**
- Produces: `cards::{Card, CardOption, card_id, cards_dir, write_card, read_card, read_cards, remove_card, log_answer}`; `Snapshot.cards: Vec<Card>`.

- [ ] **Step 1: Failing tests** in `cards.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Card {
        Card {
            id: card_id("helper", "dialog text"),
            kind: "approval".into(),
            adapter: "claude-trust".into(),
            bot: "helper".into(),
            agent: "helper".into(),
            title: "Trust this folder?".into(),
            body: "dialog text".into(),
            options: vec![
                CardOption { id: "trust".into(), label: "Yes, I trust this folder".into(), style: "default".into() },
                CardOption { id: "exit".into(), label: "No, exit".into(), style: "destructive".into() },
            ],
            at: 100,
        }
    }

    #[test]
    fn the_id_changes_with_the_agent_or_the_text_and_is_stable() {
        assert_eq!(card_id("a", "text"), card_id("a", "text"));
        assert_ne!(card_id("a", "text"), card_id("b", "text"));
        assert_ne!(card_id("a", "text"), card_id("a", "text2"));
        assert_eq!(card_id("a", "text").len(), 16);
        assert!(card_id("a", "text").chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn json_shape_is_what_the_app_reads() {
        let value = serde_json::to_value(card()).unwrap();
        for key in ["id", "kind", "adapter", "bot", "agent", "title", "body", "options", "at"] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert_eq!(value["options"][1]["style"], "destructive");
    }

    #[test]
    fn files_round_trip_list_and_remove() {
        let dir = std::env::temp_dir().join(format!("herdbot-cards-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_cards_in(&dir).is_empty());
        write_card_in(&dir, &card()).unwrap();
        assert_eq!(read_cards_in(&dir), vec![card()]);
        assert_eq!(read_card_in(&dir, "helper"), Some(card()));
        remove_card_in(&dir, "helper");
        assert!(read_cards_in(&dir).is_empty());
        std::fs::write(dir.join("junk.json"), "not json").ok();
        assert!(read_cards_in(&dir).is_empty(), "unparsable files are skipped");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

Run `cargo test -p herdbot-core cards::tests` → FAIL.

- [ ] **Step 2: Implement `cards.rs`:**

```rust
//! Pending cards: prompts that need the user (spec: approval cards design). One file per agent in
//! `.state/cards/<agent>.json`, written by the daemon, read by `status`/`watch`, removed when answered
//! or when the prompt goes away. Types and file I/O only; recognising prompts lives in `herdbot`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardOption {
    pub id: String,
    pub label: String,
    /// "default" or "destructive".
    pub style: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    /// card_id(agent, body): changes when the prompt does.
    pub id: String,
    /// What the app renders: "approval" for now.
    pub kind: String,
    /// Which adapter recognised it, and owns the key table for its options.
    pub adapter: String,
    pub bot: String,
    pub agent: String,
    pub title: String,
    pub body: String,
    pub options: Vec<CardOption>,
    pub at: i64,
}

/// FNV-1a over `agent`, a NUL and `body`, as 16 hex digits. Not secret; it only has to change with the text.
pub fn card_id(agent: &str, body: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in agent.bytes().chain(std::iter::once(0)).chain(body.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn cards_dir() -> PathBuf {
    home::root().join(".state").join("cards")
}

fn file_in(dir: &Path, agent: &str) -> PathBuf {
    dir.join(format!("{agent}.json"))
}

pub fn write_card_in(dir: &Path, card: &Card) -> Result<()> {
    fs::create_dir_all(dir)?;
    home::write_json(&file_in(dir, &card.agent), card)
}

pub fn read_card_in(dir: &Path, agent: &str) -> Option<Card> {
    home::read_json(&file_in(dir, agent))
}

pub fn read_cards_in(dir: &Path) -> Vec<Card> {
    let mut cards: Vec<Card> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| home::read_json::<Card>(&e.path()))
        .collect();
    cards.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.agent.cmp(&b.agent)));
    cards
}

pub fn remove_card_in(dir: &Path, agent: &str) {
    let _ = fs::remove_file(file_in(dir, agent));
}

pub fn write_card(card: &Card) -> Result<()> { write_card_in(&cards_dir(), card) }
pub fn read_card(agent: &str) -> Option<Card> { read_card_in(&cards_dir(), agent) }
pub fn read_cards() -> Vec<Card> { read_cards_in(&cards_dir()) }
pub fn remove_card(agent: &str) { remove_card_in(&cards_dir(), agent) }

/// Audit trail: one line per answer in `.state/cards.log`.
pub fn log_answer(card: &Card, option: &str, via: &str) -> Result<()> {
    let line = serde_json::json!({
        "at": home::now(), "card": card.id, "kind": card.kind, "adapter": card.adapter,
        "agent": card.agent, "option": option, "via": via,
    });
    let path = home::root().join(".state").join("cards.log");
    fs::create_dir_all(path.parent().unwrap())?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}
```

Adapt `home::read_json`/`write_json` calls to their real signatures (`read_json::<T>(&Path) -> Option<T>`, `write_json(&Path, &T) -> Result<()>`). Add `pub mod cards;` to `lib.rs`. In `snapshot.rs` add `pub cards: Vec<crate::cards::Card>` to `Snapshot` (doc: "Prompts waiting for the user.") and `cards: crate::cards::read_cards(),` in `snapshot()`; fix the snapshot tests that construct `Snapshot` literals.

- [ ] **Step 3: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`; `herdbot status --json` (the branch binary) contains `"cards":[]`.
- [ ] **Step 4: Commit.**

```bash
git add crates/herdbot-core/src/cards.rs crates/herdbot-core/src/lib.rs crates/herdbot-core/src/snapshot.rs
git commit -m "feat: card records, their files and the snapshot cards field"
```

---

### Task 2: The claude-trust adapter

**Files:** Create `crates/herdbot/src/adapters.rs`, `crates/herdbot/fixtures/panes/claude-trust.txt`, `crates/herdbot/fixtures/panes/claude-ready.txt`; modify `main.rs` (`mod adapters;`).

**Interfaces:**
- Produces: `adapters::recognize(tail: &str, bot: &str, agent: &str) -> Option<Card>`, `adapters::keys(adapter: &str, option: &str) -> Option<&'static [&'static str]>`.

- [ ] **Step 1: Fixtures.** `claude-trust.txt` (captured 2026-10-22 from a real claude launch; keep the blank lines and the long rules as shown, trailing spaces are not significant):

```
❯ claude --model claude-sonnet-5-5 --mcp-config /Users/shane/Bots/.state/launch/helper.mcp.json --allowedTools mcp__herdbot --effort medium
────────────────────────────────────────────────────────────────────────────────────────────────────
 Accessing workspace:
 /Users/Shared/trustfresh-92905
 Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source project, or work from your team). If not, take a moment to review what's in this
 folder first.
 Claude Code'll be able to read, edit, and execute files here.
 Security guide
 ❯ No, exit
   Yes, I trust this folder
 Enter to confirm · Esc to cancel
```

`claude-ready.txt`:

```
                                                                                       ◐ medium · /effort
────────────────────────────────────────────────────────────────────────────────────────────────────
❯ Try "write a test for <filepath>"
────────────────────────────────────────────────────────────────────────────────────────────────────
  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents
```

- [ ] **Step 2: Failing tests** in `adapters.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const TRUST: &str = include_str!("../fixtures/panes/claude-trust.txt");
    const READY: &str = include_str!("../fixtures/panes/claude-ready.txt");

    #[test]
    fn the_real_dialog_becomes_an_approval_card() {
        let card = recognize(TRUST, "helper", "helper").expect("matches");
        assert_eq!((card.kind.as_str(), card.adapter.as_str()), ("approval", "claude-trust"));
        assert_eq!((card.bot.as_str(), card.agent.as_str()), ("helper", "helper"));
        assert_eq!(card.title, "Trust this folder?");
        assert!(card.body.contains("/Users/Shared/trustfresh-92905"));
        assert!(card.body.contains("Yes, I trust this folder"));
        assert!(!card.body.contains("claude --model"), "the launch command line is not part of the dialog");
        let ids: Vec<_> = card.options.iter().map(|o| (o.id.as_str(), o.style.as_str())).collect();
        assert_eq!(ids, [("trust", "default"), ("exit", "destructive")]);
        assert_eq!(card.id, herdbot_core::cards::card_id("helper", &card.body));
    }

    #[test]
    fn the_id_follows_the_directory_in_the_dialog() {
        let other = TRUST.replace("trustfresh-92905", "trustfresh-11111");
        assert_ne!(recognize(TRUST, "b", "a").unwrap().id, recognize(&other, "b", "a").unwrap().id);
    }

    #[test]
    fn other_screens_do_not_match() {
        assert!(recognize(READY, "b", "a").is_none());
        assert!(recognize("", "b", "a").is_none());
        assert!(recognize("Do you trust the files in this folder?\n1. Yes 2. No", "b", "a").is_none());
        // only one of the two phrases
        assert!(recognize("Is this a project you created or one you trust?", "b", "a").is_none());
        assert!(recognize("   Yes, I trust this folder", "b", "a").is_none());
    }

    #[test]
    fn the_key_table_is_fixed_and_checked() {
        assert_eq!(keys("claude-trust", "trust"), Some(&["Down", "Enter"][..]));
        assert_eq!(keys("claude-trust", "exit"), Some(&["Enter"][..]));
        assert_eq!(keys("claude-trust", "anything else"), None);
        assert_eq!(keys("unknown-adapter", "trust"), None);
    }
}
```

Run `cargo test -p herdbot adapters::tests` → FAIL.

- [ ] **Step 3: Implement.**

```rust
//! Recognises prompts that need the user and says which keys answer each option (spec: approval
//! cards design). One adapter per prompt; the key tables are fixed here and nothing derived from pane text is
//! ever sent as keys. Unknown screens match nothing.

use herdbot_core::cards::{card_id, Card, CardOption};
use herdbot_core::home;

const CLAUDE_TRUST: &str = "claude-trust";
const PHRASE_ASK: &str = "Is this a project you created or one you trust";
const PHRASE_YES: &str = "Yes, I trust this folder";

pub fn recognize(tail: &str, bot: &str, agent: &str) -> Option<Card> {
    claude_trust(tail, bot, agent)
}

/// The keys that pick `option` in `adapter`'s prompt. Claude's dialog highlights "No, exit"
/// first, so trusting needs Down before Enter.
pub fn keys(adapter: &str, option: &str) -> Option<&'static [&'static str]> {
    match (adapter, option) {
        (CLAUDE_TRUST, "trust") => Some(&["Down", "Enter"]),
        (CLAUDE_TRUST, "exit") => Some(&["Enter"]),
        _ => None,
    }
}

fn claude_trust(tail: &str, bot: &str, agent: &str) -> Option<Card> {
    if !(tail.contains(PHRASE_ASK) && tail.contains(PHRASE_YES)) {
        return None;
    }
    let lines: Vec<&str> = tail.lines().collect();
    let start = lines.iter().rposition(|l| l.contains("Accessing workspace"))?;
    let end = lines[start..].iter().position(|l| l.contains("Enter to confirm")).map(|i| start + i)?;
    let body = lines[start..=end].iter().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
    let body = body.trim().to_owned();
    Some(Card {
        id: card_id(agent, &body),
        kind: "approval".into(),
        adapter: CLAUDE_TRUST.into(),
        bot: bot.to_owned(),
        agent: agent.to_owned(),
        title: "Trust this folder?".into(),
        body,
        options: vec![
            CardOption { id: "trust".into(), label: "Yes, I trust this folder".into(), style: "default".into() },
            CardOption { id: "exit".into(), label: "No, exit".into(), style: "destructive".into() },
        ],
        at: home::now(),
    })
}
```

(`rposition` takes the LAST "Accessing workspace" so a scrolled-back earlier dialog does not win.) Run the tests → PASS.

- [ ] **Step 4: Commit.**

```bash
git add crates/herdbot/src/adapters.rs crates/herdbot/fixtures/panes/claude-trust.txt crates/herdbot/fixtures/panes/claude-ready.txt crates/herdbot/src/main.rs
git commit -m "feat: claude trust dialog adapter and its fixed key table"
```

---

### Task 3: Keep a blocked agent alive and write its card

**Files:** Modify `crates/herdbot/src/run.rs`, `crates/herdbot/src/daemon.rs`.

**Interfaces:**
- Consumes: `adapters::recognize`, `cards::{write_card, remove_card, read_cards}`.
- Produces: `run::StartBlocked { route: route::Route, card: Card }` (an `Error`), `daemon`'s per-tick `prune_cards`.

- [ ] **Step 1: Failing tests** for the pure decision, in `run.rs` tests:

```rust
    #[test]
    fn a_blocked_claude_start_with_a_matching_pane_is_a_card_otherwise_an_error() {
        let trust = include_str!("../fixtures/panes/claude-trust.txt");
        assert!(blocked_card("claude", "agent_blocked", trust, "helper", "helper").is_some());
        assert!(blocked_card("claude", "timeout", trust, "helper", "helper").is_none(), "only agent_blocked");
        assert!(blocked_card("codex", "agent_blocked", trust, "helper", "helper").is_none(), "only claude");
        assert!(blocked_card("claude", "agent_blocked", "some other prompt", "helper", "helper").is_none());
    }

    #[test]
    fn prune_keeps_a_card_only_while_the_same_prompt_is_on_a_blocked_agent() {
        let trust = include_str!("../fixtures/panes/claude-trust.txt");
        let card = crate::adapters::recognize(trust, "helper", "helper").unwrap();
        assert_eq!(prune_decision(&card, Some("blocked"), trust), Prune::Keep);
        assert_eq!(prune_decision(&card, Some("idle"), trust), Prune::Remove);
        assert_eq!(prune_decision(&card, None, trust), Prune::Remove);
        assert_eq!(prune_decision(&card, Some("blocked"), "gone"), Prune::Remove);
        let moved = trust.replace("trustfresh-92905", "trustfresh-1");
        assert!(matches!(prune_decision(&card, Some("blocked"), &moved), Prune::Replace(c) if c.id != card.id));
    }
```

Run → FAIL.

- [ ] **Step 2: Implement in `run.rs`.**

```rust
/// `start` stopped on a prompt a card can show: the agent is alive and waiting for the user.
#[derive(Debug)]
pub struct StartBlocked {
    pub route: route::Route,
    pub card: herdbot_core::cards::Card,
}
impl std::fmt::Display for StartBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is waiting for you: {}", self.card.agent, self.card.title)
    }
}
impl std::error::Error for StartBlocked {}

/// A card for a claude launch that blocked on a screen an adapter knows; None keeps today's error path.
fn blocked_card(harness: &str, error_code: &str, screen: &str, bot: &str, agent: &str) -> Option<herdbot_core::cards::Card> {
    (harness == "claude" && error_code == "agent_blocked")
        .then(|| crate::adapters::recognize(screen, bot, agent))
        .flatten()
}
```

In `start`, replace `herdr.agent_start(agent, launch.kind, pane, &launch.args, 60_000).await?;` with:

```rust
    let started = herdr
        .agent_start(agent, launch.kind, pane, &launch.args, 60_000)
        .await;
    if let Err(HerdrError::Api { code, .. }) = &started {
        // Only a bot's main agent can wait on a card; a subagent keeps the plain error.
        if run.is_none() {
            let screen = tail(herdr, agent, 60).await;
            if let Some(card) = blocked_card(launch.kind, code, &screen, slug, agent) {
                return Err(Box::new(StartBlocked { route, card }));
            }
        }
    }
    started?;
```

No keys are sent here: the card is published and the agent is left exactly as it is.

In `up` and `restart`, the `match started` error arm: before the "close the workspace" fallback, add

```rust
        Err(e) if let Some(blocked) = e.downcast_ref::<StartBlocked>() => {
            // The agent is alive and waiting for the user. Keep it, record its route (failover
            // must still see it) and publish the card; flush_queues skips it while it is blocked.
            herdbot_core::cards::write_card(&blocked.card)?;
            blocked.route.to_string()
        }
```

(if `if let` guards are unavailable on this toolchain, use `Err(e) => match e.downcast::<StartBlocked>() { Ok(b) => { …; b.route.to_string() } Err(e) => { /* existing close + clear + return Err(e) */ } }`.) Both call sites must handle it. The later `home::save_state(slug, &state)?; Ok(state)` then runs as for a normal start.

Pruning in `daemon.rs`: add `Prune`, `prune_decision(card, status, screen) -> Prune` (`Keep`, `Remove`, `Replace(Card)`: status not `Some("blocked")` or no adapter match -> `Remove`; match with the same id -> `Keep`; match with a different id -> `Replace(new)`) and `async fn prune_cards(herdr)` that, for each `cards::read_cards()`, reads `run::status` and `run::tail(…, 60)`, applies the decision, and writes/removes the file. Call it each tick next to `flush_queues`, logging errors like the other steps. A card is not a reason to bring an agent up, so `prune_cards` must not call `up`.

- [ ] **Step 3: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`.
- [ ] **Step 4: Commit.**

```bash
git add crates/herdbot/src/run.rs crates/herdbot/src/daemon.rs
git commit -m "feat: keep an agent blocked on claude's trust dialog alive and publish its card"
```

---

### Task 4: `herdbot card list` and `card answer`

**Files:** Modify `crates/herdbot/src/adapters.rs` (the pure part), `crates/herdbot/src/main.rs`.

**Interfaces:**
- Produces: `adapters::plan_answer(stored: &Card, option: &str, card_id: &str, live: Option<&Card>) -> std::result::Result<&'static [&'static str], String>`; CLI `herdbot card list [--json]`, `herdbot card answer <card-id> <option-id>`.

- [ ] **Step 1: Failing tests:**

```rust
    #[test]
    fn answer_is_refused_unless_the_same_prompt_is_still_showing() {
        let stored = recognize(TRUST, "helper", "helper").unwrap();
        let id = stored.id.clone();
        // the right id, the same prompt on screen
        assert_eq!(plan_answer(&stored, "trust", &id, Some(&stored)), Ok(&["Down", "Enter"][..]));
        assert_eq!(plan_answer(&stored, "exit", &id, Some(&stored)), Ok(&["Enter"][..]));
        // a stale or wrong id
        assert!(plan_answer(&stored, "trust", "0000000000000000", Some(&stored)).unwrap_err().contains("no pending card"));
        // the prompt on screen changed
        let moved = recognize(&TRUST.replace("trustfresh-92905", "trustfresh-1"), "helper", "helper").unwrap();
        assert!(plan_answer(&stored, "trust", &id, Some(&moved)).unwrap_err().contains("prompt changed"));
        // the prompt is gone
        assert!(plan_answer(&stored, "trust", &id, None).unwrap_err().contains("prompt changed"));
        // an option the adapter does not know
        assert!(plan_answer(&stored, "always", &id, Some(&stored)).unwrap_err().contains("unknown option"));
    }
```

Run → FAIL.

- [ ] **Step 2: Implement `plan_answer`:**

```rust
pub fn plan_answer(stored: &Card, option: &str, card_id: &str, live: Option<&Card>) -> Result<&'static [&'static str], String> {
    if stored.id != card_id {
        return Err(format!("no pending card {card_id} for {}", stored.agent));
    }
    match live {
        Some(live) if live.id == stored.id => {}
        _ => return Err("the prompt changed or is gone; nothing was sent".into()),
    }
    if !stored.options.iter().any(|o| o.id == option) {
        return Err(format!("unknown option {option}"));
    }
    keys(&stored.adapter, option).ok_or_else(|| format!("unknown option {option}"))
}
```

CLI in `main.rs` (`"card"` arm; add USAGE lines `  herdbot card list [--json]            prompts waiting for you` and `  herdbot card answer <card-id> <option-id>   answer one, after checking it is still on screen`):

- `list`: print `cards::read_cards()` as JSON array with `--json`, else one line each `id  bot  title  [option ids]`.
- `answer`: find the stored card whose `id` equals the argument among `read_cards()` (error `no pending card <id>` if none); read `run::tail(&herdr, &card.agent, 60)`; `live = adapters::recognize(&tail, &card.bot, &card.agent)`; `let keys = adapters::plan_answer(&card, option, id, live.as_ref())?`; `herdr.agent_send_keys(&card.agent, keys).await?`; `cards::log_answer(&card, option, "cli")?`; `cards::remove_card(&card.agent)`; print `answered <option>`. The command prints nothing else and sends nothing on any error path.

- [ ] **Step 3: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`.
- [ ] **Step 4: Commit.**

```bash
git add crates/herdbot/src/adapters.rs crates/herdbot/src/main.rs
git commit -m "feat: herdbot card list and card answer"
```

---

### Task 5: Live check and docs

**Files:** Modify `README.md`, `docs/superpowers/specs/2026-10-22-chat-app-design.md`.

- [ ] **Step 1: Live check** (the reproduction of 2026-10-22). It starts one real claude agent in herdr; remove everything afterwards.

```bash
T=$(mktemp -d) ; D=/Users/Shared/cardcheck-$$ ; mkdir -p "$D"
export HERDBOT_HOME=$T
B=$PWD/target/debug/herdbot
$B new cardlive --class plan --cwd "$D"
$B up cardlive            # expect: returns Ok; the agent is blocked on the dialog
$B card list --json       # expect one approval card, kind "approval", options trust/exit
$B status --json | python3 -c "import sys,json;print(len(json.load(sys.stdin)['cards']))"   # 1
herdr agent get cardlive  # agent_status blocked
$B card answer 0000000000000000 trust      # expect: error "no pending card", nothing sent, agent still blocked
```

Note `/Users/Shared/...` has no trusted parent in `~/.claude.json` (checked 2026-10-22); confirm with `python3 -c "import json,os;p=json.load(open(os.path.expanduser('~/.claude.json')))['projects'];print([k for k in p if k in ('/Users/Shared','/Users','/')])"` (expect `[]`) and stop if that prints anything. Then wait 10 s and confirm the agent is STILL blocked (nothing answered it). Only then:

```bash
ID=$($B card list --json | python3 -c "import sys,json;print(json.load(sys.stdin)[0]['id'])")
$B card answer "$ID" trust                 # expect: answered trust
sleep 6; herdr agent get cardlive          # idle
$B card list --json                        # []   (this branch has no daemon in the temp home; the answer removed the file)
cat $T/.state/cards.log                    # one JSON line, via "cli"
$B down cardlive; rm -rf "$T" "$D"; unset HERDBOT_HOME
```

Report each output. Do NOT use the real daemon or `~/Bots`. Claude will add a line for `$D` to `~/.claude.json`; leave it. Also confirm the negative path by the unit tests only.

- [ ] **Step 2: Docs.** README: a "Cards" paragraph under Harness launch (a blocked claude trust dialog becomes a card in `status`/`watch --json`; `herdbot card list`; `herdbot card answer <card-id> <option-id>` is the only thing that answers and only after re-checking the screen; `.state/cards/` and `.state/cards.log`; herdbot never answers on its own; other prompts behave as before). Chat spec: replace the non-goal "Approve or deny. Approvals stay in the harness; the chat only shows a blocked banner and a Jump to pane button." with "Approving without an explicit click. An approval is answered only by a user's click on a card bound to the exact prompt (see the approval cards design)."

- [ ] **Step 3: Commit.**

```bash
git add README.md docs/superpowers/specs/2026-10-22-chat-app-design.md
git commit -m "docs: approval cards"
```

---

## Self-review (done)

- Spec coverage: card record (T1, `adapter` field added to the record so `answer` can pick the key table), adapter + keys (T2), holding the agent in both `up` and `restart` and pruning (T3), `list`/`answer` with the staleness check and log (T4), live check and docs (T5). The app side is plan 2.
- Names: `Card`, `CardOption`, `card_id`, `recognize`, `keys`, `plan_answer`, `StartBlocked`, `blocked_card`, `prune_decision`, `Prune`.
- Deliberate non-feature: `StartBlocked` is only produced for a bot's main agent (`run.is_none()`); subagent launches keep today's error behaviour.
- Unverified: that `restart` blocks the same way as `up` (the live check covers `up` only); the unit tests cover the shared decision.
