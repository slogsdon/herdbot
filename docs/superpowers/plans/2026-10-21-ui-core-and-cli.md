# UI Core and CLI Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Phase 0 of the UI layer: a `herdbot-core` crate holding the on-disk state and a read layer, plus `herdbot status --json`, `herdbot watch --json` and `herdbot focus <agent>`.

**Architecture:** Move `home.rs` and `cooldown.rs` (and the hook `Event` type) into a new `herdbot-core` crate, re-exported into `herdbot` so no other module changes. Add `snapshot.rs` to core: a serialisable `Snapshot` and a polling `Watcher`. The CLI is a thin wrapper over them. `focus` calls herdr's `agent.focus`.

**Tech Stack:** Rust 2024, serde/serde_json, tokio (CLI only), herdr socket protocol 22.

Spec: `docs/superpowers/specs/2026-10-21-ui-layer-design.md` (phase 0).

## Global Constraints

- Every JSON payload the CLI prints carries `"v": 1`.
- `status`/`watch` read files only and never need the daemon; `daemon` is `"up"` or `"down"` from connecting to `.state/daemon.lock`.
- Corrupt or missing files are skipped and reported in `warnings`; never a panic.
- The only writers the UI path adds are `home::enqueue` (already exists) and `agent.focus`. No new daemon surface.
- `cargo build`, `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` pass at the end of every task.
- Commits: conventional prefix, explicit paths (never `git add -A` / `.`), no AI attribution, no push.
- Spike result (verified 2026-10-21): herdr's socket has `agent.focus` with params `{"target": "<agent name>"}` (schema `AgentTarget`; `herdr agent focus <target>` is the CLI form).

## File Structure

- Create `crates/herdbot-core/Cargo.toml`, `src/lib.rs` — crate root.
- Move `crates/herdbot/src/home.rs` → `crates/herdbot-core/src/home.rs` (on-disk layout, bots, runs, queue).
- Move `crates/herdbot/src/cooldown.rs` → `crates/herdbot-core/src/cooldown.rs`.
- Create `crates/herdbot-core/src/events.rs` — `Event`, `events_dir`, `append_event` (moved out of `hook.rs`).
- Create `crates/herdbot-core/src/snapshot.rs` — `Snapshot`, `snapshot()`, `Watcher`.
- Create `crates/herdbot-core/tests/snapshot.rs` — one integration test over a fixture home.
- Modify `crates/herdbot/src/{main.rs,hook.rs}`, `crates/herdbot/Cargo.toml`, `Cargo.toml`, `crates/herdr-client/src/lib.rs`, `README.md`, the spec and `docs/superpowers/plans/README.md`.

Work in a worktree: `git -C ~/Code/herdbot worktree add .worktrees/ui-core -b feature/ui-core feature/ui-layer`.

---

### Task 1: Extract `herdbot-core`

**Files:**
- Create: `crates/herdbot-core/Cargo.toml`, `crates/herdbot-core/src/lib.rs`, `crates/herdbot-core/src/events.rs`
- Move: `home.rs`, `cooldown.rs` into `crates/herdbot-core/src/`
- Modify: `Cargo.toml`, `crates/herdbot/Cargo.toml`, `crates/herdbot/src/main.rs`, `crates/herdbot/src/hook.rs`, `crates/herdbot-core/src/home.rs`

**Interfaces:**
- Produces: `herdbot_core::{home, cooldown, events}`; `events::{Event, events_dir, append_event}`; `home::daemon_lock() -> PathBuf`; `home::{read_json, write_json}` now `pub`.
- `herdbot` keeps compiling unchanged: `crate::home::…`, `crate::cooldown::…`, `hook::Event` still resolve.

- [ ] **Step 1: Record the baseline**

Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: note the passed counts. `-p herdbot` alone is 68 today; after this task herdbot's count plus core's count must equal the old herdbot count (the moved tests travel with their files).

- [ ] **Step 2: Create the crate**

`crates/herdbot-core/Cargo.toml`:

```toml
[package]
name = "herdbot-core"
description = "herdbot's on-disk state and the read layer that UIs share"
edition.workspace = true
version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
toml = "1"
chrono = "0.4"
```

`crates/herdbot-core/src/lib.rs`:

```rust
//! herdbot's on-disk state and the read layer UIs share. No herdr, no tokio.

pub mod cooldown;
pub mod events;
pub mod home;
```

Root `Cargo.toml`: `members = ["crates/herdr-client", "crates/herdbot", "crates/herdbot-core"]`.

`crates/herdbot/Cargo.toml` under `[dependencies]`: add `herdbot-core = { path = "../herdbot-core" }`.

- [ ] **Step 3: Move the files**

```bash
git mv crates/herdbot/src/home.rs crates/herdbot-core/src/home.rs
git mv crates/herdbot/src/cooldown.rs crates/herdbot-core/src/cooldown.rs
```

In `crates/herdbot-core/src/home.rs` make `read_json` and `write_json` `pub` (from `pub(crate)`; `rooms.rs` in `herdbot` uses them), and add next to `queue_dir`:

```rust
/// The daemon's single-instance lock; a live daemon accepts connections on it.
pub fn daemon_lock() -> PathBuf {
    root().join(".state").join("daemon.lock")
}
```

- [ ] **Step 4: Move `Event` out of `hook.rs`**

`crates/herdbot-core/src/events.rs`:

```rust
//! The per-agent event log that harness hooks append to: `.state/events/<agent>.jsonl`.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub at: i64,
    pub agent: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub harness: String,
}

pub fn events_dir() -> PathBuf {
    home::root().join(".state").join("events")
}

pub fn append_event(event: &Event) -> Result<()> {
    std::fs::create_dir_all(events_dir())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_dir().join(format!("{}.jsonl", event.agent)))?;
    file.write_all(format!("{}\n", serde_json::to_string(event)?).as_bytes())?;
    Ok(())
}
```

In `crates/herdbot/src/hook.rs`: delete the `Event` struct (with its derive), `events_dir` and `append_event`, and add near the top:

```rust
pub use herdbot_core::events::{Event, append_event, events_dir};
```

- [ ] **Step 5: Re-export into the `herdbot` crate root**

In `crates/herdbot/src/main.rs` delete `mod cooldown;` and `mod home;` and add after the `use herdr_client::Client;` line:

```rust
use herdbot_core::{cooldown, home};
```

(Other modules' `crate::home::…` / `crate::cooldown::…` paths resolve through this import.) In `daemon.rs::take_lock` replace the `let path = home::root().join(".state").join("daemon.lock");` line with `let path = home::daemon_lock();`.

- [ ] **Step 6: Build, test, lint**

Run: `cargo build && cargo test --workspace 2>&1 | grep "test result" && cargo clippy --workspace -- -D warnings`
Expected: builds; counts sum to the baseline's herdbot count (68); clippy clean. Fix unused imports (`std::io::Write` in `hook.rs` may now be unused) and any `core`-only dependency errors (add the crate to core's `Cargo.toml` rather than gating code).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/herdbot/Cargo.toml crates/herdbot/src/main.rs crates/herdbot/src/hook.rs crates/herdbot/src/daemon.rs crates/herdbot-core
git commit -m "refactor: extract herdbot-core (home, cooldown, events)"
```

(`git mv` already staged the renames; `git status --short` must show nothing unexpected before committing.)

---

### Task 2: Snapshot and Watcher

**Files:**
- Create: `crates/herdbot-core/src/snapshot.rs`, `crates/herdbot-core/tests/snapshot.rs`
- Modify: `crates/herdbot-core/src/lib.rs`

**Interfaces:**
- Consumes: `home::{list_bots, load_bot, load_state, list_runs, root, now, daemon_lock}`, `cooldown::active`, `events::{Event, events_dir}`.
- Produces:
  - `snapshot::VERSION: u32 = 1`
  - `snapshot::snapshot() -> Snapshot`
  - `snapshot::Watcher` (`Default`) with `poll(&mut self) -> Vec<serde_json::Value>`
  - `snapshot::parse_events(&str) -> (Vec<Event>, usize)`, `snapshot::latest_blocked(&[Event]) -> Option<&Event>`

- [ ] **Step 1: Write the failing tests**

Add `pub mod snapshot;` to `lib.rs`. Create `crates/herdbot-core/src/snapshot.rs` containing only the unit tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str) -> String {
        format!(r#"{{"at":1,"agent":"a","kind":"{kind}","harness":"claude"}}"#)
    }

    #[test]
    fn parse_events_counts_bad_lines_and_keeps_good_ones() {
        let text = format!("{}\nnot json\n\n{}\n", line("blocked"), line("turn_end"));
        let (events, bad) = parse_events(&text);
        assert_eq!(events.len(), 2);
        assert_eq!(bad, 1);
    }

    #[test]
    fn blocked_until_a_later_event_clears_it() {
        let (events, _) = parse_events(&format!("{}\n{}\n", line("turn_end"), line("blocked")));
        assert!(latest_blocked(&events).is_some());
        let (events, _) = parse_events(&format!("{}\n{}\n", line("blocked"), line("turn_end")));
        assert!(latest_blocked(&events).is_none());
        assert!(latest_blocked(&[]).is_none());
    }
}
```

Create `crates/herdbot-core/tests/snapshot.rs`:

```rust
use std::fs;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::Path;

use herdbot_core::{home, snapshot};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn append(path: &Path, text: &str) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{text}").unwrap();
}

/// One test: it owns HERDBOT_HOME for this whole process.
#[test]
fn snapshot_and_watch_over_a_fixture_home() {
    // Short path: unix socket paths top out near 104 bytes on macOS.
    let root = std::env::temp_dir().join(format!("hbc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    // SAFETY: this file has a single test, so nothing else touches the environment.
    unsafe { std::env::set_var("HERDBOT_HOME", &root) };

    write(&root.join("alpha/bot.toml"), "name = \"alpha\"\ntitle = \"Alpha\"\n");
    write(&root.join("beta/bot.toml"), "name = \"beta\"\ntitle = \"Beta\"\n");
    write(&root.join("broken/bot.toml"), "not = [toml");
    write(
        &root.join(".state/alpha.json"),
        r#"{"workspace_id":"w1","root_pane":"p1","agent":"alpha-main"}"#,
    );
    let run = |id: &str, status: &str| {
        format!(
            r#"{{"id":"{id}","bot":"alpha","parent_agent":"alpha-main","agent":"alpha-{id}","pane":"p2","class":"code","route":"claude:sonnet","description":"do it","status":"{status}","started_at":1}}"#
        )
    };
    write(&root.join(".runs/r1.json"), &run("r1", "running"));
    write(&root.join(".runs/r2.json"), &run("r2", "done"));
    write(&root.join(".state/cooldown/claude.json"), r#"{"until":4102444800,"reason":"limit"}"#);
    write(&root.join(".state/cooldown/codex.json"), r#"{"until":1,"reason":"old"}"#);
    let blocked = r#"{"at":10,"agent":"alpha-main","kind":"blocked","reason":"approval: Bash","harness":"claude"}"#;
    let turn_end = r#"{"at":11,"agent":"beta-main","kind":"turn_end","text":"ok","harness":"claude"}"#;
    write(&root.join(".state/events/alpha-main.jsonl"), &format!("{blocked}\n"));
    write(
        &root.join(".state/events/beta-main.jsonl"),
        &format!("{}\nnot json\n{turn_end}\n", blocked.replace("alpha-main", "beta-main")),
    );

    let snap = snapshot::snapshot();
    assert_eq!(snap.v, 1);
    assert_eq!(snap.daemon, "down");
    let bots: Vec<_> = snap.bots.iter().map(|b| (b.slug.as_str(), b.up)).collect();
    assert_eq!(bots, vec![("alpha", true), ("beta", false)]);
    assert_eq!(snap.runs.len(), 1);
    assert_eq!(snap.runs[0].id, "r1");
    assert_eq!(snap.cooldowns.len(), 1);
    assert_eq!(snap.cooldowns[0].harness, "claude");
    assert_eq!(snap.blocked.len(), 1);
    assert_eq!(snap.blocked[0].agent, "alpha-main");
    assert_eq!(snap.blocked[0].reason.as_deref(), Some("approval: Bash"));
    assert!(snap.warnings.iter().any(|w| w.starts_with("broken:")));
    assert!(snap.warnings.iter().any(|w| w.contains("beta-main.jsonl")));

    // The JSON shape the Swift agent depends on.
    let json = serde_json::to_value(&snap).unwrap();
    let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["at", "blocked", "bots", "cooldowns", "daemon", "runs", "v", "warnings"]);

    // Watcher: first poll is the snapshot only, then quiet, then changes.
    let mut watcher = snapshot::Watcher::default();
    let first = watcher.poll();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["type"], "snapshot");
    assert_eq!(first[0]["v"], 1);
    assert!(watcher.poll().is_empty());

    append(
        &root.join(".state/events/alpha-main.jsonl"),
        r#"{"at":20,"agent":"alpha-main","kind":"turn_end","text":"done","harness":"claude"}"#,
    );
    let next = watcher.poll();
    let types: Vec<_> = next.iter().map(|l| l["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["snapshot", "event"]);
    assert_eq!(next[0]["snapshot"]["blocked"].as_array().unwrap().len(), 0);
    assert_eq!(next[1]["event"]["kind"], "turn_end");

    // A live daemon accepts connections on its lock socket.
    let _listener = UnixListener::bind(home::daemon_lock()).unwrap();
    assert_eq!(snapshot::snapshot().daemon, "up");

    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p herdbot-core 2>&1 | tail -15`
Expected: compile errors (`parse_events`, `latest_blocked`, `snapshot`, `Watcher` not found).

- [ ] **Step 3: Implement**

Prepend to `crates/herdbot-core/src/snapshot.rs` (above the `tests` module):

```rust
//! What a UI reads: one serialisable view of herdbot's on-disk state, and a poller that
//! turns changes into lines. Files only: neither needs the daemon or herdr.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::net::UnixStream;

use serde::Serialize;
use serde_json::{Value, json};

use crate::cooldown;
use crate::events::{self, Event};
use crate::home;

/// Bumped on any breaking change to the JSON the CLI prints.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BotView {
    pub slug: String,
    pub title: String,
    pub up: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunView {
    pub id: String,
    pub bot: String,
    pub agent: String,
    pub route: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CooldownView {
    pub harness: String,
    /// Unix seconds.
    pub until: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub v: u32,
    pub at: i64,
    /// "up" when something accepts connections on the daemon's lock socket, else "down".
    pub daemon: &'static str,
    pub bots: Vec<BotView>,
    /// Running subagent runs only.
    pub runs: Vec<RunView>,
    /// Harnesses still cooling down.
    pub cooldowns: Vec<CooldownView>,
    /// Agents whose latest event is `blocked`.
    pub blocked: Vec<Event>,
    pub warnings: Vec<String>,
}

/// Parse an event log. Unparsable lines are counted, not fatal.
pub fn parse_events(text: &str) -> (Vec<Event>, usize) {
    let mut bad = 0;
    let events = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Event>(line).map_err(|_| bad += 1).ok())
        .collect();
    (events, bad)
}

/// An agent is blocked while its latest event says so; any later event clears it.
pub fn latest_blocked(events: &[Event]) -> Option<&Event> {
    events.last().filter(|event| event.kind == "blocked")
}

fn event_logs(warnings: &mut Vec<String>) -> BTreeMap<String, Vec<Event>> {
    // ponytail: re-reads whole logs each poll; read only the tail if they grow past a few MB.
    let mut logs = BTreeMap::new();
    for entry in fs::read_dir(events::events_dir()).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(agent) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let (events, bad) = parse_events(&text);
        if bad > 0 {
            warnings.push(format!("{agent}.jsonl: skipped {bad} unparsable line(s)"));
        }
        logs.insert(agent.to_owned(), events);
    }
    logs
}

fn cooldowns() -> Vec<CooldownView> {
    let dir = home::root().join(".state").join("cooldown");
    let mut out: Vec<CooldownView> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let harness = entry.path().file_stem()?.to_str()?.to_owned();
            let cooldown = cooldown::active(&harness)?;
            Some(CooldownView { harness, until: cooldown.until, reason: cooldown.reason })
        })
        .collect();
    out.sort_by(|a, b| a.harness.cmp(&b.harness));
    out
}

pub fn snapshot() -> Snapshot {
    let mut warnings = Vec::new();
    let bots = home::list_bots()
        .into_iter()
        .filter_map(|slug| match home::load_bot(&slug) {
            Ok(bot) => Some(BotView {
                up: home::load_state(&slug).is_some(),
                title: bot.title,
                slug,
            }),
            Err(e) => {
                warnings.push(format!("{slug}: {e}"));
                None
            }
        })
        .collect();
    let mut runs: Vec<RunView> = home::list_runs()
        .into_iter()
        .filter(|run| run.status == "running")
        .map(|run| RunView {
            id: run.id,
            bot: run.bot,
            agent: run.agent,
            route: run.route,
            description: run.description,
        })
        .collect();
    runs.sort_by(|a, b| a.id.cmp(&b.id));
    let mut blocked: Vec<Event> = event_logs(&mut warnings)
        .into_values()
        .filter_map(|events| latest_blocked(&events).cloned())
        .collect();
    blocked.sort_by_key(|event| event.at);
    Snapshot {
        v: VERSION,
        at: home::now(),
        daemon: if UnixStream::connect(home::daemon_lock()).is_ok() { "up" } else { "down" },
        bots,
        runs,
        cooldowns: cooldowns(),
        blocked,
        warnings,
    }
}

/// Polls the state and says what changed. Poll about once a second.
#[derive(Default)]
pub struct Watcher {
    /// Events already reported, per agent.
    seen: BTreeMap<String, usize>,
    last: Option<Snapshot>,
}

impl Watcher {
    /// Lines to print now: a snapshot if anything but its timestamp changed, then each event
    /// logged since the last poll. The first poll prints the snapshot only; history isn't replayed.
    pub fn poll(&mut self) -> Vec<Value> {
        let primed = self.last.is_some();
        let mut out = Vec::new();
        let snap = snapshot();
        let changed = self
            .last
            .as_ref()
            .is_none_or(|last| Snapshot { at: last.at, ..snap.clone() } != *last);
        if changed {
            out.push(json!({ "v": VERSION, "type": "snapshot", "snapshot": snap }));
            self.last = Some(snap);
        }
        for (agent, events) in event_logs(&mut Vec::new()) {
            let seen = self.seen.entry(agent).or_insert(if primed { 0 } else { events.len() });
            for event in &events[(*seen).min(events.len())..] {
                out.push(json!({ "v": VERSION, "type": "event", "event": event }));
            }
            *seen = events.len();
        }
        out
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p herdbot-core 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: unit tests 2 passed, plus the home/cooldown tests from Task 1, and the integration test 1 passed. If the integration test fails on "down"/"up", check the lock path length (`home::daemon_lock()` must stay under about 100 bytes).

- [ ] **Step 5: Lint and commit**

Run: `cargo clippy --workspace -- -D warnings` (expect clean).

```bash
git add crates/herdbot-core/src/lib.rs crates/herdbot-core/src/snapshot.rs crates/herdbot-core/tests/snapshot.rs
git commit -m "feat: add snapshot and watcher to herdbot-core"
```

---

### Task 3: CLI commands and `agent.focus`

**Files:**
- Modify: `crates/herdr-client/src/lib.rs`, `crates/herdbot/src/main.rs`

**Interfaces:**
- Consumes: `herdbot_core::snapshot::{snapshot, Watcher}`; `herdr_client::Client::call`.
- Produces: `Client::agent_focus(&self, agent: &str) -> Result<Value>`; CLI `status --json`, `watch --json`, `focus <agent>`.

- [ ] **Step 1: Add `agent_focus`**

In `crates/herdr-client/src/lib.rs`, next to `agent_prompt` (read it first and match its signature style):

```rust
    /// Focus an agent's pane in herdr. Takes the agent name, as `agent.prompt` does.
    pub async fn agent_focus(&self, agent: &str) -> Result<Value> {
        self.call("agent.focus", json!({ "target": agent })).await
    }
```

- [ ] **Step 2: Wire the commands**

In `crates/herdbot/src/main.rs`: add `use std::io::Write;` and `use herdbot_core::snapshot;` to the imports, extend `USAGE`:

```
  herdbot status --json                  one snapshot of bots, runs, cooldowns and blocked agents
  herdbot watch --json                   NDJSON: a snapshot line, then changes (for UIs)
  herdbot focus <agent>                  focus an agent's pane in herdr
```

and add arms to the `match command.as_str()` (beside `"runs"`):

```rust
        "status" => {
            if !args.iter().any(|a| a == "--json") {
                return Err(USAGE.into());
            }
            println!("{}", serde_json::to_string(&snapshot::snapshot())?);
            Ok(())
        }
        "watch" => {
            if !args.iter().any(|a| a == "--json") {
                return Err(USAGE.into());
            }
            let mut watcher = snapshot::Watcher::default();
            let mut out = std::io::stdout();
            loop {
                for line in watcher.poll() {
                    // A closed pipe means the reader is gone.
                    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                        return Ok(());
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
        "focus" => {
            herdr.agent_focus(args.first().ok_or(USAGE)?).await?;
            Ok(())
        }
```

- [ ] **Step 3: Build, test, lint**

Run: `cargo build && cargo test --workspace 2>&1 | grep "test result" && cargo clippy --workspace -- -D warnings`
Expected: all pass.

- [ ] **Step 4: Smoke the read-only commands**

Run each and check the artifact, not the exit code:

```bash
./target/debug/herdbot status --json | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['v'], d['daemon'], len(d['bots']), 'bots', len(d['blocked']), 'blocked')"
./target/debug/herdbot status; echo "exit=$?"
perl -e 'alarm 3; exec @ARGV' ./target/debug/herdbot watch --json | head -2 | python3 -c "import sys,json; [print(json.loads(l)['type']) for l in sys.stdin]"
```

Expected: first prints `1 up|down N bots M blocked`; second prints the usage text with `exit=1`; third prints `snapshot` (the alarm ends `watch` after 3 s).

- [ ] **Step 5: Commit**

```bash
git add crates/herdr-client/src/lib.rs crates/herdbot/src/main.rs
git commit -m "feat: add status, watch and focus commands for UIs"
```

---

### Task 4: Live focus check and docs

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-10-21-ui-layer-design.md`, `docs/superpowers/plans/README.md`

- [ ] **Step 1: Live check `focus` against real herdr**

This moves herdr's focus, so tell Shane first and use an agent they can see. Pick one:

Run: `herdr agent list | head`
Then: `./target/debug/herdbot focus <an agent name from the list>; echo "exit=$?"` → expect `exit=0` and herdr focused that agent's pane.
Then: `./target/debug/herdbot focus no-such-agent; echo "exit=$?"` → expect a non-zero exit with an error message from herdr.
Record both outputs in the report. If `agent.focus` rejects the target format, check `herdr agent focus <target>` for what `<target>` takes (name vs pane id) and fix `agent_focus` accordingly.

- [ ] **Step 2: Docs**

- `README.md`: add the three commands to the "Other commands" block (same wording as `USAGE`), and replace the roadmap open question "Should the desktop UI be gpui-kit … or SwiftUI …?" with a one-line pointer: the UI is SwiftUI for the menu bar and quick-entry plus gpui for the later app, per `docs/superpowers/specs/2026-10-21-ui-layer-design.md`.
- Spec: in "Risks and assumptions", replace the **Focus** bullet with: "**Focus** is `agent.focus` on herdr's socket (verified 2026-10-21, protocol 22)." and delete the "Spike first" sentence in Phase 0, noting it resolved.
- `docs/superpowers/plans/README.md`: replace the `| Desktop UI | deferred | — |` row with `| [Desktop UI, phase 0: core + CLI](2026-10-21-ui-core-and-cli.md) ([spec](../specs/2026-10-21-ui-layer-design.md)) | planned | new \`herdbot-core\` crate (\`home.rs\`, \`cooldown.rs\` move), \`hook.rs\`, \`main.rs\`, \`herdr-client\`, README |`. Phases 1–2 (Swift) get their own plan after this merges.

- [ ] **Step 3: Final checks and commit**

Run: `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: all pass.

```bash
git add README.md docs/superpowers/specs/2026-10-21-ui-layer-design.md docs/superpowers/plans/README.md
git commit -m "docs: document status, watch and focus; resolve the focus spike"
```

---

## Self-review (spec coverage)

- herdbot-core with snapshot/watch/enqueue/focus: Tasks 1–3 (`enqueue` already lives in `home`, unchanged).
- `status --json`, `watch --json`, `focus`: Task 3. `"v": 1`: `VERSION`, asserted in Task 2.
- `daemon: up|down`, `warnings`: Task 2 test. Corrupt files skipped: broken `bot.toml` and a bad jsonl line in the fixture.
- Focus spike: resolved in Global Constraints, verified live in Task 4.
- Phases 1–2 (Swift) are out of scope for this plan; they get their own plan once the CLI contract is merged.
- Types used across tasks: `Snapshot`, `Watcher::poll`, `parse_events`, `latest_blocked`, `agent_focus`, `home::daemon_lock` are defined where first used and named identically later.
