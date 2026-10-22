# Quick Entry (UI phase 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A global hotkey opens a floating panel that queues a message to a bot or room, and works with dictation tools such as MacWhisper.

**Architecture:** `status --json` and `watch --json` gain a `rooms` list (rooms' data types move from the `herdbot` crate into `herdbot-core` so the snapshot can read them). The Swift package gets a recipient model and a `send` call on the `herdbot` CLI wrapper; the app gets a Carbon hotkey, a floating `NSPanel` and a SwiftUI view. Nothing is sent until the person presses Return or ⌘Return.

**Tech Stack:** Rust (herdbot-core), Swift 6 / SwiftUI / AppKit / Carbon (`ui/menubar`), Swift Testing.

Spec: `docs/superpowers/specs/2026-10-21-ui-layer-design.md` (Phase 2).

## Global Constraints

- Branch `feature/ui-quickentry`, worktree `/Users/shane/Code/herdbot/.worktrees/ui-quickentry`. Work only there.
- Shane approved committing each task below: explicit paths only (never `git add -A` or `git add .`), conventional message, **no AI or tool attribution**, no push, no merge.
- Never run `cargo install`, never restart the daemon, never launch the menu bar app (the manual check is for a person).
- Swift: 4-space indent, no `@State` (a macro whose plugin is missing under Command Line Tools; use plain `let` or `Binding(get:set:)`), no mutable globals without `nonisolated(unsafe)`.
- Test Swift with `./scripts/test.sh` from `ui/menubar`, never bare `swift test`. Test Rust with `cargo test --workspace`; lint with `cargo clippy --workspace -- -D warnings`.
- `"v": 1` stays: the `rooms` field is additive. The Swift decoder must keep reading a snapshot that has no `rooms`.
- Baseline before this plan: Rust 57 / 14 / 1 / 2 tests pass; Swift 14 tests pass.
- Keep your ledger in `.superpowers/sdd/ui-quickentry/progress.md` (untracked; do not commit it).

All code below was compiled and its tests run in a scratch copy before this plan was written. Reproduce it exactly; if a step fails, report it, don't redesign.

---

### Task 1: rooms in the snapshot

**Files:**
- Create: `crates/herdbot-core/src/rooms.rs`
- Modify: `crates/herdbot-core/src/lib.rs`, `crates/herdbot-core/src/snapshot.rs`, `crates/herdbot-core/tests/snapshot.rs`, `crates/herdbot/src/rooms.rs`

**Interfaces:**
- Produces: `herdbot_core::rooms::{Mode, Room, Round, State, parse_room, room_dir, load_room, list_rooms, state_path, load_state}`, moved verbatim from `crates/herdbot/src/rooms.rs`; `snapshot::RoomView { slug, name, mode: rooms::Mode, paused }` and `Snapshot.rooms: Vec<RoomView>` (JSON key `rooms`, `mode` as `"channel" | "round_robin" | "moderated"`).
- `crates/herdbot/src/rooms.rs` keeps working for every caller: it re-exports the moved items, and keeps `save_room`, `save_state` and everything else.

- [ ] **Step 1: Create `crates/herdbot-core/src/rooms.rs`**

```rust
//! A room's files, as the daemon and the UIs read them: `rooms/<slug>/room.toml` and
//! `.state/rooms/<slug>.json`. Routing and the transcript live in the herdbot crate.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Members speak only when named.
    #[default]
    Channel,
    /// Each of your messages starts a round: every member once, in order.
    RoundRobin,
    /// Everything goes to the moderator; the moderator names who speaks.
    Moderated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Room {
    pub name: String,
    /// Bot slugs.
    pub members: Vec<String>,
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderator: Option<String>,
    /// Bot posts in a row, with no message from you, before the room pauses.
    pub max_turns: u32,
    /// A member is not called again this soon after its own post.
    pub member_cooldown_secs: i64,
    /// round_robin: a called member that has not posted by then is skipped.
    pub turn_timeout_secs: i64,
}

impl Default for Room {
    fn default() -> Self {
        Room {
            name: String::new(),
            members: Vec::new(),
            mode: Mode::Channel,
            moderator: None,
            max_turns: 6,
            member_cooldown_secs: 30,
            turn_timeout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Round {
    /// Index into `members` of the member whose turn it is.
    pub next: usize,
    pub called_at: i64,
}

/// Written by the daemon only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Last transcript line routed.
    pub cursor: usize,
    /// Last line delivered to each member.
    pub last_seen: BTreeMap<String, usize>,
    pub last_post: BTreeMap<String, i64>,
    /// Called but still inside their cooldown.
    pub pending: BTreeSet<String>,
    pub bot_turns: u32,
    pub paused: bool,
    pub round: Option<Round>,
    pub missing_notified: BTreeSet<String>,
}

pub fn parse_room(text: &str) -> Result<Room> {
    Ok(toml::from_str(text)?)
}

pub fn room_dir(slug: &str) -> PathBuf {
    home::root().join("rooms").join(slug)
}

pub fn load_room(slug: &str) -> Result<Room> {
    let path = room_dir(slug).join("room.toml");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("no room {slug:?} ({}): {e}", path.display()))?;
    parse_room(&text)
}

pub fn list_rooms() -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(home::root().join("rooms"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("room.toml").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

pub fn state_path(slug: &str) -> PathBuf {
    home::root().join(".state").join("rooms").join(format!("{slug}.json"))
}

pub fn load_state(slug: &str) -> State {
    home::read_json(&state_path(slug)).unwrap_or_default()
}
```

- [ ] **Step 2: Register the module in `crates/herdbot-core/src/lib.rs`**

Add `pub mod rooms;` after `pub mod home;`.

- [ ] **Step 3: Move the items out of `crates/herdbot/src/rooms.rs`**

Apply this diff (it deletes the moved definitions, re-exports them, and keeps `save_room` here; the `parse_room` import in the test module is needed because the tests call it):

```diff
--- a/crates/herdbot/src/rooms.rs
+++ b/crates/herdbot/src/rooms.rs
@@ -5,56 +5,22 @@
 use std::collections::{BTreeMap, BTreeSet};
 use std::fs;
 use std::io::Write;
-use std::path::PathBuf;
 
 use serde::{Deserialize, Serialize};
 
 use crate::home::{self, Result};
+pub use herdbot_core::rooms::{
+    Mode, Room, Round, State, list_rooms, load_room, load_state, room_dir,
+};
+use herdbot_core::rooms::state_path;
 
-#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
-#[serde(rename_all = "snake_case")]
-pub enum Mode {
-    /// Members speak only when named.
-    #[default]
-    Channel,
-    /// Each of your messages starts a round: every member once, in order.
-    RoundRobin,
-    /// Everything goes to the moderator; the moderator names who speaks.
-    Moderated,
+pub fn save_room(slug: &str, room: &Room) -> Result<()> {
+    fs::create_dir_all(room_dir(slug))?;
+    fs::write(room_dir(slug).join("room.toml"), toml::to_string_pretty(room)?)?;
+    Ok(())
 }
 
 #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
-#[serde(default)]
-pub struct Room {
-    pub name: String,
-    /// Bot slugs.
-    pub members: Vec<String>,
-    pub mode: Mode,
-    #[serde(skip_serializing_if = "Option::is_none")]
-    pub moderator: Option<String>,
-    /// Bot posts in a row, with no message from you, before the room pauses.
-    pub max_turns: u32,
-    /// A member is not called again this soon after its own post.
-    pub member_cooldown_secs: i64,
-    /// round_robin: a called member that has not posted by then is skipped.
-    pub turn_timeout_secs: i64,
-}
-
-impl Default for Room {
-    fn default() -> Self {
-        Room {
-            name: String::new(),
-            members: Vec::new(),
-            mode: Mode::Channel,
-            moderator: None,
-            max_turns: 6,
-            member_cooldown_secs: 30,
-            turn_timeout_secs: 300,
-        }
-    }
-}
-
-#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
 pub struct Entry {
     pub at: i64,
     /// "you", a bot slug, or "herdbot".
@@ -62,34 +28,6 @@
     pub text: String,
 }
 
-#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
-pub struct Round {
-    /// Index into `members` of the member whose turn it is.
-    pub next: usize,
-    pub called_at: i64,
-}
-
-/// Written by the daemon only.
-#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
-#[serde(default)]
-pub struct State {
-    /// Last transcript line routed.
-    pub cursor: usize,
-    /// Last line delivered to each member.
-    pub last_seen: BTreeMap<String, usize>,
-    pub last_post: BTreeMap<String, i64>,
-    /// Called but still inside their cooldown.
-    pub pending: BTreeSet<String>,
-    pub bot_turns: u32,
-    pub paused: bool,
-    pub round: Option<Round>,
-    pub missing_notified: BTreeSet<String>,
-}
-
-pub fn parse_room(text: &str) -> Result<Room> {
-    Ok(toml::from_str(text)?)
-}
-
 pub fn validate(room: &Room, bots: &[String]) -> std::result::Result<(), String> {
     if room.members.len() < 2 {
         return Err("a room needs at least two members".into());
@@ -102,36 +40,7 @@
     {
         return Err("a moderated room needs --moderator, one of its members".into());
     }
-    Ok(())
-}
-
-pub fn room_dir(slug: &str) -> PathBuf {
-    home::root().join("rooms").join(slug)
-}
-
-pub fn load_room(slug: &str) -> Result<Room> {
-    let path = room_dir(slug).join("room.toml");
-    let text = fs::read_to_string(&path)
-        .map_err(|e| format!("no room {slug:?} ({}): {e}", path.display()))?;
-    parse_room(&text)
-}
-
-pub fn save_room(slug: &str, room: &Room) -> Result<()> {
-    fs::create_dir_all(room_dir(slug))?;
-    fs::write(room_dir(slug).join("room.toml"), toml::to_string_pretty(room)?)?;
     Ok(())
-}
-
-pub fn list_rooms() -> Vec<String> {
-    let mut out: Vec<String> = fs::read_dir(home::root().join("rooms"))
-        .into_iter()
-        .flatten()
-        .flatten()
-        .filter(|e| e.path().join("room.toml").is_file())
-        .filter_map(|e| e.file_name().into_string().ok())
-        .collect();
-    out.sort();
-    out
 }
 
 /// One JSON line. serde_json escapes newlines in `text`, so an entry is always one line.
@@ -160,14 +69,6 @@
         .collect()
 }
 
-fn state_path(slug: &str) -> PathBuf {
-    home::root().join(".state").join("rooms").join(format!("{slug}.json"))
-}
-
-pub fn load_state(slug: &str) -> State {
-    home::read_json(&state_path(slug)).unwrap_or_default()
-}
-
 pub fn save_state(slug: &str, state: &State) -> Result<()> {
     fs::create_dir_all(state_path(slug).parent().unwrap())?;
     home::write_json(&state_path(slug), state)
@@ -410,6 +311,8 @@
 
 #[cfg(test)]
 mod tests {
+    use herdbot_core::rooms::parse_room;
+
     use super::*;
 
     fn room(mode: Mode) -> Room {
```

- [ ] **Step 4: Run the existing tests to prove the move changed nothing**

Run: `cargo build -p herdbot && cargo test --workspace`
Expected: PASS, 57 / 14 / 1 / 2 (the golden test still passes: no `rooms` yet).

- [ ] **Step 5: Add `rooms` to the snapshot and extend the golden test**

Apply both diffs. The golden test now fixtures three rooms (one round-robin, one paused, one malformed) and pins the JSON shape.

```diff
--- a/crates/herdbot-core/src/snapshot.rs
+++ b/crates/herdbot-core/src/snapshot.rs
@@ -11,6 +11,7 @@
 use crate::cooldown;
 use crate::events::{self, Event};
 use crate::home;
+use crate::rooms;
 
 /// Bumped on any breaking change to the JSON the CLI prints.
 pub const VERSION: u32 = 1;
@@ -23,6 +24,14 @@
 }
 
 #[derive(Debug, Clone, PartialEq, Serialize)]
+pub struct RoomView {
+    pub slug: String,
+    pub name: String,
+    pub mode: rooms::Mode,
+    pub paused: bool,
+}
+
+#[derive(Debug, Clone, PartialEq, Serialize)]
 pub struct RunView {
     pub id: String,
     pub bot: String,
@@ -46,6 +55,7 @@
     /// "up" when something accepts connections on the daemon's lock socket, else "down".
     pub daemon: &'static str,
     pub bots: Vec<BotView>,
+    pub rooms: Vec<RoomView>,
     /// Running subagent runs only.
     pub runs: Vec<RunView>,
     /// Harnesses still cooling down.
@@ -126,6 +136,21 @@
             }
         })
         .collect();
+    let rooms = rooms::list_rooms()
+        .into_iter()
+        .filter_map(|slug| match rooms::load_room(&slug) {
+            Ok(room) => Some(RoomView {
+                paused: rooms::load_state(&slug).paused,
+                name: room.name,
+                mode: room.mode,
+                slug,
+            }),
+            Err(e) => {
+                warnings.push(format!("room {slug}: {e}"));
+                None
+            }
+        })
+        .collect();
     let mut runs: Vec<RunView> = home::list_runs()
         .into_iter()
         .filter(|run| run.status == "running")
@@ -148,6 +173,7 @@
         at: home::now(),
         daemon: if UnixStream::connect(home::daemon_lock()).is_ok() { "up" } else { "down" },
         bots,
+        rooms,
         runs,
         cooldowns: cooldowns(),
         blocked,
```

```diff
--- a/crates/herdbot-core/tests/snapshot.rs
+++ b/crates/herdbot-core/tests/snapshot.rs
@@ -36,6 +36,10 @@
             r#"{{"id":"{id}","bot":"alpha","parent_agent":"alpha-main","agent":"alpha-{id}","pane":"p2","class":"code","route":"claude:sonnet","description":"do it","status":"{status}","started_at":1}}"#
         )
     };
+    write(&root.join("rooms/crew/room.toml"), "name = \"Crew\"\nmembers = [\"alpha\", \"beta\"]\nmode = \"round_robin\"\n");
+    write(&root.join("rooms/quiet/room.toml"), "name = \"Quiet\"\nmembers = [\"alpha\", \"beta\"]\n");
+    write(&root.join(".state/rooms/quiet.json"), r#"{"paused":true}"#);
+    write(&root.join("rooms/torn/room.toml"), "mode = \"chaos\"");
     write(&root.join(".runs/r1.json"), &run("r1", "running"));
     write(&root.join(".runs/r2.json"), &run("r2", "done"));
     write(&root.join(".state/cooldown/claude.json"), r#"{"until":4102444800,"reason":"limit"}"#);
@@ -53,6 +57,9 @@
     assert_eq!(snap.daemon, "down");
     let bots: Vec<_> = snap.bots.iter().map(|b| (b.slug.as_str(), b.up)).collect();
     assert_eq!(bots, vec![("alpha", true), ("beta", false)]);
+    let rooms: Vec<_> = snap.rooms.iter().map(|r| (r.slug.as_str(), r.name.as_str(), r.paused)).collect();
+    assert_eq!(rooms, vec![("crew", "Crew", false), ("quiet", "Quiet", true)]);
+    assert!(snap.warnings.iter().any(|w| w.starts_with("room torn:")));
     assert_eq!(snap.runs.len(), 1);
     assert_eq!(snap.runs[0].id, "r1");
     assert_eq!(snap.cooldowns.len(), 1);
@@ -67,8 +74,10 @@
     let json = serde_json::to_value(&snap).unwrap();
     let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
     keys.sort();
-    assert_eq!(keys, ["at", "blocked", "bots", "cooldowns", "daemon", "runs", "v", "warnings"]);
+    assert_eq!(keys, ["at", "blocked", "bots", "cooldowns", "daemon", "rooms", "runs", "v", "warnings"]);
 
+    assert_eq!(json["rooms"][0], serde_json::json!({"slug":"crew","name":"Crew","mode":"round_robin","paused":false}));
+
     // Watcher: first poll is the snapshot only, then quiet, then changes.
     let mut watcher = snapshot::Watcher::default();
     let first = watcher.poll();
```

- [ ] **Step 6: Verify**

Run: `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: all pass, counts unchanged (57 / 14 / 1 / 2; the golden test gained assertions, not tests), clippy clean.

- [ ] **Step 7: Commit**

```bash
git add crates/herdbot-core/src/rooms.rs crates/herdbot-core/src/lib.rs crates/herdbot-core/src/snapshot.rs crates/herdbot-core/tests/snapshot.rs crates/herdbot/src/rooms.rs
git commit -m "feat: list rooms in status and watch"
```

---

### Task 2: Swift recipient model and `send`

**Files:**
- Create: `ui/menubar/Sources/HerdbotKit/QuickEntry.swift`, `ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift`
- Modify: `ui/menubar/Sources/HerdbotKit/Models.swift`, `ui/menubar/Sources/HerdbotKit/Stream.swift`, `ui/menubar/Sources/HerdbotKit/ProcessCLI.swift`, `ui/menubar/Tests/HerdbotKitTests/Fixtures.swift`, `ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift`, `ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift`

**Interfaces:**
- Consumes: the `rooms` JSON from Task 1 (but the Swift side must also decode a snapshot without it).
- Produces: `Recipient` (`.bot(slug)`, `.room(slug)`), `RecipientOption`, `QuickEntryState` (`@MainActor @Observable`: `text`, `status`, `options`, `recipient`, `daemonDown`, `canSend`, `choose(_:)`, `reset()`, `send(using:) async -> Bool`), and `HerdbotCLI.send(to:text:)`.

- [ ] **Step 1: Write the failing tests first**

Create `ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

@MainActor
@Suite struct QuickEntry {
    func make(daemon: String = "up", rooms: Bool = true) -> (QuickEntryState, MenuState, UserDefaults) {
        let menu = MenuState()
        menu.apply(line(snapshot(
            daemon: daemon,
            bots: [BotView(slug: "alpha", title: "Alpha", up: true), BotView(slug: "beta", title: "", up: false)],
            rooms: rooms ? [RoomView(slug: "crew", name: "Crew", mode: "channel", paused: false)] : [])))
        let defaults = UserDefaults(suiteName: "qe-\(UUID().uuidString)")!
        return (QuickEntryState(menu: menu, defaults: defaults), menu, defaults)
    }

    @Test func optionsListBotsThenRooms() {
        let (state, _, _) = make()
        #expect(state.options.map(\.label) == ["Alpha", "beta", "Crew (room)"])
        #expect(state.recipient == .bot("alpha"))
    }

    @Test func remembersTheLastRecipientAcrossResets() {
        let (state, menu, defaults) = make()
        state.choose(.room("crew"))
        state.reset()
        #expect(state.recipient == .room("crew"))
        // A new state over the same defaults, as after an app restart.
        #expect(QuickEntryState(menu: menu, defaults: defaults).recipient == .room("crew"))
    }

    @Test func aRemovedRecipientFallsBackToTheFirstOption() {
        let (state, menu, _) = make()
        state.choose(.room("crew"))
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "Alpha", up: true)])))
        #expect(state.recipient == .bot("alpha"))
    }

    @Test func noSnapshotMeansNothingToSend() {
        let state = QuickEntryState(menu: MenuState(), defaults: UserDefaults(suiteName: "qe-\(UUID().uuidString)")!)
        state.text = "hi"
        #expect(state.options.isEmpty)
        #expect(!state.canSend)
    }

    @Test func blankTextCannotBeSent() {
        let (state, _, _) = make()
        state.text = "  \n "
        #expect(!state.canSend)
        state.text = "hi"
        #expect(state.canSend)
    }

    @Test func sendTrimsQueuesAndClears() async {
        let (state, _, _) = make()
        let cli = FakeCLI()
        state.choose(.room("crew"))
        state.text = "  line one\nline two \n"
        #expect(await state.send(using: cli))
        #expect(cli.sent.count == 1)
        #expect(cli.sent[0].0 == .room("crew"))
        #expect(cli.sent[0].1 == "line one\nline two")
        #expect(state.status == .queued)
        #expect(state.text.isEmpty)
    }

    @Test func aFailedSendKeepsTheTextAndSaysWhy() async {
        let (state, _, _) = make()
        let cli = FakeCLI()
        cli.failure = "no bot alpha"
        state.text = "hello"
        #expect(!(await state.send(using: cli)))
        #expect(state.status == .failed("no bot alpha"))
        #expect(state.text == "hello")
        #expect(state.canSend)
    }

    @Test func daemonDownIsSurfacedButDoesNotBlockSending() async {
        let (state, _, _) = make(daemon: "down")
        #expect(state.daemonDown)
        state.text = "hi"
        #expect(await state.send(using: FakeCLI()))
    }

    @Test func resetStartsAFreshMessage() async {
        let (state, _, _) = make()
        state.text = "x"
        _ = await state.send(using: FakeCLI())
        state.reset()
        #expect(state.status == .idle)
        #expect(state.text.isEmpty)
    }
}
```

Replace `ui/menubar/Tests/HerdbotKitTests/Fixtures.swift` with (adds `rooms`/`bots` parameters to `snapshot(...)`, a rooms entry in `snapshotLine`, and a `FakeCLI`):

```swift
import Foundation
import Testing
@testable import HerdbotKit

let snapshotLine = """
{"v":1,"type":"snapshot","snapshot":{"v":1,"at":100,"daemon":"up",\
"bots":[{"slug":"alpha","title":"Alpha","up":true}],\
"rooms":[{"slug":"crew","name":"Crew","mode":"round_robin","paused":false}],\
"runs":[{"id":"r1","bot":"alpha","agent":"alpha-r1","route":"claude:sonnet","description":"do it"}],\
"cooldowns":[{"harness":"claude","until":4102444800,"reason":"limit"}],\
"blocked":[{"at":10,"agent":"alpha-main","kind":"blocked","reason":"approval: Bash","harness":"claude"}],\
"warnings":["x"]}}
"""

let eventLine = """
{"v":1,"type":"event","event":{"at":20,"agent":"alpha-main","kind":"turn_end","text":"done","harness":"claude"}}
"""

func snapshot(daemon: String = "up", runs: Int = 0, blocked: [Event] = [], bots: [BotView] = [], rooms: [RoomView] = []) -> Snapshot {
    Snapshot(
        v: 1, at: 1, daemon: daemon, bots: bots, rooms: rooms,
        runs: (0..<runs).map { RunView(id: "r\($0)", bot: "a", agent: "a-r\($0)", route: "x", description: "d") },
        cooldowns: [], blocked: blocked, warnings: [])
}

func blockedEvent(_ agent: String, at: Int = 1) -> Event {
    Event(at: at, agent: agent, kind: "blocked", text: nil, reason: "approval", harness: "claude")
}

func line(_ snapshot: Snapshot) -> StreamLine {
    StreamLine(v: 1, type: "snapshot", snapshot: snapshot, event: nil)
}

/// A fake `herdbot` in a temp dir: the script body decides what it prints.
func fakeHerdbot(_ body: String) throws -> URL {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("hk-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let script = dir.appendingPathComponent("herdbot")
    try "#!/bin/sh\n\(body)\n".write(to: script, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
    return script
}

/// Records what quick entry sends; `failure` makes the next send throw.
final class FakeCLI: HerdbotCLI, @unchecked Sendable {
    nonisolated(unsafe) var sent: [(Recipient, String)] = []
    nonisolated(unsafe) var failure: String?

    func watch() -> AsyncThrowingStream<StreamLine, Error> { AsyncThrowingStream { $0.finish() } }
    func focus(agent: String) async throws {}
    func send(to recipient: Recipient, text: String) async throws {
        if let failure { throw CLIError.failed(failure) }
        sent.append((recipient, text))
    }
}
```

Replace `ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

@Suite struct Decoding {
    @Test func snapshotLineDecodes() throws {
        let parsed = try #require(try StreamParser.parse(snapshotLine))
        #expect(parsed.type == "snapshot")
        let snap = try #require(parsed.snapshot)
        #expect(snap.daemon == "up")
        #expect(snap.bots.map(\.slug) == ["alpha"])
        #expect(snap.rooms.map(\.slug) == ["crew"])
        #expect(snap.rooms.first?.mode == "round_robin")
        #expect(snap.runs.first?.agent == "alpha-r1")
        #expect(snap.cooldowns.first?.until == 4102444800)
        #expect(snap.blocked.first?.reason == "approval: Bash")
        #expect(snap.blocked.first?.text == nil)
    }

    @Test func aSnapshotFromAnOlderHerdbotHasNoRooms() throws {
        let old = """
        {"v":1,"type":"snapshot","snapshot":{"v":1,"at":1,"daemon":"up","bots":[],"runs":[],"cooldowns":[],"blocked":[],"warnings":[]}}
        """
        #expect(try #require(try StreamParser.parse(old)?.snapshot).rooms.isEmpty)
    }

    @Test func eventLineDecodes() throws {
        let parsed = try #require(try StreamParser.parse(eventLine))
        #expect(parsed.event?.kind == "turn_end")
        #expect(parsed.event?.text == "done")
    }

    @Test func unknownVersionThrows() {
        #expect(throws: CLIError.unsupported(2)) { try StreamParser.parse(#"{"v":2,"type":"snapshot"}"#) }
    }

    @Test func garbageIsSkipped() throws {
        #expect(try StreamParser.parse("not json") == nil)
        #expect(try StreamParser.parse(#"{"hello":1}"#) == nil)
    }
}
```

Replace `ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

@Suite struct ProcessCLITests {
    @Test func watchStreamsLinesThenEnds() async throws {
        let script = try fakeHerdbot("echo '\(snapshotLine)'\necho 'junk'\necho '\(eventLine)'")
        var types: [String] = []
        do {
            for try await parsed in ProcessCLI(binary: script).watch() { types.append(parsed.type) }
            Issue.record("stream should end with an error")
        } catch {
            #expect(error as? CLIError == .failed("herdbot watch ended"))
        }
        #expect(types == ["snapshot", "event"])
    }

    @Test func watchReportsAnUnknownVersion() async throws {
        let script = try fakeHerdbot(#"echo '{"v":9,"type":"snapshot"}'"#)
        do {
            for try await _ in ProcessCLI(binary: script).watch() {}
            Issue.record("expected an error")
        } catch {
            #expect(error as? CLIError == .unsupported(9))
        }
    }

    @Test func focusPassesTheAgentAndReportsFailure() async throws {
        let ok = try fakeHerdbot(#"[ "$1 $2" = "focus a-1" ]"#)
        try await ProcessCLI(binary: ok).focus(agent: "a-1")
        let bad = try fakeHerdbot("echo 'herdr agent_not_found' >&2; exit 1")
        await #expect(throws: CLIError.failed("herdr agent_not_found")) {
            try await ProcessCLI(binary: bad).focus(agent: "nope")
        }
    }

    @Test func sendPassesTheTextAsOneArgument() async throws {
        let bot = try fakeHerdbot(##"[ "$#" = 3 ] && [ "$1" = send ] && [ "$2" = alpha ] && [ "$3" = "$(printf 'two words "quoted"\nsecond line')" ]"##)
        try await ProcessCLI(binary: bot).send(to: .bot("alpha"), text: "two words \"quoted\"\nsecond line")
        let room = try fakeHerdbot(#"[ "$#" = 4 ] && [ "$1 $2 $3 $4" = "room send crew hi there" ]"#)
        try await ProcessCLI(binary: room).send(to: .room("crew"), text: "hi there")
        let bad = try fakeHerdbot("echo 'no bot alpha' >&2; exit 1")
        await #expect(throws: CLIError.failed("no bot alpha")) {
            try await ProcessCLI(binary: bad).send(to: .bot("alpha"), text: "x")
        }
    }

    @Test func locatePrefersHerdbotBin() throws {
        let script = try fakeHerdbot("true")
        #expect(ProcessCLI.locate(environment: ["HERDBOT_BIN": script.path], home: "/nonexistent") == script)
        #expect(ProcessCLI.locate(environment: [:], home: "/nonexistent") == nil)
    }

    /// Only when HERDBOT_BIN points at a real, new-enough herdbot: the live contract check.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["HERDBOT_BIN"] != nil))
    func realBinaryStreamsASnapshotFirst() async throws {
        let binary = try #require(ProcessCLI.locate())
        var first: StreamLine?
        for try await parsed in ProcessCLI(binary: binary).watch() {
            first = parsed
            break
        }
        #expect(first?.type == "snapshot")
        #expect(first?.snapshot?.v == supportedVersion)
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run (from `ui/menubar`): `./scripts/test.sh`
Expected: build FAIL (`Recipient`, `QuickEntryState`, `RoomView`, `send` do not exist).

- [ ] **Step 3: Implement**

Replace `ui/menubar/Sources/HerdbotKit/Models.swift` (adds `RoomView`, `Snapshot.rooms`, and a decoder that reads a missing `rooms` as `[]`):

```swift
import Foundation

/// The JSON `herdbot status --json` and `herdbot watch --json` print. Mirrors
/// crates/herdbot-core/src/snapshot.rs; bump `supportedVersion` with `snapshot::VERSION`.
public let supportedVersion = 1

public struct Event: Codable, Hashable, Sendable {
    public let at: Int
    public let agent: String
    public let kind: String
    public let text: String?
    public let reason: String?
    public let harness: String
}

public struct BotView: Codable, Hashable, Sendable {
    public let slug: String
    public let title: String
    public let up: Bool
}

public struct RoomView: Codable, Hashable, Sendable {
    public let slug: String
    public let name: String
    /// "channel", "round_robin" or "moderated".
    public let mode: String
    public let paused: Bool
}

public struct RunView: Codable, Hashable, Sendable {
    public let id: String
    public let bot: String
    public let agent: String
    public let route: String
    public let description: String
}

public struct CooldownView: Codable, Hashable, Sendable {
    public let harness: String
    /// Unix seconds.
    public let until: Int
    public let reason: String
}

public struct Snapshot: Codable, Hashable, Sendable {
    public let v: Int
    public let at: Int
    /// "up" or "down".
    public let daemon: String
    public let bots: [BotView]
    public let rooms: [RoomView]
    public let runs: [RunView]
    public let cooldowns: [CooldownView]
    public let blocked: [Event]
    public let warnings: [String]
}

/// One line of `herdbot watch --json`: `type` is "snapshot" or "event".
public struct StreamLine: Codable, Hashable, Sendable {
    public let v: Int
    public let type: String
    public let snapshot: Snapshot?
    public let event: Event?
}

extension Snapshot {
    /// A herdbot older than rooms-in-status sends no `rooms`; read that as none.
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            v: try c.decode(Int.self, forKey: .v),
            at: try c.decode(Int.self, forKey: .at),
            daemon: try c.decode(String.self, forKey: .daemon),
            bots: try c.decode([BotView].self, forKey: .bots),
            rooms: try c.decodeIfPresent([RoomView].self, forKey: .rooms) ?? [],
            runs: try c.decode([RunView].self, forKey: .runs),
            cooldowns: try c.decode([CooldownView].self, forKey: .cooldowns),
            blocked: try c.decode([Event].self, forKey: .blocked),
            warnings: try c.decode([String].self, forKey: .warnings))
    }
}
```

In `ui/menubar/Sources/HerdbotKit/Stream.swift` add one requirement to `HerdbotCLI`:

```diff
--- a/ui/menubar/Sources/HerdbotKit/Stream.swift
+++ b/ui/menubar/Sources/HerdbotKit/Stream.swift
@@ -20,6 +20,8 @@
     func watch() -> AsyncThrowingStream<StreamLine, Error>
     /// `herdbot focus <agent>`.
     func focus(agent: String) async throws
+    /// `herdbot send <bot> <text>` or `herdbot room send <room> <text>`: queues the text.
+    func send(to recipient: Recipient, text: String) async throws
 }
 
 public enum StreamParser {
```

Replace `ui/menubar/Sources/HerdbotKit/ProcessCLI.swift` (`focus` and the new `send` share one private `run`; the text is a single argv entry, so quotes and newlines survive):

```swift
import Foundation

public struct ProcessCLI: HerdbotCLI {
    public let binary: URL
    public let environment: [String: String]

    public init(binary: URL, environment: [String: String] = ProcessInfo.processInfo.environment) {
        self.binary = binary
        self.environment = environment
    }

    /// GUI apps get a bare PATH, so look in the usual install spots. `HERDBOT_BIN` wins.
    public static func locate(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        home: String = NSHomeDirectory()
    ) -> URL? {
        let candidates = [environment["HERDBOT_BIN"], home + "/.cargo/bin/herdbot", "/opt/homebrew/bin/herdbot", "/usr/local/bin/herdbot"]
        return candidates
            .compactMap { $0 }
            .map { URL(fileURLWithPath: $0) }
            .first { FileManager.default.isExecutableFile(atPath: $0.path) }
    }

    public func watch() -> AsyncThrowingStream<StreamLine, Error> {
        AsyncThrowingStream { continuation in
            let process = Process()
            let pipe = Pipe()
            process.executableURL = binary
            process.arguments = ["watch", "--json"]
            process.environment = environment
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice
            do {
                try process.run()
            } catch {
                continuation.finish(throwing: CLIError.failed("could not start herdbot: \(error.localizedDescription)"))
                return
            }
            let reader = Task {
                do {
                    for try await line in pipe.fileHandleForReading.bytes.lines {
                        if let parsed = try StreamParser.parse(line) { continuation.yield(parsed) }
                    }
                    continuation.finish(throwing: CLIError.failed("herdbot watch ended"))
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in
                reader.cancel()
                if process.isRunning { process.terminate() }
            }
        }
    }

    public func focus(agent: String) async throws {
        try await run(["focus", agent])
    }

    /// The text is one argv entry, so spaces, quotes and newlines reach herdbot untouched.
    public func send(to recipient: Recipient, text: String) async throws {
        switch recipient {
        case .bot(let slug): try await run(["send", slug, text])
        case .room(let slug): try await run(["room", "send", slug, text])
        }
    }

    /// Run `herdbot <arguments>` to completion; a non-zero exit throws its stderr.
    private func run(_ arguments: [String]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let process = Process()
            let errors = Pipe()
            process.executableURL = binary
            process.arguments = arguments
            process.environment = environment
            process.standardOutput = FileHandle.nullDevice
            process.standardError = errors
            process.terminationHandler = { finished in
                if finished.terminationStatus == 0 {
                    continuation.resume()
                } else {
                    let text = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                    continuation.resume(throwing: CLIError.failed(text.trimmingCharacters(in: .whitespacesAndNewlines)))
                }
            }
            do {
                try process.run()
            } catch {
                continuation.resume(throwing: CLIError.failed("could not start herdbot: \(error.localizedDescription)"))
            }
        }
    }
}
```

Create `ui/menubar/Sources/HerdbotKit/QuickEntry.swift`:

```swift
import Foundation
import Observation

/// Where a quick-entry message goes.
public enum Recipient: Hashable, Sendable {
    case bot(String)
    case room(String)

    /// Stable text form, for remembering the last choice.
    var key: String {
        switch self {
        case .bot(let slug): "bot:\(slug)"
        case .room(let slug): "room:\(slug)"
        }
    }

    init?(key: String) {
        if key.hasPrefix("bot:") { self = .bot(String(key.dropFirst(4))) }
        else if key.hasPrefix("room:") { self = .room(String(key.dropFirst(5))) }
        else { return nil }
    }
}

public struct RecipientOption: Hashable, Identifiable, Sendable {
    public let recipient: Recipient
    public let label: String
    public var id: Recipient { recipient }
}

@MainActor @Observable
public final class QuickEntryState {
    public enum Status: Equatable, Sendable {
        case idle
        case sending
        case queued
        case failed(String)
    }

    public var text = ""
    public private(set) var status: Status = .idle
    private var chosen: Recipient?
    @ObservationIgnored private let menu: MenuState
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private static let lastKey = "quickEntry.lastRecipient"

    public init(menu: MenuState, defaults: UserDefaults = .standard) {
        self.menu = menu
        self.defaults = defaults
    }

    /// Bots first, then rooms.
    public var options: [RecipientOption] {
        guard let snapshot = menu.snapshot else { return [] }
        let bots = snapshot.bots.map {
            RecipientOption(recipient: .bot($0.slug), label: $0.title.isEmpty ? $0.slug : $0.title)
        }
        let rooms = snapshot.rooms.map {
            RecipientOption(recipient: .room($0.slug), label: "\($0.name.isEmpty ? $0.slug : $0.name) (room)")
        }
        return bots + rooms
    }

    /// This session's pick, else the last one used, else the first option.
    public var recipient: Recipient? {
        let available = Set(options.map(\.recipient))
        let remembered = defaults.string(forKey: Self.lastKey).flatMap(Recipient.init(key:))
        return [chosen, remembered].compactMap { $0 }.first(where: available.contains)
            ?? options.first?.recipient
    }

    public var daemonDown: Bool { menu.snapshot?.daemon == "down" }

    public var canSend: Bool {
        recipient != nil && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && status != .sending
    }

    public func choose(_ recipient: Recipient) {
        chosen = recipient
        defaults.set(recipient.key, forKey: Self.lastKey)
    }

    /// Start a fresh message. The recipient choice carries over.
    public func reset() {
        text = ""
        status = .idle
        chosen = nil
    }

    /// True when the message was queued. Dictated text is never sent on its own: this runs only
    /// when the person submits.
    @discardableResult
    public func send(using cli: any HerdbotCLI) async -> Bool {
        guard canSend, let recipient else { return false }
        let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
        status = .sending
        do {
            try await cli.send(to: recipient, text: message)
            text = ""
            status = .queued
            return true
        } catch {
            status = .failed(error.localizedDescription)
            return false
        }
    }
}
```

- [ ] **Step 4: Run to see it pass**

Run: `./scripts/test.sh`
Expected: PASS, 25 tests (the 14 existing plus 11 new).

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotKit/QuickEntry.swift ui/menubar/Sources/HerdbotKit/Models.swift ui/menubar/Sources/HerdbotKit/Stream.swift ui/menubar/Sources/HerdbotKit/ProcessCLI.swift ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift ui/menubar/Tests/HerdbotKitTests/Fixtures.swift ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift
git commit -m "feat(menubar): recipient model and send for quick entry"
```

---

### Task 3: hotkey, panel and wiring

No unit tests: this is AppKit and Carbon glue, and the hotkey, key focus and dictation can only be checked by a person (Task 4's checklist). The gate is a clean build.

**Files:**
- Create: `ui/menubar/Sources/HerdbotMenu/Hotkey.swift`, `ui/menubar/Sources/HerdbotMenu/QuickEntryView.swift`, `ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/App.swift`, `ui/menubar/Sources/HerdbotMenu/MenuContent.swift`

**Interfaces:**
- Consumes: `QuickEntryState`, `Recipient`, `ProcessCLI` from Task 2.
- Produces: ⌥Space toggles the panel; a "Quick entry" menu item opens it; the menu says so when the hotkey could not be registered.

Design notes that the code encodes (do not change them without asking):
- ⌥Space via Carbon `RegisterEventHotKey`: needs no Accessibility permission, and it is not a modifier alone, so it can't clash with MacWhisper's Right ⌘ dictation key.
- The panel activates the app and becomes key (`NSApp.activate` + `canBecomeKey`), so dictated text lands in the panel's field. A non-activating panel would leave the previous app frontmost.
- `hidesOnDeactivate = false` and no close-on-resign: a dictation HUD that briefly takes focus must not dismiss the panel. Only Esc, Cancel or a send closes it.
- A fresh `NSHostingView` on every show, so the field takes focus every time.

- [ ] **Step 1: Create `ui/menubar/Sources/HerdbotMenu/Hotkey.swift`**

```swift
import Carbon.HIToolbox

/// One system-wide hotkey via Carbon. It needs no Accessibility permission and never sees other
/// keys, so it cannot interfere with dictation tools that watch a modifier (MacWhisper's Right ⌘).
@MainActor
final class GlobalHotkey {
    private static var action: (@MainActor () -> Void)?
    private var reference: EventHotKeyRef?

    /// False when another app already owns the combination.
    func register(keyCode: Int, modifiers: Int, action: @escaping @MainActor () -> Void) -> Bool {
        Self.action = action
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, _ in
            // Carbon delivers hotkey events on the main thread.
            MainActor.assumeIsolated { GlobalHotkey.action?() }
            return noErr
        }, 1, &spec, nil, nil)
        let id = EventHotKeyID(signature: OSType(0x4842_4B59), id: 1)  // 'HBKY'
        let status = RegisterEventHotKey(UInt32(keyCode), UInt32(modifiers), id, GetApplicationEventTarget(), 0, &reference)
        return status == noErr
    }
}
```

- [ ] **Step 2: Create `ui/menubar/Sources/HerdbotMenu/QuickEntryView.swift`**

```swift
import HerdbotKit
import SwiftUI

/// Recipient picker, a text field that dictation tools type or paste into, and Send.
/// Return sends; ⌘Return does too. Nothing is sent until you submit.
struct QuickEntryView: View {
    let state: QuickEntryState
    let cli: ProcessCLI?
    let close: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Picker("To", selection: Binding(get: { state.recipient }, set: { if let r = $0 { state.choose(r) } })) {
                ForEach(state.options) { option in
                    Text(option.label).tag(Optional(option.recipient))
                }
            }
            TextField("Message", text: Binding(get: { state.text }, set: { state.text = $0 }), axis: .vertical)
                .lineLimit(1...8)
                .focused($focused)
                .onSubmit(submit)
            HStack {
                status
                Spacer()
                Button("Cancel", action: close).keyboardShortcut(.cancelAction)
                Button("Send", action: submit)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(!state.canSend)
            }
        }
        .padding(16)
        .frame(width: 480)
        .onAppear { focused = true }
    }

    @ViewBuilder private var status: some View {
        switch state.status {
        case .queued: Text("queued").foregroundStyle(.green)
        case .sending: Text("sending…").foregroundStyle(.secondary)
        case .failed(let message): Text(message).foregroundStyle(.red).lineLimit(2)
        case .idle:
            if cli == nil {
                Text("herdbot not found").foregroundStyle(.red)
            } else if state.daemonDown {
                Text("daemon not running; messages won't deliver until it is").foregroundStyle(.orange)
            }
        }
    }

    private func submit() {
        guard let cli else { return }
        Task {
            if await state.send(using: cli) {
                try? await Task.sleep(for: .milliseconds(600))
                close()
            }
        }
    }
}
```

- [ ] **Step 3: Create `ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift`**

```swift
import AppKit
import HerdbotKit
import SwiftUI

/// Escape closes it; a plain NSPanel refuses key status without this subclass.
private final class QuickEntryPanel: NSPanel {
    var onCancel: () -> Void = {}
    override var canBecomeKey: Bool { true }
    override func cancelOperation(_ sender: Any?) { onCancel() }
}

@MainActor
final class QuickEntryController {
    private let state: QuickEntryState
    private let cli: ProcessCLI?
    private let panel: QuickEntryPanel

    init(state: QuickEntryState, cli: ProcessCLI?) {
        self.state = state
        self.cli = cli
        panel = QuickEntryPanel(
            contentRect: NSRect(x: 0, y: 0, width: 480, height: 120),
            styleMask: [.titled, .fullSizeContentView],
            backing: .buffered,
            defer: false)
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        // Stay open if a dictation HUD takes focus for a moment: only Escape, Cancel or a send closes it.
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.onCancel = { [weak self] in self?.hide() }
    }

    func toggle() {
        panel.isVisible ? hide() : show()
    }

    func show() {
        state.reset()
        // A fresh view each time, so the text field takes focus on every open.
        let view = NSHostingView(rootView: QuickEntryView(state: state, cli: cli, close: { [weak self] in self?.hide() }))
        panel.contentView = view
        panel.setContentSize(view.fittingSize)
        panel.center()
        // The app must be active for the panel to become key: dictation tools type into the active app.
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
    }

    func hide() {
        panel.orderOut(nil)
    }
}
```

- [ ] **Step 4: Replace `App.swift` and `MenuContent.swift`**

`ui/menubar/Sources/HerdbotMenu/App.swift`:

```swift
import AppKit
import Carbon.HIToolbox
import HerdbotKit
import SwiftUI

@main
struct HerdbotMenuApp: App {
    // A plain `let`: the App struct is built once, and @State is a macro whose plugin needs full Xcode.
    private let state: MenuState
    private let cli: ProcessCLI?
    private let notifier: Notifier
    private let quickEntry: QuickEntryController
    private let hotkeyRegistered: Bool

    init() {
        let state = MenuState()
        let cli = ProcessCLI.locate().map { ProcessCLI(binary: $0) }
        let notifier = Notifier(cli: cli)
        state.onBlocked = { notifier.post($0) }
        if let cli {
            Task { await state.follow(cli) }
        } else {
            state.fail(CLIError.notFound.localizedDescription)
        }
        let quickEntry = QuickEntryController(state: QuickEntryState(menu: state), cli: cli)
        // ⌥Space. Not a modifier alone, so it can't clash with MacWhisper's Right ⌘ dictation key.
        hotkeyRegistered = GlobalHotkey().register(keyCode: kVK_Space, modifiers: optionKey) { quickEntry.toggle() }
        self.state = state
        self.cli = cli
        self.notifier = notifier
        self.quickEntry = quickEntry
    }

    var body: some Scene {
        MenuBarExtra {
            MenuContent(state: state, cli: cli, hotkeyRegistered: hotkeyRegistered, openQuickEntry: { quickEntry.show() })
        } label: {
            MenuIcon(icon: state.icon)
        }
    }
}
```

`ui/menubar/Sources/HerdbotMenu/MenuContent.swift`:

```swift
import AppKit
import HerdbotKit
import SwiftUI

struct MenuContent: View {
    let state: MenuState
    let cli: ProcessCLI?
    let hotkeyRegistered: Bool
    let openQuickEntry: () -> Void

    var body: some View {
        if let problem = state.problem {
            Text(problem)
        } else if let snapshot = state.snapshot {
            if snapshot.daemon == "down" {
                Text("daemon not running; messages won't deliver")
            }
            if !snapshot.blocked.isEmpty {
                Section("Waiting on you") {
                    ForEach(snapshot.blocked, id: \.self) { event in
                        Button("\(event.agent): \(event.reason ?? "waiting")") { focus(event.agent) }
                    }
                }
            }
            Section("Bots") {
                ForEach(snapshot.bots, id: \.slug) { bot in
                    Text("\(bot.up ? "●" : "○") \(bot.title.isEmpty ? bot.slug : bot.title)")
                }
            }
            if !snapshot.runs.isEmpty {
                Section("Running") {
                    ForEach(snapshot.runs, id: \.id) { run in
                        Button("\(run.bot): \(run.description) (\(run.route))") { focus(run.agent) }
                    }
                }
            }
            if !snapshot.cooldowns.isEmpty {
                Section("Cooling down") {
                    ForEach(snapshot.cooldowns, id: \.harness) { cooldown in
                        Text("\(cooldown.harness) until \(until(cooldown.until)): \(cooldown.reason)")
                    }
                }
            }
            if !snapshot.warnings.isEmpty {
                Text("\(snapshot.warnings.count) warning(s): \(snapshot.warnings[0])")
            }
        } else {
            Text("waiting for herdbot…")
        }
        Divider()
        Button(hotkeyRegistered ? "Quick entry (⌥Space)" : "Quick entry (⌥Space is taken by another app)") { openQuickEntry() }
        Button("Quit") { NSApplication.shared.terminate(nil) }
    }

    private func focus(_ agent: String) {
        Task { try? await cli?.focus(agent: agent) }
    }

    private func until(_ seconds: Int) -> String {
        Date(timeIntervalSince1970: TimeInterval(seconds)).formatted(date: .omitted, time: .shortened)
    }
}
```

- [ ] **Step 5: Verify**

Run (from `ui/menubar`): `swift build 2>&1 | grep -E "error|warning: [^s]|Build"` and `./scripts/test.sh` and `./scripts/build-app.sh`
Expected: `Build complete!`, no Swift errors or warnings (linker `search path ... not found` warnings are an environment quirk and fine), 25 tests pass, `built build/HerdbotMenu.app`. Do not launch it.

- [ ] **Step 6: Commit**

```bash
git add ui/menubar/Sources/HerdbotMenu/Hotkey.swift ui/menubar/Sources/HerdbotMenu/QuickEntryView.swift ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift ui/menubar/Sources/HerdbotMenu/App.swift ui/menubar/Sources/HerdbotMenu/MenuContent.swift
git commit -m "feat(menubar): quick-entry panel on a global hotkey"
```

---

### Task 4: docs and the manual check

**Files:**
- Modify: `ui/menubar/README.md`, `README.md`, `docs/superpowers/plans/README.md`, `docs/superpowers/specs/2026-10-21-ui-layer-design.md`

- [ ] **Step 1: `ui/menubar/README.md`**

In the opening paragraph, after "...runs `herdbot focus <agent>`.", add: "⌥Space opens a quick-entry panel that queues a message to a bot or room."

In "Needs macOS 14+…", replace "a herdbot that has `watch --json` and `focus`" with "a herdbot that has `watch --json`, `focus` and (for rooms in the picker) `rooms` in `status --json`".

Append this section at the end of the file:

````markdown
## Quick entry

⌥Space (or the menu's "Quick entry") opens a panel: pick a bot or room, type or dictate, press Return
(or ⌘Return). It runs `herdbot send <bot> <text>` or `herdbot room send <room> <text>`, shows "queued"
and closes. Esc cancels. The last recipient is remembered. If the daemon is down the panel warns that
nothing is delivered until it is up. Nothing is ever sent without a keypress, so you can read a
dictated message before it goes.

Dictation (MacWhisper, hold Right ⌘): the hotkey is not a bare modifier, so it can't collide with the
dictation key; the panel activates the app and takes key focus, so dictated text lands in its field; and the
panel stays open if a dictation HUD takes focus for a moment.

### Manual check (needs a person; MacWhisper for steps 6-7)

1. `cargo install --path crates/herdbot`, restart the daemon, and quit any running HerdbotMenu. Then
   `./scripts/build-app.sh`.
2. Throwaway state with two bots and a room:
   `export HB=$(mktemp -d); for b in scratch other; do HERDBOT_HOME=$HB herdbot new $b; done; HERDBOT_HOME=$HB herdbot room new crew --members scratch,other`
3. `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`. Press ⌥Space. Expect a
   floating panel with the text field focused, a picker listing `scratch`, `other` and `Crew (room)`, and a
   "daemon not running" warning. If the menu says "⌥Space is taken by another app", free the key first.
4. Type `hello "quoted" world`, press Return. Expect "queued" and the panel closes. Check
   `cat $HB/.queue/scratch.jsonl` shows the text.
5. Open it again: the picker still shows `scratch`. Choose `Crew (room)`, type `to the room`, press ⌘Return.
   Check `tail -1 $HB/rooms/crew/transcript.jsonl` shows it from `you`. Press ⌥Space, then Esc: it closes and sends nothing.
6. MacWhisper: open the panel, hold Right ⌘ and speak. Expect the text to appear in the field and nothing
   to send. Release; read it; press Return to send. If the text goes elsewhere, the panel is not key: report that.
7. Repeat 6 while another app is frontmost before pressing ⌥Space (the usual case). If the transcript
   lands in the other app, or the panel closes when MacWhisper's HUD appears, report it.
8. Quit from the menu; `rm -r $HB`.
````

- [ ] **Step 2: root `README.md`**

In the "Not built yet:" sentence of the status section, change "The menu bar app exists in ui/menubar (built separately with Swift; see its README), and the full desktop app is not built." to "The menu bar app, with a ⌥Space quick-entry panel, exists in ui/menubar (built separately with Swift; see its README), and the full desktop app is not built."

- [ ] **Step 3: `docs/superpowers/plans/README.md`**

Add this row after the phase 1 row:

`| [Desktop UI, phase 2: quick entry](2026-10-22-quick-entry.md) | done (committed on feature/ui-quickentry), 2026-10-22 | `herdbot-core` (`rooms.rs` moves in), `herdbot` `rooms.rs`, `ui/menubar` quick-entry files, README |`

- [ ] **Step 4: spec**

Replace the Phase 2 block in `docs/superpowers/specs/2026-10-21-ui-layer-design.md` with:

````markdown
**Phase 2: quick-entry** (refined 2026-10-22).
- ⌥Space (Carbon `RegisterEventHotKey`, no Accessibility permission) opens a floating panel: a bot/room picker preselected to the last recipient, and a multi-line text field.
- Return or ⌘Return submits through `herdbot send` / `herdbot room send`; the panel shows "queued" and closes. Esc cancels. Nothing is sent without a keypress, so dictated text can be reviewed first.
- Dictation (MacWhisper: hold Right ⌘; it types or pastes into the focused field): the hotkey is not a bare modifier; the panel activates the app and becomes key; it stays open if a dictation HUD takes focus; the field is a standard text field. Unverified until a person tests it with MacWhisper.
- If `daemon` is `down`, the panel warns that messages won't deliver until it's up.
- `status --json` and `watch --json` gain `rooms` (`slug`, `name`, `mode`, `paused`); `v` stays 1 because the field is additive, and the Swift app reads a missing `rooms` as none.
````

- [ ] **Step 5: Verify and commit**

Run: `git diff --stat` (only the four files above), `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`, and `cd ui/menubar && ./scripts/test.sh`.

```bash
git add ui/menubar/README.md README.md docs/superpowers/plans/README.md docs/superpowers/specs/2026-10-21-ui-layer-design.md
git commit -m "docs: document the quick-entry panel and its manual check"
```

## Self-review

- Spec coverage: hotkey + panel (Tasks 2-3), picker over bots and rooms (Tasks 1-2), `send`/`room send` (Task 2), daemon-down warning (Task 3 view), last-recipient memory (Task 2), no auto-send (Task 2 `send` runs only on submit; Task 3 view), MacWhisper constraints (Task 3 notes, Task 4 checklist), additive `rooms` (Task 1, decoder in Task 2).
- Not verifiable here, so left to the manual check: that Return submits a vertical-axis `TextField` (⌘Return is the backup), that the panel takes key focus on activation, and that MacWhisper inserts into it.
