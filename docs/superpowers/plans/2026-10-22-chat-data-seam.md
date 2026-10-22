# Chat Data Seam (UI phase 3, plan 1 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `herdbot history <bot|room:NAME> --json`, message counts and previews in `status`/`watch`, and a daemon-written in-flight marker, so a UI can show a conversation from files alone.

**Architecture:** A new `herdbot-core` module `history` reads a bot's `turns.jsonl` or a room's `transcript.jsonl`, pages from the end, and appends pending items (the in-flight prompt, then queued messages). `snapshot` gains per-bot and per-room `items`, `last_at`, `preview` and (bots) `working`. The daemon writes `.state/inflight/<bot>.json` when it delivers a prompt and clears it when it logs that turn. Everything is additive: `"v": 1` stays.

**Tech Stack:** Rust (herdbot-core, herdbot).

Spec: `docs/superpowers/specs/2026-10-22-chat-app-design.md` (Data seam).

## Global Constraints

- Branch `feature/ui-chat`, worktree `/Users/shane/Code/herdbot/.worktrees/ui-chat`. Work only there.
- Shane approved committing each task below: explicit paths only (never `git add -A` or `git add .`), the conventional message given, **no AI or tool attribution**, no push, no merge.
- Never run `cargo install`, never restart the daemon.
- Test with `cargo test --workspace`; lint with `cargo clippy --workspace -- -D warnings`.
- `"v": 1` stays. New JSON fields are additive; a bot or room with no history has `items: 0`, `last_at: null`, `preview: ""`.
- Item `n` is the 0-based index among persisted items (valid lines only), stable as new ones append; pending items have `n: null`.
- A marker older than 3600 s is ignored, so a crash cannot leave a bot "working" forever.
- Baseline before this plan: Rust 60 / 15 / 1 / 2 tests pass.
- Keep your ledger in `.superpowers/sdd/chat-data-seam/progress.md` (untracked; do not commit it).

All code below was compiled, its tests run, and the CLI smoke-tested in a scratch copy before this plan was written. Reproduce it exactly; if a step fails, report it, don't redesign.

---

### Task 1: the history module and the in-flight marker

**Files:**
- Create: `crates/herdbot-core/src/history.rs`, `crates/herdbot-core/tests/history.rs`
- Modify: `crates/herdbot-core/src/lib.rs`, `crates/herdbot-core/src/home.rs`, `crates/herdbot-core/src/rooms.rs`, `crates/herdbot/src/rooms.rs`

**Interfaces:**
- Produces: `herdbot_core::history::{Item, History, VERSION, DEFAULT_LIMIT, bot_items, room_items, pending_items, preview, history}`; `home::{Inflight, INFLIGHT_STALE_SECS, write_inflight, clear_inflight, read_inflight}`; `herdbot_core::rooms::{Entry, read_entries}` (moved from the `herdbot` crate, which re-exports them so its callers are unchanged).

- [ ] **Step 1: Write the failing integration test**

Create `crates/herdbot-core/tests/history.rs`:

```rust
use std::fs;
use std::path::Path;

use herdbot_core::{history, home};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// One test: it owns HERDBOT_HOME for this whole process.
#[test]
fn history_pages_and_appends_what_is_pending() {
    let root = std::env::temp_dir().join(format!("hbh-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    // SAFETY: this file has a single test, so nothing else touches the environment.
    unsafe { std::env::set_var("HERDBOT_HOME", &root) };

    write(&root.join("alpha/bot.toml"), "name = \"alpha\"\ntitle = \"Alpha\"\n");
    let turns: Vec<String> = (0..4)
        .map(|i| format!(r#"{{"at":{i},"prompt":"q{i}","reply":"a{i}"}}"#))
        .collect();
    write(&root.join("alpha/turns.jsonl"), &format!("{}\nnot json\n", turns.join("\n")));
    write(
        &root.join(".queue/alpha.jsonl"),
        "{\"text\":\"queued one\",\"at\":50}\n{\"text\":\"queued two\",\"at\":51}\n",
    );
    write(&root.join(".state/inflight/alpha.json"), &format!(r#"{{"prompt":"working on it","at":{}}}"#, home::now()));

    // Newest page: persisted items are numbered; pending ones follow with no number.
    let all = history::history("alpha", None, 50).unwrap();
    assert_eq!((all.v, all.total, all.items.len()), (1, 8, 11));
    assert_eq!(all.warnings, ["skipped 1 unparsable line(s)"]);
    assert_eq!((all.items[0].n, all.items[0].kind, all.items[0].text.as_str()), (Some(0), "user", "q0"));
    assert_eq!((all.items[1].n, all.items[1].kind, all.items[1].text.as_str()), (Some(1), "bot", "a0"));
    let pending: Vec<_> = all.items[8..].iter().map(|i| (i.n, i.state, i.text.as_str())).collect();
    assert_eq!(
        pending,
        [(None, Some("inflight"), "working on it"), (None, Some("queued"), "queued one"), (None, Some("queued"), "queued two")]
    );

    // Paging from the end: the last 3 persisted items, then the 3 before them.
    let page = history::history("alpha", None, 3).unwrap();
    let numbers: Vec<_> = page.items.iter().map(|i| i.n).collect();
    assert_eq!(numbers, [Some(5), Some(6), Some(7), None, None, None]);
    let older = history::history("alpha", Some(5), 3).unwrap();
    let numbers: Vec<_> = older.items.iter().map(|i| i.n).collect();
    assert_eq!(numbers, [Some(2), Some(3), Some(4)]);
    // An older page has no pending items; a page past the start is just short.
    assert!(history::history("alpha", Some(2), 10).unwrap().items.iter().all(|i| i.n.is_some()));
    assert!(history::history("alpha", Some(0), 10).unwrap().items.is_empty());

    // A stale marker is a crash's leftover, not a bot still working.
    write(&root.join(".state/inflight/alpha.json"), r#"{"prompt":"old","at":1}"#);
    assert!(history::history("alpha", None, 50).unwrap().items.iter().all(|i| i.state != Some("inflight")));
    home::write_inflight("alpha", "fresh").unwrap();
    assert_eq!(home::read_inflight("alpha").unwrap().prompt, "fresh");
    home::clear_inflight("alpha");
    assert!(home::read_inflight("alpha").is_none());

    // A prompt-less turn (typed in the pane) is a bot item alone.
    write(&root.join("beta/bot.toml"), "name = \"beta\"\n");
    write(&root.join("beta/turns.jsonl"), "{\"at\":1,\"prompt\":null,\"reply\":\"typed\"}\n");
    let beta = history::history("beta", None, 50).unwrap();
    assert_eq!(beta.items.len(), 1);
    assert_eq!(beta.items[0].kind, "bot");

    // Rooms: you, bots (with their slug), notices; no pending items.
    write(&root.join("rooms/crew/room.toml"), "name = \"Crew\"\nmembers = [\"alpha\", \"beta\"]\n");
    write(
        &root.join("rooms/crew/transcript.jsonl"),
        concat!(
            "{\"at\":1,\"speaker\":\"you\",\"text\":\"hi all\"}\n",
            "{\"at\":2,\"speaker\":\"alpha\",\"text\":\"hello\"}\n",
            "oops\n",
            "{\"at\":3,\"speaker\":\"herdbot\",\"text\":\"beta is down\"}\n",
        ),
    );
    let room = history::history("room:crew", None, 50).unwrap();
    assert_eq!(room.total, 3);
    let kinds: Vec<_> = room.items.iter().map(|i| (i.kind, i.speaker.as_deref())).collect();
    assert_eq!(kinds, [("user", None), ("bot", Some("alpha")), ("notice", None)]);
    assert_eq!(room.warnings, ["skipped 1 unparsable line(s)"]);

    // Unknown targets are errors, not empty conversations.
    assert!(history::history("nobody", None, 50).is_err());
    assert!(history::history("room:nowhere", None, 50).is_err());

    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p herdbot-core --test history`
Expected: compile FAIL (`herdbot_core::history` and `write_inflight` do not exist).

- [ ] **Step 3: Move `Entry` and `read_entries` into core**

Apply both diffs (the `herdbot` crate re-exports them, so nothing else changes):

```diff
--- a/crates/herdbot-core/src/rooms.rs
+++ b/crates/herdbot-core/src/rooms.rs
@@ -76,6 +76,14 @@
     pub missing_notified: BTreeSet<String>,
 }
 
+#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
+pub struct Entry {
+    pub at: i64,
+    /// "you", a bot slug, or "herdbot".
+    pub speaker: String,
+    pub text: String,
+}
+
 pub fn parse_room(text: &str) -> Result<Room> {
     Ok(toml::from_str(text)?)
 }
@@ -110,3 +118,12 @@
 pub fn load_state(slug: &str) -> State {
     home::read_json(&state_path(slug)).unwrap_or_default()
 }
+
+/// Every transcript line, `None` for a malformed one, so index `i` is always sequence `i + 1`.
+pub fn read_entries(slug: &str) -> Vec<Option<Entry>> {
+    fs::read_to_string(room_dir(slug).join("transcript.jsonl"))
+        .unwrap_or_default()
+        .lines()
+        .map(|l| serde_json::from_str(l).ok())
+        .collect()
+}
```

```diff
--- a/crates/herdbot/src/rooms.rs
+++ b/crates/herdbot/src/rooms.rs
@@ -6,11 +6,9 @@
 use std::fs;
 use std::io::Write;
 
-use serde::{Deserialize, Serialize};
-
 use crate::home::{self, Result};
 pub use herdbot_core::rooms::{
-    Mode, Room, Round, State, list_rooms, load_room, load_state, room_dir,
+    Entry, Mode, Room, Round, State, list_rooms, load_room, load_state, read_entries, room_dir,
 };
 use herdbot_core::rooms::state_path;
 
@@ -20,14 +18,6 @@
     Ok(())
 }
 
-#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
-pub struct Entry {
-    pub at: i64,
-    /// "you", a bot slug, or "herdbot".
-    pub speaker: String,
-    pub text: String,
-}
-
 pub fn validate(room: &Room, bots: &[String]) -> std::result::Result<(), String> {
     if room.members.len() < 2 {
         return Err("a room needs at least two members".into());
@@ -58,15 +48,6 @@
         .open(room_dir(slug).join("transcript.jsonl"))?;
     file.write_all(entry_line(speaker, text, home::now()).as_bytes())?;
     Ok(())
-}
-
-/// Every line, `None` for a malformed one, so index `i` is always sequence `i + 1`.
-pub fn read_entries(slug: &str) -> Vec<Option<Entry>> {
-    fs::read_to_string(room_dir(slug).join("transcript.jsonl"))
-        .unwrap_or_default()
-        .lines()
-        .map(|l| serde_json::from_str(l).ok())
-        .collect()
 }
 
 pub fn save_state(slug: &str, state: &State) -> Result<()> {
```

- [ ] **Step 4: Add the marker to `home.rs` and register the module**

```diff
--- a/crates/herdbot-core/src/home.rs
+++ b/crates/herdbot-core/src/home.rs
@@ -288,6 +288,37 @@
     Ok(())
 }
 
+/// A delivered prompt whose turn has not ended yet. `turns.jsonl` only gets the prompt with its
+/// reply, so this is how a UI shows the message while the bot works.
+#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
+pub struct Inflight {
+    pub prompt: String,
+    pub at: i64,
+}
+
+/// A marker older than this is a crash's leftover, not a bot still working.
+pub const INFLIGHT_STALE_SECS: i64 = 3600;
+
+fn inflight_path(bot: &str) -> PathBuf {
+    root().join(".state").join("inflight").join(format!("{bot}.json"))
+}
+
+/// Mark `prompt` as delivered to `bot`'s main agent. A later delivery replaces it.
+pub fn write_inflight(bot: &str, prompt: &str) -> Result<()> {
+    let path = inflight_path(bot);
+    fs::create_dir_all(path.parent().unwrap())?;
+    write_json(&path, &Inflight { prompt: prompt.to_owned(), at: now() })
+}
+
+pub fn clear_inflight(bot: &str) {
+    let _ = fs::remove_file(inflight_path(bot));
+}
+
+/// The marker, unless it is stale.
+pub fn read_inflight(bot: &str) -> Option<Inflight> {
+    read_json::<Inflight>(&inflight_path(bot)).filter(|m| now() - m.at < INFLIGHT_STALE_SECS)
+}
+
 pub fn queue_dir() -> PathBuf {
     sub(".queue")
 }
```

```diff
--- a/crates/herdbot-core/src/lib.rs
+++ b/crates/herdbot-core/src/lib.rs
@@ -2,6 +2,7 @@
 
 pub mod cooldown;
 pub mod events;
+pub mod history;
 pub mod home;
 pub mod rooms;
 pub mod snapshot;
```

- [ ] **Step 5: Create `crates/herdbot-core/src/history.rs`**

```rust
//! A conversation as a UI shows it: finished turns (a bot) or posts (a room), then whatever is
//! still on its way. Files only: it needs neither the daemon nor herdr.

use std::fs;

use serde::Serialize;
use serde_json::Value;

use crate::home::{self, Result};
use crate::rooms;

/// Bumped with `snapshot::VERSION`: both are the JSON the CLI prints.
pub const VERSION: u32 = crate::snapshot::VERSION;

/// Items per page unless the caller asks for another size.
pub const DEFAULT_LIMIT: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    /// Index among the persisted items, stable as new ones append. `None` while pending.
    pub n: Option<usize>,
    pub at: i64,
    /// "user", "bot" or "notice".
    pub kind: &'static str,
    /// A room's speaker (a bot slug) for `bot` items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    pub text: String,
    /// "inflight" or "queued" for pending items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct History {
    pub v: u32,
    pub target: String,
    /// Persisted items in the whole conversation; pending ones are not counted.
    pub total: usize,
    pub items: Vec<Item>,
    pub warnings: Vec<String>,
}

fn item(at: i64, kind: &'static str, speaker: Option<String>, text: &str, state: Option<&'static str>) -> Item {
    Item { n: None, at, kind, speaker, text: text.to_owned(), state }
}

/// Number the persisted items in order.
fn numbered(mut items: Vec<Item>) -> Vec<Item> {
    for (i, it) in items.iter_mut().enumerate() {
        it.n = Some(i);
    }
    items
}

/// A bot's turns: each `turns.jsonl` line gives your prompt (absent when typed in the pane), then
/// the reply. Also the count of unparsable lines.
// ponytail: re-reads the whole log each call; read only the tail if logs grow past a few MB.
pub fn bot_items(slug: &str) -> (Vec<Item>, usize) {
    let text = fs::read_to_string(home::bot_dir(slug).join("turns.jsonl")).unwrap_or_default();
    let mut items = Vec::new();
    let mut bad = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(turn) = serde_json::from_str::<Value>(line) else {
            bad += 1;
            continue;
        };
        let at = turn["at"].as_i64().unwrap_or_default();
        if let Some(prompt) = turn["prompt"].as_str() {
            items.push(item(at, "user", None, prompt, None));
        }
        if let Some(reply) = turn["reply"].as_str() {
            items.push(item(at, "bot", None, reply, None));
        }
    }
    (numbered(items), bad)
}

/// A room's transcript: you, each bot (with its slug as `speaker`), and herdbot's notices.
pub fn room_items(slug: &str) -> (Vec<Item>, usize) {
    let entries = rooms::read_entries(slug);
    let bad = entries.iter().filter(|e| e.is_none()).count();
    let items = entries
        .into_iter()
        .flatten()
        .map(|e| match e.speaker.as_str() {
            "you" => item(e.at, "user", None, &e.text, None),
            "herdbot" => item(e.at, "notice", None, &e.text, None),
            bot => item(e.at, "bot", Some(bot.to_owned()), &e.text, None),
        })
        .collect();
    (numbered(items), bad)
}

/// What a bot has been sent but not logged: the in-flight prompt, then each queued message.
/// The main agent's name is the bot's slug (`run::main_agent`), which names its queue file.
pub fn pending_items(slug: &str) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(sent) = home::read_inflight(slug) {
        out.push(item(sent.at, "user", None, &sent.prompt, Some("inflight")));
    }
    let queue = fs::read_to_string(home::queue_dir().join(format!("{slug}.jsonl"))).unwrap_or_default();
    for queued in queue.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        if let Some(text) = queued["text"].as_str() {
            out.push(item(queued["at"].as_i64().unwrap_or_default(), "user", None, text, Some("queued")));
        }
    }
    out
}

/// The first line of `text`, cut to 120 characters, for a list row.
pub fn preview(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().chars().take(120).collect()
}

/// `target` is a bot slug or `room:<slug>`. Pages from the end: `before` is an item `n` and the
/// page is the `limit` items ending just before it. Pending items ride along on the newest page.
pub fn history(target: &str, before: Option<usize>, limit: usize) -> Result<History> {
    let (items, bad, pending) = match target.strip_prefix("room:") {
        Some(slug) => {
            rooms::load_room(slug)?;
            let (items, bad) = room_items(slug);
            (items, bad, Vec::new())
        }
        None => {
            home::load_bot(target)?;
            let (items, bad) = bot_items(target);
            (items, bad, pending_items(target))
        }
    };
    let total = items.len();
    let end = before.map_or(total, |n| n.min(total));
    let mut page = items[end.saturating_sub(limit)..end].to_vec();
    if before.is_none() {
        page.extend(pending);
    }
    let warnings = if bad > 0 { vec![format!("skipped {bad} unparsable line(s)")] } else { Vec::new() };
    Ok(History { v: VERSION, target: target.to_owned(), total, items: page, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_the_first_nonblank_line_cut_short() {
        assert_eq!(preview("\n  hello \nworld"), "hello");
        assert_eq!(preview(""), "");
        assert_eq!(preview(&"x".repeat(300)).len(), 120);
    }
}
```

- [ ] **Step 6: Run to see it pass**

Run: `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS. Counts: 60 / 16 / 1 / 1 / 2 (the `herdbot` tests, the core unit tests with the new `preview` test, the existing snapshot test, the new history test, `herdr-client`). Clippy clean, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/herdbot-core/src/history.rs crates/herdbot-core/tests/history.rs crates/herdbot-core/src/lib.rs crates/herdbot-core/src/home.rs crates/herdbot-core/src/rooms.rs crates/herdbot/src/rooms.rs
git commit -m "feat: read a conversation's history from files"
```

---

### Task 2: counts and previews in the snapshot, and the `history` command

**Files:**
- Modify: `crates/herdbot-core/src/snapshot.rs`, `crates/herdbot-core/tests/snapshot.rs`, `crates/herdbot/src/main.rs`

**Interfaces:**
- Consumes: `history::{bot_items, room_items, preview}` and `home::read_inflight` from Task 1.
- Produces: `BotView { items, last_at, preview, working }` and `RoomView { items, last_at, preview }` in `status`/`watch` JSON; `herdbot history <target> --json [--before N] [--limit L]`.

- [ ] **Step 1: Extend the golden test (it fails first)**

```diff
--- a/crates/herdbot-core/tests/snapshot.rs
+++ b/crates/herdbot-core/tests/snapshot.rs
@@ -40,6 +40,12 @@
     write(&root.join("rooms/quiet/room.toml"), "name = \"Quiet\"\nmembers = [\"alpha\", \"beta\"]\n");
     write(&root.join(".state/rooms/quiet.json"), r#"{"paused":true}"#);
     write(&root.join("rooms/torn/room.toml"), "mode = \"chaos\"");
+    write(
+        &root.join("alpha/turns.jsonl"),
+        "{\"at\":5,\"prompt\":\"hi\",\"reply\":\"\\n  Hello there\\nmore\"}\nnot json\n{\"at\":7,\"prompt\":null,\"reply\":\"typed in the pane\"}\n",
+    );
+    write(&root.join(".state/inflight/beta.json"), &format!(r#"{{"prompt":"p","at":{}}}"#, home::now()));
+    write(&root.join("rooms/crew/transcript.jsonl"), "{\"at\":9,\"speaker\":\"you\",\"text\":\"hello crew\"}\n");
     write(&root.join(".runs/r1.json"), &run("r1", "running"));
     write(&root.join(".runs/r2.json"), &run("r2", "done"));
     write(&root.join(".state/cooldown/claude.json"), r#"{"until":4102444800,"reason":"limit"}"#);
@@ -57,8 +63,14 @@
     assert_eq!(snap.daemon, "down");
     let bots: Vec<_> = snap.bots.iter().map(|b| (b.slug.as_str(), b.up)).collect();
     assert_eq!(bots, vec![("alpha", true), ("beta", false)]);
+    // alpha: user+bot from the first turn, bot only from the second; one bad line skipped.
+    assert_eq!((snap.bots[0].items, snap.bots[0].last_at, snap.bots[0].preview.as_str()), (3, Some(7), "typed in the pane"));
+    assert!(!snap.bots[0].working);
+    assert_eq!((snap.bots[1].items, snap.bots[1].last_at, snap.bots[1].preview.as_str()), (0, None, ""));
+    assert!(snap.bots[1].working);
     let rooms: Vec<_> = snap.rooms.iter().map(|r| (r.slug.as_str(), r.name.as_str(), r.paused)).collect();
     assert_eq!(rooms, vec![("crew", "Crew", false), ("quiet", "Quiet", true)]);
+    assert_eq!((snap.rooms[0].items, snap.rooms[0].last_at, snap.rooms[0].preview.as_str()), (1, Some(9), "hello crew"));
     assert!(snap.warnings.iter().any(|w| w.starts_with("room torn:")));
     assert_eq!(snap.runs.len(), 1);
     assert_eq!(snap.runs[0].id, "r1");
@@ -76,7 +88,11 @@
     keys.sort();
     assert_eq!(keys, ["at", "blocked", "bots", "cooldowns", "daemon", "rooms", "runs", "v", "warnings"]);
 
-    assert_eq!(json["rooms"][0], serde_json::json!({"slug":"crew","name":"Crew","mode":"round_robin","paused":false}));
+    assert_eq!(
+        json["bots"][0],
+        serde_json::json!({"slug":"alpha","title":"Alpha","up":true,"items":3,"last_at":7,"preview":"typed in the pane","working":false})
+    );
+    assert_eq!(json["rooms"][0], serde_json::json!({"slug":"crew","name":"Crew","mode":"round_robin","paused":false,"items":1,"last_at":9,"preview":"hello crew"}));
 
     // Watcher: first poll is the snapshot only, then quiet, then changes.
     let mut watcher = snapshot::Watcher::default();
```

Run: `cargo test -p herdbot-core --test snapshot`
Expected: compile FAIL (`items`, `last_at`, `preview`, `working` do not exist on the views).

- [ ] **Step 2: Implement**

```diff
--- a/crates/herdbot-core/src/snapshot.rs
+++ b/crates/herdbot-core/src/snapshot.rs
@@ -10,6 +10,7 @@
 
 use crate::cooldown;
 use crate::events::{self, Event};
+use crate::history;
 use crate::home;
 use crate::rooms;
 
@@ -21,6 +22,14 @@
     pub slug: String,
     pub title: String,
     pub up: bool,
+    /// Persisted history items (see `history::history`'s `total`).
+    pub items: usize,
+    /// Unix seconds of the last item.
+    pub last_at: Option<i64>,
+    /// First line of the last item, up to 120 characters.
+    pub preview: String,
+    /// A prompt was delivered and its turn has not been logged yet.
+    pub working: bool,
 }
 
 #[derive(Debug, Clone, PartialEq, Serialize)]
@@ -29,6 +38,9 @@
     pub name: String,
     pub mode: rooms::Mode,
     pub paused: bool,
+    pub items: usize,
+    pub last_at: Option<i64>,
+    pub preview: String,
 }
 
 #[derive(Debug, Clone, PartialEq, Serialize)]
@@ -125,11 +137,18 @@
     let bots = home::list_bots()
         .into_iter()
         .filter_map(|slug| match home::load_bot(&slug) {
-            Ok(bot) => Some(BotView {
-                up: home::load_state(&slug).is_some(),
-                title: bot.title,
-                slug,
-            }),
+            Ok(bot) => {
+                let (items, _) = history::bot_items(&slug);
+                Some(BotView {
+                    up: home::load_state(&slug).is_some(),
+                    title: bot.title,
+                    items: items.len(),
+                    last_at: items.last().map(|i| i.at),
+                    preview: items.last().map(|i| history::preview(&i.text)).unwrap_or_default(),
+                    working: home::read_inflight(&slug).is_some(),
+                    slug,
+                })
+            }
             Err(e) => {
                 warnings.push(format!("{slug}: {e}"));
                 None
@@ -139,12 +158,18 @@
     let rooms = rooms::list_rooms()
         .into_iter()
         .filter_map(|slug| match rooms::load_room(&slug) {
-            Ok(room) => Some(RoomView {
-                paused: rooms::load_state(&slug).paused,
-                name: room.name,
-                mode: room.mode,
-                slug,
-            }),
+            Ok(room) => {
+                let (items, _) = history::room_items(&slug);
+                Some(RoomView {
+                    paused: rooms::load_state(&slug).paused,
+                    name: room.name,
+                    mode: room.mode,
+                    items: items.len(),
+                    last_at: items.last().map(|i| i.at),
+                    preview: items.last().map(|i| history::preview(&i.text)).unwrap_or_default(),
+                    slug,
+                })
+            }
             Err(e) => {
                 warnings.push(format!("room {slug}: {e}"));
                 None
```

```diff
--- a/crates/herdbot/src/main.rs
+++ b/crates/herdbot/src/main.rs
@@ -15,7 +15,7 @@
 use std::io::Write;
 
 use herdr_client::Client;
-use herdbot_core::{cooldown, home, snapshot};
+use herdbot_core::{cooldown, history, home, snapshot};
 
 use home::Result;
 
@@ -27,6 +27,7 @@
   herdbot list | runs
   herdbot status --json                  one snapshot of bots, runs, cooldowns and blocked agents
   herdbot watch --json                   NDJSON: a snapshot line, then changes (for UIs)
+  herdbot history <bot|room:NAME> --json [--before N] [--limit L]   a conversation, newest page first
   herdbot focus <agent>                  focus an agent's pane in herdr
   herdbot up <bot> | down <bot>
   herdbot send <bot> <text…>             queue a prompt for the bot (needs the daemon)
@@ -162,7 +163,18 @@
                     }
                 }
                 tokio::time::sleep(std::time::Duration::from_secs(1)).await;
+            }
+        }
+        "history" => {
+            if !args.iter().any(|a| a == "--json") {
+                return Err(USAGE.into());
             }
+            let number = |value: Option<String>| value.map(|v| v.parse::<usize>()).transpose();
+            let before = number(flag(&mut args, "--before"))?;
+            let limit = number(flag(&mut args, "--limit"))?.unwrap_or(history::DEFAULT_LIMIT);
+            let target = args.iter().find(|a| *a != "--json").ok_or(USAGE)?;
+            println!("{}", serde_json::to_string(&history::history(target, before, limit)?)?);
+            Ok(())
         }
         "focus" => {
             herdr.agent_focus(args.first().ok_or(USAGE)?).await?;
```

- [ ] **Step 3: Verify**

Run: `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS, counts unchanged from Task 1.

Then smoke-test the command against a throwaway home (use a `mktemp -d` outside the repo; it must not touch `~/Bots`):

```bash
export HERDBOT_HOME=$(mktemp -d); B=target/debug/herdbot
$B new alpha; printf '{"at":1,"prompt":"hi","reply":"hello"}\n{"at":2,"prompt":null,"reply":"two"}\n' > $HERDBOT_HOME/alpha/turns.jsonl
$B send alpha "queued msg"
$B history alpha --json --limit 2     # items n 1 and 2, then the queued one with "state":"queued"; total 3
$B history alpha --json --before 2 --limit 5   # items n 0 and 1 only
$B history nobody --json; echo "rc=$?"          # error text, rc=1
$B status --json | python3 -c "import sys,json;print(json.load(sys.stdin)['bots'])"  # items 3, last_at 2, preview "two", working false
rm -r $HERDBOT_HOME
```

- [ ] **Step 4: Commit**

```bash
git add crates/herdbot-core/src/snapshot.rs crates/herdbot-core/tests/snapshot.rs crates/herdbot/src/main.rs
git commit -m "feat: add history command and conversation counts to status"
```

---

### Task 3: the daemon's in-flight marker and docs

No unit test: the two call sites are in the daemon's delivery and turn-log paths, which need herdr. The marker functions are tested in Task 1; Shane runs the live check.

**Files:**
- Modify: `crates/herdbot/src/daemon.rs`, `README.md`

**Interfaces:**
- Consumes: `home::write_inflight`, `home::clear_inflight` from Task 1.

- [ ] **Step 1: Wire the marker into the daemon**

When a prompt is delivered to a main agent the daemon already remembers it in `mains.delivered`; mirror it on disk. When that agent's turn is logged, clear it. A launch-turn `READY` that is dropped (the `else` branch) must not clear a marker that belongs to a later delivery, so clear only where `append_turn` runs.

```diff
--- a/crates/herdbot/src/daemon.rs
+++ b/crates/herdbot/src/daemon.rs
@@ -174,6 +174,8 @@
             if home::load_state(&agent).is_some() {
                 mains.status.insert(agent.clone(), "working".into());
                 mains.prompt.insert(agent.clone(), text.clone());
+                // Mirror `delivered` on disk so a UI can show the prompt while the turn runs.
+                home::write_inflight(&agent, &text)?;
                 mains.delivered.insert(agent.clone(), text);
             }
             println!("delivered {} message(s) to {agent}", texts.len());
@@ -282,6 +284,7 @@
                         &reply,
                     ) {
                         hook::append_turn(&event.agent, event.at, prompt.as_deref(), &reply)?;
+                        home::clear_inflight(&event.agent);
                     } else {
                         println!(
                             "launch {}: dropped READY reply: {}",
```

- [ ] **Step 2: Docs**

In `README.md`:
1. In the command list that has `herdbot watch --json` (around line 47), add after it:
   `herdbot history <bot|room:NAME> --json [--before N] [--limit L]   a conversation's items, newest page first (for UIs)`
2. In "### Delivery goes through a queue", add this paragraph at its end:

   > When the daemon delivers a prompt to a bot's main agent it also writes `.state/inflight/<bot>.json` (`{"prompt","at"}`), and removes it when that turn is logged to `turns.jsonl`. `turns.jsonl` only gets a line when a turn ends, so this marker is how a UI shows your message while the bot works (`working` in `status --json`, and an `inflight` item in `history`). A marker older than an hour is ignored. If two prompts are delivered before a turn ends, only the latest is shown.

- [ ] **Step 3: Verify and commit**

Run: `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS, counts unchanged.

```bash
git add crates/herdbot/src/daemon.rs README.md
git commit -m "feat: record the in-flight prompt for UIs"
```

## Self-review

- Spec coverage: `history` command with paging, pending items and room support (Tasks 1-2); `items`/`last_at`/`preview`/`working` in the snapshot (Task 2); the in-flight marker with a staleness cutoff (Tasks 1 and 3); additive `v: 1` (Task 2 golden test pins the shapes).
- Not verifiable without herdr, so left for Shane after installing and restarting the daemon: send a message to a real bot and check that `.state/inflight/<bot>.json` appears, `herdbot history <bot> --json` shows it as `inflight`, and both disappear when the reply is logged.
- The queue file name is the bot's slug (`run::main_agent` is the identity); `history::pending_items` relies on that and says so in a comment.
