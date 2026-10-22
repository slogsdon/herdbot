# Chat Window (UI phase 3, plan 2 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A chat window in the menu bar app: a sidebar of bots and rooms and a scrollable, read-only chat for the selected one, built on `herdbot history`.

**Architecture:** `HerdbotKit` gains the history models, a `history` call on the CLI wrapper, and testable view-models (`Sidebar`, `Timeline`, `Markdown`, `ChatState`). `HerdbotMenu` gets a `Window` scene (`NavigationSplitView`) that opens from the menu and is suppressed at launch. Sending, unread badges and notification-to-chat are plan 3.

**Tech Stack:** Swift 6, SwiftUI, AppKit, Swift Testing. The deployment floor rises from macOS 14 to **15**, so the window can be kept from opening at launch (`defaultLaunchBehavior`).

Spec: `docs/superpowers/specs/2026-10-22-chat-app-design.md` (The window). Depends on plan 1 (`docs/superpowers/plans/2026-10-22-chat-data-seam.md`): its `herdbot history` and snapshot fields must be installed for the window to show anything beyond "update herdbot".

## Global Constraints

- Branch `feature/ui-chat`, worktree `/Users/shane/Code/herdbot/.worktrees/ui-chat`. Work only there. Plan 1's Rust commits must already be on the branch.
- Shane approved committing each task below: explicit paths only (never `git add -A` or `git add .`), the conventional message given, **no AI or tool attribution**, no push, no merge.
- Never run `cargo install`, never restart the daemon, never launch the menu bar app (a running instance belongs to Shane; do not kill it).
- Swift: 4-space indent, no `@State` (a macro whose plugin is missing under Command Line Tools; use observable classes or `Binding(get:set:)`), zero Swift compiler warnings (linker `search path ... not found` warnings are an environment quirk and fine).
- Test with `timeout 300 ./scripts/test.sh` from `ui/menubar`, never bare `swift test`. The first test build after a change can take two to three minutes; that is a slow compile, not a hang.
- Colours are system semantic colours (`.primary`, `.secondary`, `.quaternary`, `Color.accentColor`); avatars and speaker names use `Avatar.hue(for:)`.
- JSON additions are additive; a herdbot without them must still decode (missing counts read as 0 / empty).
- Baseline before this plan: Swift 27 tests pass.
- Keep your ledger in `.superpowers/sdd/chat-window/progress.md` (untracked; do not commit it).

All code below was compiled, its tests run, and the window launched, screenshotted and clicked through against a scratch herdbot home in a scratch copy before this plan was written. Reproduce it exactly; if a step fails, report it, don't redesign.

---

### Task 1: history models and the `history` call

**Files:**
- Modify: `ui/menubar/Sources/HerdbotKit/Models.swift`, `Stream.swift`, `ProcessCLI.swift`, `QuickEntry.swift`, `ui/menubar/Tests/HerdbotKitTests/Fixtures.swift`, `DecodingTests.swift`, `ProcessCLITests.swift`

**Interfaces:**
- Produces: `BotView { items, lastAt, preview, working }` and `RoomView { items, lastAt, preview }` (JSON `last_at`; missing fields default); `HistoryItem` (`n`, `at`, `kind`, `speaker`, `text`, `state`, `id`, `isPending`); `HistoryPage`; `Recipient.historyTarget` (`alpha` or `room:crew`); `HerdbotCLI.history(of:before:limit:) async throws -> HistoryPage`; `FakeCLI` test double with `pages`, `pending`, `historyCalls`, `historyFailure`.

- [ ] **Step 1: Write the failing tests**

```diff
--- a/ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift
+++ b/ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift
@@ -38,4 +38,34 @@
         #expect(try StreamParser.parse("not json") == nil)
         #expect(try StreamParser.parse(#"{"hello":1}"#) == nil)
     }
+
+    @Test func conversationCountsDecodeAndDefaultForAnOlderHerdbot() throws {
+        let new = """
+        {"v":1,"type":"snapshot","snapshot":{"v":1,"at":1,"daemon":"up",\
+        "bots":[{"slug":"a","title":"A","up":true,"items":3,"last_at":7,"preview":"hi","working":true}],\
+        "rooms":[{"slug":"c","name":"C","mode":"channel","paused":false,"items":2,"last_at":9,"preview":"yo"}],\
+        "runs":[],"cooldowns":[],"blocked":[],"warnings":[]}}
+        """
+        let snap = try #require(try StreamParser.parse(new)?.snapshot)
+        #expect(snap.bots == [BotView(slug: "a", title: "A", up: true, items: 3, lastAt: 7, preview: "hi", working: true)])
+        #expect(snap.rooms == [RoomView(slug: "c", name: "C", mode: "channel", paused: false, items: 2, lastAt: 9, preview: "yo")])
+        let old = try #require(try StreamParser.parse(snapshotLine)?.snapshot)
+        #expect(old.bots == [BotView(slug: "alpha", title: "Alpha", up: true)])
+        #expect(old.bots[0].lastAt == nil)
+    }
+
+    @Test func historyPageDecodesPersistedAndPendingItems() throws {
+        let json = """
+        {"v":1,"target":"alpha","total":2,"items":[
+        {"n":0,"at":1,"kind":"user","text":"hi"},
+        {"n":1,"at":2,"kind":"bot","speaker":"alpha","text":"hello"},
+        {"n":null,"at":3,"kind":"user","text":"later","state":"queued"}],"warnings":["w"]}
+        """
+        let page = try JSONDecoder().decode(HistoryPage.self, from: Data(json.utf8))
+        #expect(page.total == 2)
+        #expect(page.items.map(\.id) == ["n0", "n1", "p:queued:3:later"])
+        #expect(page.items.map(\.isPending) == [false, false, true])
+        #expect(page.items[1].speaker == "alpha")
+        #expect(page.warnings == ["w"])
+    }
 }
```

```diff
--- a/ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift
+++ b/ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift
@@ -45,6 +45,29 @@
         }
     }
 
+    @Test func historyAsksForAPageAndDecodesIt() async throws {
+        let page = ##"{"v":1,"target":"room:crew","total":1,"items":[{"n":0,"at":1,"kind":"user","text":"hi"}],"warnings":[]}"##
+        let newest = try fakeHerdbot("[ \"$*\" = 'history room:crew --json --limit 3' ] && echo '\(page)'")
+        let result = try await ProcessCLI(binary: newest).history(of: .room("crew"), before: nil, limit: 3)
+        #expect(result.items.map(\.text) == ["hi"])
+        let older = try fakeHerdbot("[ \"$*\" = 'history alpha --json --limit 5 --before 9' ] && echo '\(page)'")
+        _ = try await ProcessCLI(binary: older).history(of: .bot("alpha"), before: 9, limit: 5)
+        let bad = try fakeHerdbot("echo 'no bot \"x\"' >&2; exit 1")
+        await #expect(throws: CLIError.failed(#"no bot "x""#)) {
+            _ = try await ProcessCLI(binary: bad).history(of: .bot("x"), before: nil, limit: 3)
+        }
+        let future = try fakeHerdbot(##"echo '{"v":9,"target":"a","total":0,"items":[],"warnings":[]}'"##)
+        await #expect(throws: CLIError.unsupported(9)) {
+            _ = try await ProcessCLI(binary: future).history(of: .bot("a"), before: nil, limit: 3)
+        }
+    }
+
+    @Test func aHugePageDoesNotStallTheProcess() async throws {
+        let big = try fakeHerdbot(##"printf '{"v":1,"target":"a","total":1,"items":[{"n":0,"at":1,"kind":"bot","text":"'; head -c 300000 /dev/zero | tr '\0' x; printf '"}],"warnings":[]}'"##)
+        let result = try await ProcessCLI(binary: big).history(of: .bot("a"), before: nil, limit: 50)
+        #expect(result.items[0].text.count == 300_000)
+    }
+
     @Test func locatePrefersHerdbotBin() throws {
         let script = try fakeHerdbot("true")
         #expect(ProcessCLI.locate(environment: ["HERDBOT_BIN": script.path], home: "/nonexistent") == script)
```

```diff
--- a/ui/menubar/Tests/HerdbotKitTests/Fixtures.swift
+++ b/ui/menubar/Tests/HerdbotKitTests/Fixtures.swift
@@ -52,4 +52,17 @@
         if let failure { throw CLIError.failed(failure) }
         sent.append((recipient, text))
     }
+    /// What `history` answers, keyed by target; an absent target is an empty conversation.
+    nonisolated(unsafe) var pages: [Recipient: [HistoryItem]] = [:]
+    nonisolated(unsafe) var pending: [Recipient: [HistoryItem]] = [:]
+    nonisolated(unsafe) var historyCalls: [(Recipient, Int?, Int)] = []
+    nonisolated(unsafe) var historyFailure: String?
+    func history(of target: Recipient, before: Int?, limit: Int) async throws -> HistoryPage {
+        historyCalls.append((target, before, limit))
+        if let historyFailure { throw CLIError.failed(historyFailure) }
+        let all = pages[target] ?? []
+        let end = min(before ?? all.count, all.count)
+        let slice = Array(all[max(0, end - limit)..<end])
+        return HistoryPage(target: target.historyTarget, total: all.count, items: slice + (before == nil ? pending[target] ?? [] : []))
+    }
 }
```

- [ ] **Step 2: Run to see it fail**

Run (from `ui/menubar`): `timeout 300 ./scripts/test.sh`
Expected: build FAIL (`HistoryPage`, `history(of:before:limit:)`, `lastAt` do not exist).

- [ ] **Step 3: Implement**

```diff
--- a/ui/menubar/Sources/HerdbotKit/Models.swift
+++ b/ui/menubar/Sources/HerdbotKit/Models.swift
@@ -17,6 +17,28 @@
     public let slug: String
     public let title: String
     public let up: Bool
+    /// History items, as `HistoryPage.total`. Zero from a herdbot that doesn't send it.
+    public let items: Int
+    /// Unix seconds of the last item.
+    public let lastAt: Int?
+    /// First line of the last item.
+    public let preview: String
+    /// A prompt was delivered and its turn has not been logged yet.
+    public let working: Bool
+
+    enum CodingKeys: String, CodingKey {
+        case slug, title, up, items, lastAt = "last_at", preview, working
+    }
+
+    public init(slug: String, title: String, up: Bool, items: Int = 0, lastAt: Int? = nil, preview: String = "", working: Bool = false) {
+        self.slug = slug
+        self.title = title
+        self.up = up
+        self.items = items
+        self.lastAt = lastAt
+        self.preview = preview
+        self.working = working
+    }
 }
 
 public struct RoomView: Codable, Hashable, Sendable {
@@ -25,6 +47,23 @@
     /// "channel", "round_robin" or "moderated".
     public let mode: String
     public let paused: Bool
+    public let items: Int
+    public let lastAt: Int?
+    public let preview: String
+
+    enum CodingKeys: String, CodingKey {
+        case slug, name, mode, paused, items, lastAt = "last_at", preview
+    }
+
+    public init(slug: String, name: String, mode: String, paused: Bool, items: Int = 0, lastAt: Int? = nil, preview: String = "") {
+        self.slug = slug
+        self.name = name
+        self.mode = mode
+        self.paused = paused
+        self.items = items
+        self.lastAt = lastAt
+        self.preview = preview
+    }
 }
 
 public struct RunView: Codable, Hashable, Sendable {
@@ -79,3 +118,76 @@
             warnings: try c.decode([String].self, forKey: .warnings))
     }
 }
+
+extension BotView {
+    /// A herdbot older than conversation counts sends only slug, title and up.
+    public init(from decoder: any Decoder) throws {
+        let c = try decoder.container(keyedBy: CodingKeys.self)
+        self.init(
+            slug: try c.decode(String.self, forKey: .slug),
+            title: try c.decode(String.self, forKey: .title),
+            up: try c.decode(Bool.self, forKey: .up),
+            items: try c.decodeIfPresent(Int.self, forKey: .items) ?? 0,
+            lastAt: try c.decodeIfPresent(Int.self, forKey: .lastAt),
+            preview: try c.decodeIfPresent(String.self, forKey: .preview) ?? "",
+            working: try c.decodeIfPresent(Bool.self, forKey: .working) ?? false)
+    }
+}
+
+extension RoomView {
+    public init(from decoder: any Decoder) throws {
+        let c = try decoder.container(keyedBy: CodingKeys.self)
+        self.init(
+            slug: try c.decode(String.self, forKey: .slug),
+            name: try c.decode(String.self, forKey: .name),
+            mode: try c.decode(String.self, forKey: .mode),
+            paused: try c.decode(Bool.self, forKey: .paused),
+            items: try c.decodeIfPresent(Int.self, forKey: .items) ?? 0,
+            lastAt: try c.decodeIfPresent(Int.self, forKey: .lastAt),
+            preview: try c.decodeIfPresent(String.self, forKey: .preview) ?? "")
+    }
+}
+
+/// One message in a conversation, as `herdbot history --json` prints it.
+public struct HistoryItem: Codable, Hashable, Sendable, Identifiable {
+    /// Index among the persisted items, stable as new ones append. nil while pending.
+    public let n: Int?
+    public let at: Int
+    /// "user", "bot" or "notice".
+    public let kind: String
+    /// A room's speaker (a bot slug) on `bot` items.
+    public let speaker: String?
+    public let text: String
+    /// "inflight" or "queued" on pending items.
+    public let state: String?
+
+    public init(n: Int?, at: Int, kind: String, speaker: String? = nil, text: String, state: String? = nil) {
+        self.n = n
+        self.at = at
+        self.kind = kind
+        self.speaker = speaker
+        self.text = text
+        self.state = state
+    }
+
+    public var id: String { n.map { "n\($0)" } ?? "p:\(state ?? ""):\(at):\(text)" }
+    public var isPending: Bool { n == nil }
+}
+
+/// `herdbot history <target> --json`.
+public struct HistoryPage: Codable, Hashable, Sendable {
+    public let v: Int
+    public let target: String
+    /// Persisted items in the whole conversation; pending ones are not counted.
+    public let total: Int
+    public let items: [HistoryItem]
+    public let warnings: [String]
+
+    public init(v: Int = supportedVersion, target: String, total: Int, items: [HistoryItem], warnings: [String] = []) {
+        self.v = v
+        self.target = target
+        self.total = total
+        self.items = items
+        self.warnings = warnings
+    }
+}
```

```diff
--- a/ui/menubar/Sources/HerdbotKit/Stream.swift
+++ b/ui/menubar/Sources/HerdbotKit/Stream.swift
@@ -22,6 +22,8 @@
     func focus(agent: String) async throws
     /// `herdbot send <bot> <text>` or `herdbot room send <room> <text>`: queues the text.
     func send(to recipient: Recipient, text: String) async throws
+    /// `herdbot history <target> --json`: the newest page, or the `limit` items before item `before`.
+    func history(of target: Recipient, before: Int?, limit: Int) async throws -> HistoryPage
 }
 
 public enum StreamParser {
```

```diff
--- a/ui/menubar/Sources/HerdbotKit/QuickEntry.swift
+++ b/ui/menubar/Sources/HerdbotKit/QuickEntry.swift
@@ -14,6 +14,14 @@
         }
     }
 
+    /// The `<target>` argument of `herdbot history`.
+    var historyTarget: String {
+        switch self {
+        case .bot(let slug): slug
+        case .room(let slug): "room:\(slug)"
+        }
+    }
+
     init?(key: String) {
         if key.hasPrefix("bot:") { self = .bot(String(key.dropFirst(4))) }
         else if key.hasPrefix("room:") { self = .room(String(key.dropFirst(5))) }
```

```diff
--- a/ui/menubar/Sources/HerdbotKit/ProcessCLI.swift
+++ b/ui/menubar/Sources/HerdbotKit/ProcessCLI.swift
@@ -65,29 +65,49 @@
         }
     }
 
-    /// Run `herdbot <arguments>` to completion; a non-zero exit throws its stderr.
+    public func history(of target: Recipient, before: Int?, limit: Int) async throws -> HistoryPage {
+        var arguments = ["history", target.historyTarget, "--json", "--limit", String(limit)]
+        if let before { arguments += ["--before", String(before)] }
+        let data = try await output(arguments)
+        let page = try JSONDecoder().decode(HistoryPage.self, from: data)
+        guard page.v == supportedVersion else { throw CLIError.unsupported(page.v) }
+        return page
+    }
+
     private func run(_ arguments: [String]) async throws {
-        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
+        _ = try await output(arguments)
+    }
+
+    /// Run `herdbot <arguments>` to completion and return its stdout; a non-zero exit throws its
+    /// stderr. Stdout is read to its end while the process runs, so a large page can't stall it; stderr
+    /// (herdbot's errors are one line) is read after it.
+    private func output(_ arguments: [String]) async throws -> Data {
+        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
             let process = Process()
+            let outputs = Pipe()
             let errors = Pipe()
             process.executableURL = binary
             process.arguments = arguments
             process.environment = environment
-            process.standardOutput = FileHandle.nullDevice
+            process.standardOutput = outputs
             process.standardError = errors
-            process.terminationHandler = { finished in
-                if finished.terminationStatus == 0 {
-                    continuation.resume()
-                } else {
-                    let text = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
-                    continuation.resume(throwing: CLIError.failed(text.trimmingCharacters(in: .whitespacesAndNewlines)))
-                }
-            }
             do {
                 try process.run()
             } catch {
                 continuation.resume(throwing: CLIError.failed("could not start herdbot: \(error.localizedDescription)"))
+                return
             }
+            DispatchQueue.global().async {
+                let data = outputs.fileHandleForReading.readDataToEndOfFile()
+                let errorText = errors.fileHandleForReading.readDataToEndOfFile()
+                process.waitUntilExit()
+                if process.terminationStatus == 0 {
+                    continuation.resume(returning: data)
+                } else {
+                    let text = String(decoding: errorText, as: UTF8.self)
+                    continuation.resume(throwing: CLIError.failed(text.trimmingCharacters(in: .whitespacesAndNewlines)))
+                }
+            }
         }
     }
 }
```

- [ ] **Step 4: Run to see it pass**

Run: `timeout 300 ./scripts/test.sh`
Expected: PASS, 31 tests (27 plus 4 new). `swift build` has no Swift warnings.

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotKit/Models.swift ui/menubar/Sources/HerdbotKit/Stream.swift ui/menubar/Sources/HerdbotKit/ProcessCLI.swift ui/menubar/Sources/HerdbotKit/QuickEntry.swift ui/menubar/Tests/HerdbotKitTests/Fixtures.swift ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift
git commit -m "feat(menubar): read a conversation's history through the CLI"
```

---

### Task 2: view-models for the sidebar and the chat

**Files:**
- Create: `ui/menubar/Sources/HerdbotKit/Sidebar.swift`, `Timeline.swift`, `ChatState.swift`, `ui/menubar/Tests/HerdbotKitTests/ChatTests.swift`

**Interfaces:**
- Consumes: Task 1's `HistoryItem`, `HistoryPage`, `HerdbotCLI.history`, `FakeCLI`, and the existing `MenuState`.
- Produces: `Avatar.hue(for:)`/`initial(for:)`; `SidebarRow` and `Sidebar.rows(from:filter:)`; `TimelineEntry` and `Timeline.entries(_:calendar:now:)`; `Markdown.blocks(_:)`; `ChatState(menu:cli:)` with `selection`, `items`, `canLoadOlder`, `problem`, `loading`, `filter`, `atBottom`, `select(_:)`, `observe()`, `refresh()`, `loadOlder()`.

- [ ] **Step 1: Write the failing tests**

Create `ui/menubar/Tests/HerdbotKitTests/ChatTests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

func texts(_ items: [HistoryItem]) -> [String] { items.map(\.text) }

func turns(_ count: Int, from start: Int = 0) -> [HistoryItem] {
    (start..<(start + count)).map { HistoryItem(n: $0, at: 100 + $0, kind: $0 % 2 == 0 ? "user" : "bot", text: "m\($0)") }
}

@MainActor
@Suite struct Chat {
    func make(_ cli: FakeCLI, bots: [BotView] = [BotView(slug: "alpha", title: "Alpha", up: true, items: 0)]) -> (ChatState, MenuState) {
        let menu = MenuState()
        menu.apply(line(snapshot(bots: bots)))
        return (ChatState(menu: menu, cli: cli), menu)
    }

    @Test func selectingLoadsTheNewestPage() async {
        let cli = FakeCLI()
        cli.pages[.bot("alpha")] = turns(120)
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        #expect(chat.items.count == ChatState.pageSize)
        #expect(chat.items.first?.n == 70)
        #expect(chat.items.last?.n == 119)
        #expect(chat.canLoadOlder)
        #expect(cli.historyCalls.count == 1)
    }

    @Test func olderPagesPrependAndStopAtTheStart() async {
        let cli = FakeCLI()
        cli.pages[.bot("alpha")] = turns(120)
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        #expect(await chat.loadOlder() == "n70")
        #expect(chat.items.first?.n == 20)
        #expect(chat.items.count == 100)
        #expect(await chat.loadOlder() == "n20")
        #expect(chat.items.first?.n == 0)
        #expect(!chat.canLoadOlder)
        #expect(await chat.loadOlder() == nil)
    }

    @Test func refreshAddsNewItemsAndKeepsOlderOnes() async {
        let cli = FakeCLI()
        cli.pages[.bot("alpha")] = turns(120)
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        await chat.loadOlder()
        cli.pages[.bot("alpha")] = turns(122)
        await chat.refresh()
        #expect(chat.items.first?.n == 20)
        #expect(chat.items.last?.n == 121)
        #expect(chat.items.count == 102)
        #expect(Set(chat.items.compactMap(\.n)).count == 102)
    }

    @Test func aGapLargerThanAPageDropsTheStaleHistory() async {
        let cli = FakeCLI()
        cli.pages[.bot("alpha")] = turns(10)
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        cli.pages[.bot("alpha")] = turns(200)
        await chat.refresh()
        #expect(chat.items.first?.n == 150)
        #expect(chat.items.count == 50)
    }

    @Test func pendingItemsComeAndGoWithEachFetch() async {
        let cli = FakeCLI()
        cli.pages[.bot("alpha")] = turns(2)
        cli.pending[.bot("alpha")] = [HistoryItem(n: nil, at: 500, kind: "user", text: "queued one", state: "queued")]
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        #expect(texts(chat.items) == ["m0", "m1", "queued one"])
        cli.pending[.bot("alpha")] = []
        cli.pages[.bot("alpha")] = turns(3)
        await chat.refresh()
        #expect(texts(chat.items) == ["m0", "m1", "m2"])
    }

    @Test func observeRefetchesOnlyWhenTheOpenConversationChanged() async {
        let cli = FakeCLI()
        let (chat, menu) = make(cli)
        await chat.select(.bot("alpha"))
        #expect(cli.historyCalls.count == 1)
        await chat.observe()
        #expect(cli.historyCalls.count == 1)
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "Alpha", up: true, items: 2)])))
        await chat.observe()
        #expect(cli.historyCalls.count == 2)
        // A different bot changing is not news for this conversation.
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "Alpha", up: true, items: 2), BotView(slug: "beta", title: "B", up: true, items: 9)])))
        await chat.observe()
        #expect(cli.historyCalls.count == 2)
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "Alpha", up: true, items: 2, working: true)])))
        await chat.observe()
        #expect(cli.historyCalls.count == 3)
    }

    @Test func switchingConversationsClearsTheOldOne() async {
        let cli = FakeCLI()
        cli.pages[.bot("alpha")] = turns(3)
        cli.pages[.room("crew")] = [HistoryItem(n: 0, at: 1, kind: "bot", speaker: "alpha", text: "hello")]
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        await chat.select(.room("crew"))
        #expect(texts(chat.items) == ["hello"])
        #expect(chat.selection == .room("crew"))
    }

    @Test func failuresBecomeAProblemAndClearOnSuccess() async {
        let cli = FakeCLI()
        cli.historyFailure = "usage:\n  herdbot daemon ..."
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        #expect(chat.problem == "update herdbot: this version has no `history` command")
        cli.historyFailure = "disk on fire"
        await chat.refresh()
        #expect(chat.problem == "disk on fire")
        cli.historyFailure = nil
        await chat.refresh()
        #expect(chat.problem == nil)
    }

    @Test func noHerdbotMeansNothingToLoad() async {
        let chat = ChatState(menu: MenuState(), cli: nil)
        await chat.select(.bot("alpha"))
        #expect(chat.items.isEmpty)
    }
}

@Suite struct Presentation {
    @Test func sidebarOrdersByRecentActivityThenName() {
        let snap = snapshot(bots: [
            BotView(slug: "zed", title: "Zed", up: true, lastAt: 5),
            BotView(slug: "amy", title: "Amy", up: false, lastAt: 9, working: true),
            BotView(slug: "bob", title: "", up: true),
            BotView(slug: "cat", title: "Cat", up: true),
        ], rooms: [RoomView(slug: "crew", name: "Crew", mode: "channel", paused: true, lastAt: 1)])
        let rows = Sidebar.rows(from: snap)
        #expect(rows.bots.map(\.title) == ["Amy", "Zed", "bob", "Cat"])
        #expect(rows.bots[0].dimmed && rows.bots[0].working)
        #expect(rows.rooms.map(\.id) == [.room("crew")])
        #expect(rows.rooms[0].dimmed)
    }

    @Test func sidebarFilterMatchesTitleOrSlugIgnoringCase() {
        let snap = snapshot(bots: [BotView(slug: "herdbot-dev", title: "Maintainer", up: true), BotView(slug: "atlas", title: "Chief", up: true)])
        #expect(Sidebar.rows(from: snap, filter: "MAINT").bots.map(\.id) == [.bot("herdbot-dev")])
        #expect(Sidebar.rows(from: snap, filter: "atl").bots.map(\.id) == [.bot("atlas")])
        #expect(Sidebar.rows(from: snap, filter: "  ").bots.count == 2)
        #expect(Sidebar.rows(from: snap, filter: "zzz").bots.isEmpty)
    }

    @Test func avatarIsStableAndInRange() {
        #expect(Avatar.hue(for: "alpha") == Avatar.hue(for: "alpha"))
        #expect(Avatar.hue(for: "alpha") != Avatar.hue(for: "beta"))
        #expect((0..<1).contains(Avatar.hue(for: "x")))
        #expect(Avatar.initial(for: "  ¿atlas") == "A")
        #expect(Avatar.initial(for: "") == "?")
    }

    @Test func timelineInsertsADayLabelPerCalendarDay() {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let noon = { (day: Int) in Int(DateComponents(calendar: calendar, timeZone: calendar.timeZone, year: 2026, month: 10, day: day, hour: 12).date!.timeIntervalSince1970) }
        let now = DateComponents(calendar: calendar, timeZone: calendar.timeZone, year: 2026, month: 10, day: 22, hour: 18).date!
        let items = [
            HistoryItem(n: 0, at: noon(20), kind: "user", text: "a"),
            HistoryItem(n: 1, at: noon(20) + 60, kind: "bot", text: "b"),
            HistoryItem(n: 2, at: noon(21), kind: "user", text: "c"),
            HistoryItem(n: nil, at: noon(22), kind: "user", text: "d", state: "queued"),
        ]
        let entries = Timeline.entries(items, calendar: calendar, now: now)
        let shape = entries.map { entry -> String in
            switch entry {
            case .day(let label): "day \(label)"
            case .item(let item): item.text
            }
        }
        #expect(shape == ["day Tuesday, October 20", "a", "b", "day Yesterday", "c", "day Today", "d"])
        #expect(Timeline.dayLabel(for: DateComponents(calendar: calendar, timeZone: calendar.timeZone, year: 2025, month: 3, day: 4).date!, now: now, calendar: calendar).contains("2025"))
        #expect(Set(entries.map(\.id)).count == entries.count)
    }

    @Test func markdownSplitsFencedCodeFromText() {
        let source = "Here:\n\n```swift\nlet x = 1\n```\n\nand `inline` after\n```\nunclosed\nstill code"
        #expect(Markdown.blocks(source) == [
            .text("Here:"),
            .code(language: "swift", body: "let x = 1"),
            .text("and `inline` after"),
            .code(language: nil, body: "unclosed\nstill code"),
        ])
        #expect(Markdown.blocks("") == [])
        #expect(Markdown.blocks("just text\nmore") == [.text("just text\nmore")])
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `timeout 300 ./scripts/test.sh`
Expected: build FAIL (`ChatState`, `Sidebar`, `Timeline`, `Markdown`, `Avatar` do not exist).

- [ ] **Step 3: Implement**

Create `ui/menubar/Sources/HerdbotKit/Sidebar.swift`:

```swift
import Foundation

public enum Avatar {
    /// A stable hue in 0..<1 for a slug (FNV-1a), so a bot keeps its colour between launches.
    /// Swift's own `hashValue` is randomised per process.
    public static func hue(for seed: String) -> Double {
        var hash: UInt32 = 2_166_136_261
        for byte in seed.utf8 {
            hash = (hash ^ UInt32(byte)) &* 16_777_619
        }
        return Double(hash % 360) / 360
    }

    /// The first letter, upper-cased, for the avatar disc.
    public static func initial(for name: String) -> String {
        name.first { $0.isLetter || $0.isNumber }.map { String($0).uppercased() } ?? "?"
    }
}

public struct SidebarRow: Identifiable, Hashable, Sendable {
    public let id: Recipient
    public let title: String
    public let preview: String
    public let lastAt: Int?
    public let working: Bool
    /// A bot that isn't running; its history is still readable.
    public let dimmed: Bool
    /// Colours the avatar and, in rooms, the speaker's name.
    public let slug: String
}

public enum Sidebar {
    /// Bots and rooms for the list, most recently active first (never-active ones last, by name).
    /// `filter` matches the title or slug, ignoring case.
    public static func rows(from snapshot: Snapshot, filter: String = "") -> (bots: [SidebarRow], rooms: [SidebarRow]) {
        let bots = snapshot.bots.map {
            SidebarRow(id: .bot($0.slug), title: $0.title.isEmpty ? $0.slug : $0.title, preview: $0.preview,
                       lastAt: $0.lastAt, working: $0.working, dimmed: !$0.up, slug: $0.slug)
        }
        let rooms = snapshot.rooms.map {
            SidebarRow(id: .room($0.slug), title: $0.name.isEmpty ? $0.slug : $0.name, preview: $0.preview,
                       lastAt: $0.lastAt, working: false, dimmed: $0.paused, slug: $0.slug)
        }
        return (ordered(matching(bots, filter)), ordered(matching(rooms, filter)))
    }

    private static func matching(_ rows: [SidebarRow], _ filter: String) -> [SidebarRow] {
        let needle = filter.trimmingCharacters(in: .whitespaces)
        guard !needle.isEmpty else { return rows }
        return rows.filter { $0.title.localizedCaseInsensitiveContains(needle) || $0.slug.localizedCaseInsensitiveContains(needle) }
    }

    private static func ordered(_ rows: [SidebarRow]) -> [SidebarRow] {
        rows.sorted {
            switch ($0.lastAt, $1.lastAt) {
            case let (a?, b?) where a != b: a > b
            case (_?, nil): true
            case (nil, _?): false
            default: $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending
            }
        }
    }
}
```

Create `ui/menubar/Sources/HerdbotKit/Timeline.swift`:

```swift
import Foundation

public enum TimelineEntry: Hashable, Identifiable, Sendable {
    case day(String)
    case item(HistoryItem)

    public var id: String {
        switch self {
        case .day(let label): "day:\(label)"
        case .item(let item): item.id
        }
    }
}

public enum Timeline {
    /// The items with a day label before the first item of each calendar day.
    public static func entries(_ items: [HistoryItem], calendar: Calendar = .current, now: Date = .now) -> [TimelineEntry] {
        var out: [TimelineEntry] = []
        var lastDay: DateComponents?
        for item in items {
            let date = Date(timeIntervalSince1970: TimeInterval(item.at))
            let day = calendar.dateComponents([.year, .month, .day], from: date)
            if day != lastDay {
                out.append(.day(dayLabel(for: date, now: now, calendar: calendar)))
                lastDay = day
            }
            out.append(.item(item))
        }
        return out
    }

    public static func dayLabel(for date: Date, now: Date, calendar: Calendar) -> String {
        if calendar.isDate(date, inSameDayAs: now) { return "Today" }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(date, inSameDayAs: yesterday) {
            return "Yesterday"
        }
        var style = Date.FormatStyle(date: .omitted, time: .omitted, calendar: calendar, timeZone: calendar.timeZone)
        style = calendar.isDate(date, equalTo: now, toGranularity: .year)
            ? style.weekday(.wide).month(.wide).day()
            : style.year().month(.wide).day()
        return date.formatted(style)
    }
}

public enum MarkdownBlock: Hashable, Sendable {
    case text(String)
    case code(language: String?, body: String)
}

public enum Markdown {
    /// Splits fenced code blocks (```) from the text around them. An unclosed fence runs to the end.
    /// Everything outside a fence is left for inline markdown.
    public static func blocks(_ source: String) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        var text: [Substring] = []
        var code: [Substring] = []
        var language: String?
        var inFence = false

        func flushText() {
            let joined = text.joined(separator: "\n").trimmingCharacters(in: .newlines)
            if !joined.isEmpty { blocks.append(.text(joined)) }
            text = []
        }

        for line in source.split(separator: "\n", omittingEmptySubsequences: false) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") {
                if inFence {
                    blocks.append(.code(language: language, body: code.joined(separator: "\n")))
                    code = []
                    inFence = false
                } else {
                    flushText()
                    let name = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces)
                    language = name.isEmpty ? nil : name
                    inFence = true
                }
            } else if inFence {
                code.append(line)
            } else {
                text.append(line)
            }
        }
        if inFence { blocks.append(.code(language: language, body: code.joined(separator: "\n"))) }
        flushText()
        return blocks
    }
}
```

Create `ui/menubar/Sources/HerdbotKit/ChatState.swift`:

```swift
import Foundation
import Observation

/// The open conversation: the newest page, older pages on demand, and what is still pending.
@MainActor @Observable
public final class ChatState {
    public static let pageSize = 50

    public private(set) var selection: Recipient?
    public private(set) var problem: String?
    public private(set) var loading = false
    /// The sidebar's search text.
    public var filter = ""
    /// Whether the newest message is on screen; set by the chat view.
    public var atBottom = true
    private var persisted: [HistoryItem] = []
    private var pending: [HistoryItem] = []
    private var lastKey: Counts?
    @ObservationIgnored private let menu: MenuState
    @ObservationIgnored private let cli: (any HerdbotCLI)?

    /// What changes in a snapshot when the open conversation has news.
    struct Counts: Equatable {
        let items: Int
        let working: Bool
    }

    public init(menu: MenuState, cli: (any HerdbotCLI)?) {
        self.menu = menu
        self.cli = cli
    }

    /// Persisted items oldest first, then pending ones.
    public var items: [HistoryItem] { persisted + pending }

    public var canLoadOlder: Bool { (persisted.first?.n ?? 0) > 0 }

    public func select(_ recipient: Recipient?) async {
        guard recipient != selection else { return }
        selection = recipient
        persisted = []
        pending = []
        problem = nil
        lastKey = counts(for: recipient)
        await refresh()
    }

    /// Re-fetch the newest page when the snapshot says the open conversation changed.
    public func observe() async {
        let key = counts(for: selection)
        guard key != lastKey else { return }
        lastKey = key
        await refresh()
    }

    public func refresh() async {
        guard let cli, let target = selection else { return }
        do {
            let page = try await cli.history(of: target, before: nil, limit: Self.pageSize)
            guard target == selection else { return }
            let fresh = page.items.filter { !$0.isPending }
            var older = persisted
            if let firstNew = fresh.first?.n {
                older = older.filter { ($0.n ?? 0) < firstNew }
                // New items past a whole page leave a gap; drop what we had rather than show it.
                if let last = older.last?.n, last + 1 < firstNew { older = [] }
            }
            persisted = older + fresh
            pending = page.items.filter(\.isPending)
            problem = nil
        } catch {
            guard target == selection else { return }
            problem = Self.describe(error)
        }
    }

    /// Prepend the page before the oldest loaded item. Returns the id of the item that was first,
    /// so the view can keep its place.
    @discardableResult
    public func loadOlder() async -> String? {
        guard let cli, let target = selection, !loading, let oldest = persisted.first, let n = oldest.n, n > 0 else { return nil }
        loading = true
        defer { loading = false }
        do {
            let page = try await cli.history(of: target, before: n, limit: Self.pageSize)
            guard target == selection else { return nil }
            persisted = page.items.filter { !$0.isPending } + persisted
            return oldest.id
        } catch {
            problem = Self.describe(error)
            return nil
        }
    }

    private func counts(for recipient: Recipient?) -> Counts? {
        guard let snapshot = menu.snapshot, let recipient else { return nil }
        switch recipient {
        case .bot(let slug):
            return snapshot.bots.first { $0.slug == slug }.map { Counts(items: $0.items, working: $0.working) }
        case .room(let slug):
            return snapshot.rooms.first { $0.slug == slug }.map { Counts(items: $0.items, working: false) }
        }
    }

    /// An older herdbot has no `history`; it answers with its usage text.
    static func describe(_ error: Error) -> String {
        let message = error.localizedDescription
        if message.contains("usage:") { return "update herdbot: this version has no `history` command" }
        return message
    }
}
```

- [ ] **Step 4: Run to see it pass**

Run: `timeout 300 ./scripts/test.sh`
Expected: PASS, 45 tests (31 plus 14 new).

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotKit/Sidebar.swift ui/menubar/Sources/HerdbotKit/Timeline.swift ui/menubar/Sources/HerdbotKit/ChatState.swift ui/menubar/Tests/HerdbotKitTests/ChatTests.swift
git commit -m "feat(menubar): view-models for the chat sidebar and timeline"
```

---

### Task 3: the window

No unit tests: this is SwiftUI presentation. The gate is a warning-free build and a bundle; looks and scrolling are checked by a person (the manual check below).

**Files:**
- Create: `ui/menubar/Sources/HerdbotMenu/MessageViews.swift`, `SidebarView.swift`, `ChatView.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/App.swift`, `MenuContent.swift`, `ui/menubar/Package.swift`, `ui/menubar/Info.plist`, `ui/menubar/README.md`

**Interfaces:**
- Consumes: Task 2's view-models.
- Produces: a `Window("herdbot", id: "chat")` scene, suppressed at launch, opened by the menu's "Open herdbot"; the Dock icon shows only while it is open (`NSApp.setActivationPolicy`).

Design notes the code encodes (do not change them without asking):
- Sidebar: `NavigationSplitView` with `List(selection:)` and `.searchable(placement: .sidebar)`; sections Bots and Rooms, newest activity first; dimmed rows for down bots and paused rooms; a spinner while a bot is `working`.
- Chat: yours right, the bot's left, notices small and centred, rooms name the speaker in their colour; fenced code in monospace; pending items lighter, with "queued" or a "working" spinner; day separators; `defaultScrollAnchor(.bottom)`; a "jump to latest" button driven by the bottom sentinel row's `onAppear`/`onDisappear` (no macOS 15 scroll-geometry API needed).
- The header's "Jump to pane" runs `focus` for the bot's main agent (its slug); subagent chips run `focus` for the run's agent; a blocked banner shows for a blocked event whose `agent` is the bot's slug.

- [ ] **Step 1: Create the three view files**

`ui/menubar/Sources/HerdbotMenu/MessageViews.swift`:

```swift
import HerdbotKit
import SwiftUI

/// Inline markdown, with fenced code blocks in monospace. Selectable.
struct MarkdownText: View {
    let source: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(Markdown.blocks(source).enumerated()), id: \.offset) { _, block in
                switch block {
                case .text(let text):
                    Text(inline(text))
                case .code(_, let body):
                    ScrollView(.horizontal, showsIndicators: false) {
                        Text(body).font(.system(.callout, design: .monospaced)).padding(8)
                    }
                    .background(.black.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
            }
        }
        .textSelection(.enabled)
    }

    private func inline(_ text: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        return (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
    }
}

struct SpeakerColor {
    static func color(for slug: String) -> Color {
        Color(hue: Avatar.hue(for: slug), saturation: 0.6, brightness: 0.8)
    }
}

/// One message: yours on the right, the bot's on the left, notices small and centred.
struct MessageRow: View {
    let item: HistoryItem
    /// Rooms name the speaker above each bot message.
    let showSpeaker: Bool

    var body: some View {
        switch item.kind {
        case "user": bubble(alignment: .trailing, fill: AnyShapeStyle(Color.accentColor), foreground: .white)
        case "notice":
            Text(item.text).font(.caption).foregroundStyle(.secondary).frame(maxWidth: .infinity)
        default: bubble(alignment: .leading, fill: AnyShapeStyle(.quaternary), foreground: .primary)
        }
    }

    private func bubble(alignment: HorizontalAlignment, fill: AnyShapeStyle, foreground: Color) -> some View {
        VStack(alignment: alignment, spacing: 2) {
            if showSpeaker, let speaker = item.speaker {
                Text(speaker).font(.caption.weight(.semibold)).foregroundStyle(SpeakerColor.color(for: speaker))
            }
            MarkdownText(source: item.text)
                .foregroundStyle(foreground)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(fill, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                .opacity(item.isPending ? 0.6 : 1)
            if let state = item.state {
                HStack(spacing: 4) {
                    if state == "inflight" { ProgressView().controlSize(.mini) }
                    Text(state == "inflight" ? "working" : "queued").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: alignment == .trailing ? .trailing : .leading)
        .padding(alignment == .trailing ? .leading : .trailing, 60)
    }
}

struct DaySeparator: View {
    let label: String

    var body: some View {
        Text(label)
            .font(.caption.weight(.medium))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .padding(.vertical, 3)
            .background(.quaternary, in: Capsule())
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
    }
}
```

`ui/menubar/Sources/HerdbotMenu/SidebarView.swift`:

```swift
import HerdbotKit
import SwiftUI

struct AvatarView: View {
    let slug: String
    let title: String
    var size: CGFloat = 34

    var body: some View {
        Circle()
            .fill(Color(hue: Avatar.hue(for: slug), saturation: 0.45, brightness: 0.85))
            .frame(width: size, height: size)
            .overlay(Text(Avatar.initial(for: title)).font(.system(size: size * 0.42, weight: .semibold)).foregroundStyle(.white))
    }
}

private struct Row: View {
    let row: SidebarRow

    var body: some View {
        HStack(spacing: 10) {
            AvatarView(slug: row.slug, title: row.title)
            VStack(alignment: .leading, spacing: 1) {
                Text(row.title).font(.body.weight(.medium)).lineLimit(1)
                Text(row.preview.isEmpty ? " " : row.preview).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            VStack(alignment: .trailing, spacing: 3) {
                if let at = row.lastAt { Text(time(at)).font(.caption2).foregroundStyle(.secondary) }
                if row.working { ProgressView().controlSize(.mini) }
            }
        }
        .opacity(row.dimmed ? 0.55 : 1)
        .padding(.vertical, 2)
    }

    private func time(_ seconds: Int) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        return Calendar.current.isDateInToday(date)
            ? date.formatted(.dateTime.hour().minute())
            : date.formatted(.dateTime.month(.abbreviated).day())
    }
}

/// Bots, then rooms, newest activity first, with the system search field on top.
struct SidebarView: View {
    let menu: MenuState
    let chat: ChatState

    var body: some View {
        let rows = menu.snapshot.map { Sidebar.rows(from: $0, filter: chat.filter) } ?? (bots: [], rooms: [])
        List(selection: Binding(get: { chat.selection }, set: { choice in Task { await chat.select(choice) } })) {
            if !rows.bots.isEmpty {
                Section("Bots") { ForEach(rows.bots) { Row(row: $0).tag(Optional($0.id)) } }
            }
            if !rows.rooms.isEmpty {
                Section("Rooms") { ForEach(rows.rooms) { Row(row: $0).tag(Optional($0.id)) } }
            }
        }
        .searchable(text: Binding(get: { chat.filter }, set: { chat.filter = $0 }), placement: .sidebar, prompt: "Filter")
        .overlay {
            if rows.bots.isEmpty && rows.rooms.isEmpty {
                Text(menu.problem ?? (chat.filter.isEmpty ? "No bots yet" : "No matches")).foregroundStyle(.secondary)
            }
        }
    }
}
```

`ui/menubar/Sources/HerdbotMenu/ChatView.swift`:

```swift
import HerdbotKit
import SwiftUI

/// The selected conversation: a header, a blocked banner, and the scrollable messages.
struct ChatView: View {
    let menu: MenuState
    let chat: ChatState
    let cli: ProcessCLI?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        if let selection = chat.selection {
            VStack(spacing: 0) {
                header(selection)
                Divider()
                banner(selection)
                messages(selection)
            }
        } else {
            Text("Choose a bot or room").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    /// The header's title and status line, and the agent a bot's "Jump to pane" focuses.
    private func info(for selection: Recipient) -> (title: String, status: String, agent: String?) {
        switch selection {
        case .bot(let slug):
            let bot = menu.snapshot?.bots.first { $0.slug == slug }
            let status = bot?.working == true ? "working…" : (bot?.up == true ? "idle" : "not running")
            return (bot.map { $0.title.isEmpty ? slug : $0.title } ?? slug, status, slug)
        case .room(let slug):
            let room = menu.snapshot?.rooms.first { $0.slug == slug }
            let status = [room?.mode.replacingOccurrences(of: "_", with: " "), room?.paused == true ? "paused" : nil]
                .compactMap { $0 }.joined(separator: " · ")
            return (room.map { $0.name.isEmpty ? slug : $0.name } ?? slug, status, nil)
        }
    }

    private func header(_ selection: Recipient) -> some View {
        let info = info(for: selection)
        let runs = menu.snapshot?.runs.filter { selection == .bot($0.bot) } ?? []
        return HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 1) {
                Text(info.title).font(.headline)
                Text(info.status).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            ForEach(runs, id: \.id) { run in
                Button { Task { try? await cli?.focus(agent: run.agent) } } label: {
                    Label(run.description, systemImage: "terminal").lineLimit(1)
                }
                .buttonStyle(.bordered).controlSize(.small)
            }
            if let agent = info.agent {
                Button("Jump to pane") { Task { try? await cli?.focus(agent: agent) } }.controlSize(.small)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    @ViewBuilder private func banner(_ selection: Recipient) -> some View {
        if case .bot(let slug) = selection, let blocked = menu.snapshot?.blocked.first(where: { $0.agent == slug }) {
            HStack {
                Image(systemName: "hand.raised.fill").foregroundStyle(.orange)
                Text("Waiting on you: \(blocked.reason ?? "needs your input")").lineLimit(2)
                Spacer()
                Button("Jump to pane") { Task { try? await cli?.focus(agent: slug) } }.controlSize(.small)
            }
            .padding(10)
            .background(.orange.opacity(0.15))
        }
    }

    private func messages(_ selection: Recipient) -> some View {
        let isRoom: Bool = { if case .room = selection { true } else { false } }()
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 8) {
                    if chat.canLoadOlder {
                        ProgressView().controlSize(.small).frame(maxWidth: .infinity)
                            .onAppear { Task { if let kept = await chat.loadOlder() { proxy.scrollTo(kept, anchor: .top) } } }
                    }
                    ForEach(Timeline.entries(chat.items)) { entry in
                        switch entry {
                        case .day(let label): DaySeparator(label: label)
                        case .item(let item): MessageRow(item: item, showSpeaker: isRoom).id(item.id)
                        }
                    }
                    Color.clear.frame(height: 1).id("bottom")
                        .onAppear { chat.atBottom = true }
                        .onDisappear { chat.atBottom = false }
                }
                .padding(16)
            }
            .defaultScrollAnchor(.bottom)
            .id(selection)
            .onChange(of: chat.items.last?.id) {
                if chat.atBottom { proxy.scrollTo("bottom", anchor: .bottom) }
            }
            .overlay(alignment: .bottomTrailing) {
                if !chat.atBottom {
                    Button {
                        withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) { proxy.scrollTo("bottom", anchor: .bottom) }
                    } label: { Image(systemName: "arrow.down.circle.fill").font(.title) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).padding(14)
                    .help("Jump to latest")
                }
            }
            .overlay {
                if let problem = chat.problem {
                    Text(problem).foregroundStyle(.red).padding()
                } else if chat.items.isEmpty && !chat.loading {
                    Text("No messages yet").foregroundStyle(.secondary)
                }
            }
        }
    }
}

/// The window: sidebar and chat. The Dock icon shows only while it is open.
struct ChatWindow: View {
    let menu: MenuState
    let chat: ChatState
    let cli: ProcessCLI?

    var body: some View {
        NavigationSplitView {
            SidebarView(menu: menu, chat: chat)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 340)
        } detail: {
            ChatView(menu: menu, chat: chat, cli: cli)
        }
        .frame(minWidth: 720, minHeight: 480)
        .task(id: menu.snapshot) { await chat.observe() }
        .onAppear {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willCloseNotification)) { note in
            if (note.object as? NSWindow)?.title == "herdbot" { NSApp.setActivationPolicy(.accessory) }
        }
    }
}
```

- [ ] **Step 2: Wire the scene, the menu item and the floor**

```diff
--- a/ui/menubar/Sources/HerdbotMenu/App.swift
+++ b/ui/menubar/Sources/HerdbotMenu/App.swift
@@ -10,6 +10,7 @@
     private let cli: ProcessCLI?
     private let notifier: Notifier
     private let quickEntry: QuickEntryController
+    private let chat: ChatState
     private let hotkeyRegistered: Bool
 
     init() {
@@ -29,9 +30,15 @@
         self.cli = cli
         self.notifier = notifier
         self.quickEntry = quickEntry
+        self.chat = ChatState(menu: state, cli: cli)
     }
 
     var body: some Scene {
+        // Opened from the menu, not at launch.
+        Window("herdbot", id: "chat") {
+            ChatWindow(menu: state, chat: chat, cli: cli)
+        }
+        .defaultLaunchBehavior(.suppressed)
         MenuBarExtra {
             MenuContent(state: state, cli: cli, hotkeyRegistered: hotkeyRegistered, openQuickEntry: { quickEntry.show() })
         } label: {
```

```diff
--- a/ui/menubar/Sources/HerdbotMenu/MenuContent.swift
+++ b/ui/menubar/Sources/HerdbotMenu/MenuContent.swift
@@ -7,6 +7,7 @@
     let cli: ProcessCLI?
     let hotkeyRegistered: Bool
     let openQuickEntry: () -> Void
+    @Environment(\.openWindow) private var openWindow
 
     var body: some View {
         if let problem = state.problem {
@@ -48,6 +49,10 @@
             Text("waiting for herdbot…")
         }
         Divider()
+        Button("Open herdbot") {
+            NSApp.activate(ignoringOtherApps: true)
+            openWindow(id: "chat")
+        }
         Button(hotkeyRegistered ? "Quick entry (⌥Space)" : "Quick entry (⌥Space is taken by another app)") { openQuickEntry() }
         Button("Quit") { NSApplication.shared.terminate(nil) }
     }
```

```diff
--- a/ui/menubar/Package.swift
+++ b/ui/menubar/Package.swift
@@ -3,7 +3,7 @@
 
 let package = Package(
     name: "HerdbotMenu",
-    platforms: [.macOS(.v14)],
+    platforms: [.macOS(.v15)],
     targets: [
         .target(name: "HerdbotKit"),
         .executableTarget(name: "HerdbotMenu", dependencies: ["HerdbotKit"]),
```

```diff
--- a/ui/menubar/Info.plist
+++ b/ui/menubar/Info.plist
@@ -8,7 +8,7 @@
     <key>CFBundlePackageType</key><string>APPL</string>
     <key>CFBundleShortVersionString</key><string>0.1.0</string>
     <key>CFBundleVersion</key><string>1</string>
-    <key>LSMinimumSystemVersion</key><string>14.0</string>
+    <key>LSMinimumSystemVersion</key><string>15.0</string>
     <key>LSUIElement</key><true/>
     <key>NSHighResolutionCapable</key><true/>
 </dict>
```

- [ ] **Step 3: Verify**

Run (from `ui/menubar`): `swift build 2>&1 | grep -E "error|warning: [^s]|Build"`, `timeout 300 ./scripts/test.sh`, `timeout 300 ./scripts/build-app.sh`
Expected: `Build complete!` with no Swift errors or warnings, 45 tests pass, `built build/HerdbotMenu.app`. Do not launch it.

- [ ] **Step 4: Docs**

In `ui/menubar/README.md`:
1. Change "Needs macOS 14+" to "Needs macOS 15+".
2. In the opening paragraph add: "A chat window (menu: Open herdbot) lists your bots and rooms and shows each conversation."
3. Append this section at the end:

````markdown
## Chat window

The menu's "Open herdbot" opens a window: a sidebar of bots and rooms (newest activity first, with a
filter field) and the selected conversation. It reads history through `herdbot history`, so it needs a
herdbot that has it (otherwise the chat says "update herdbot"). Your messages are on the right, the bot's
on the left; rooms name each speaker. A message you sent that the bot has not answered shows lighter,
"queued" until the daemon delivers it and then "working". Scroll up to load older messages; "jump to latest"
appears when you are not at the bottom. The header's "Jump to pane" opens the bot's herdr pane, and a banner
appears when the bot is blocked on you. This window is read-only; sending is the quick-entry panel for now.
The Dock icon shows only while the window is open.

### Manual check (needs a person; install the new herdbot and restart the daemon first)

1. `export HB=$(mktemp -d); for b in alpha beta; do HERDBOT_HOME=$HB herdbot new $b; done; HERDBOT_HOME=$HB herdbot room new crew --members alpha,beta`
   Give alpha a history: append a few JSON lines like `{"at":<unix seconds>,"prompt":"hi","reply":"Hello **there**\n\n```rust\nfn x() {}\n```"}` to `$HB/alpha/turns.jsonl` (use times from today and from two days ago), and a line to `$HB/rooms/crew/transcript.jsonl` such as `{"at":<now>,"speaker":"you","text":"hi crew"}`.
2. `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`. No window appears at launch. Menu → "Open herdbot": the window opens and a Dock icon appears.
3. The sidebar lists Bots and Rooms with avatars, previews and times; type in the filter field and rows narrow. Click alpha: bubbles, bold text, the code block in monospace, "Yesterday"/"Today" separators, scrolled to the bottom.
4. `HERDBOT_HOME=$HB herdbot send alpha "queued one"`: within a second a lighter "queued" bubble appears (the daemon is not delivering for this home). Click crew: speaker names in colour.
5. Add more than 50 items to alpha's `turns.jsonl` and scroll up: older messages load and your place holds. Scroll up, then add a line: "jump to latest" appears, and the view does not jump while you are scrolled up.
6. Close the window: the Dock icon goes away and the menu bar item stays. Reopen from the menu.
7. System Settings → Accessibility → Display: Reduce Transparency and Increase Contrast change the sidebar and window; Reduce Motion removes the jump button's animation. Light mode stays readable.
8. Quit from the menu; `rm -r $HB`.
````

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotMenu/MessageViews.swift ui/menubar/Sources/HerdbotMenu/SidebarView.swift ui/menubar/Sources/HerdbotMenu/ChatView.swift ui/menubar/Sources/HerdbotMenu/App.swift ui/menubar/Sources/HerdbotMenu/MenuContent.swift ui/menubar/Package.swift ui/menubar/Info.plist ui/menubar/README.md
git commit -m "feat(menubar): chat window with a sidebar of bots and rooms"
```

## Self-review

- Spec coverage: sidebar with sections, filter, previews, times, working spinner, dimmed rows (Tasks 2-3); chat bubbles, speakers, notices, day separators, markdown with code blocks, paging, jump to latest, pending items (Tasks 2-3); header with status, subagent chips, Jump to pane, blocked banner (Task 3); window from the menu, Dock icon only while open (Task 3).
- Deviations from the spec: the macOS floor is now 15 (so the window can be suppressed at launch); the sidebar has no unread dots yet (plan 3); the header chips and banner use the snapshot's `runs` and `blocked`, which exist already.
- Not verifiable without a person: Reduce Transparency/Contrast/Motion on the window; scroll position holding after loading older pages; "jump to latest" behaviour with a growing list; whether the Dock icon reliably disappears on close.
