# Chat Composer, Unread and Notifications (UI phase 3, plan 3 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the chat window a chat client: send from a composer, see unread counts in the sidebar, and open the right chat when you click a blocked notification.

**Architecture:** `ChatState` gains a per-conversation draft and a `send()` that queues through the existing `herdbot send` / `room send` and re-fetches so the message shows at once as queued. A new `UnreadTracker` remembers how many items of each conversation you have seen (a conversation seen for the first time counts as read). A `WindowOpener` hands `openWindow` to the notification handler.

**Tech Stack:** Swift 6, SwiftUI, AppKit, Swift Testing.

Spec: `docs/superpowers/specs/2026-10-22-chat-app-design.md` (The window: Composer, App wiring). Depends on plan 2 (`docs/superpowers/plans/2026-10-22-chat-window.md`) being applied.

## Global Constraints

- Branch `feature/ui-chat`, worktree `/Users/shane/Code/herdbot/.worktrees/ui-chat`. Work only there. Plans 1 and 2 must already be committed on the branch.
- Shane approved committing each task below: explicit paths only (never `git add -A` or `git add .`), the conventional message given, **no AI or tool attribution**, no push, no merge.
- Never run `cargo install`, never restart the daemon, never launch the menu bar app (a running instance belongs to Shane; do not kill it). Never script keystrokes or clicks (System Events) either: they go to whatever window is in front.
- Swift: 4-space indent, no `@State` (a macro whose plugin is missing under Command Line Tools), zero Swift compiler warnings (linker `search path ... not found` warnings are fine).
- Test with `timeout 300 ./scripts/test.sh` from `ui/menubar`, never bare `swift test`. The first test build after a change can take two to three minutes; that is a slow compile, not a hang.
- Nothing is sent without a keypress: dictated text goes into the composer and waits for Return.
- Baseline before this plan: Swift 45 tests pass.
- Keep your ledger in `.superpowers/sdd/chat-composer/progress.md` (untracked; do not commit it).

All code below was compiled and its tests run in a scratch copy, and the composer was exercised against a scratch herdbot home (typing, Return, the queued bubble and the queue file), before this plan was written. Reproduce it exactly; if a step fails, report it, don't redesign.

---

### Task 1: draft, send, unread and agent-to-bot mapping

**Files:**
- Create: `ui/menubar/Sources/HerdbotKit/Unread.swift`, `ui/menubar/Tests/HerdbotKitTests/ComposerTests.swift`
- Modify: `ui/menubar/Sources/HerdbotKit/ChatState.swift`, `ui/menubar/Sources/HerdbotKit/QuickEntry.swift`

**Interfaces:**
- Consumes: plan 2's `ChatState`, `FakeCLI`, `MenuState`, `Recipient`.
- Produces: `ChatState.draft`, `canSend`, `daemonDown`, `send() async -> Bool`, `sendStatus` (`.idle`/`.sending`/`.failed(String)`), `dismissSendFailure()`, `bot(forAgent:) -> Recipient?`; `UnreadTracker(menu:defaults:)` with `unread(_:)`, `sync()`, `markSeen(_:)`; `Recipient.key` is now `public`.

- [ ] **Step 1: Write the failing tests**

Create `ui/menubar/Tests/HerdbotKitTests/ComposerTests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

@MainActor
@Suite struct Composer {
    func make(_ cli: FakeCLI, daemon: String = "up") -> (ChatState, MenuState) {
        let menu = MenuState()
        menu.apply(line(snapshot(daemon: daemon, bots: [BotView(slug: "alpha", title: "Alpha", up: true), BotView(slug: "beta", title: "Beta", up: true)])))
        return (ChatState(menu: menu, cli: cli), menu)
    }

    @Test func sendQueuesTheTrimmedDraftThenShowsItAsPending() async {
        let cli = FakeCLI()
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        chat.draft = "  hello\nthere \n"
        #expect(chat.canSend)
        cli.pending[.bot("alpha")] = [HistoryItem(n: nil, at: 9, kind: "user", text: "hello\nthere", state: "queued")]
        #expect(await chat.send())
        #expect(cli.sent.count == 1)
        #expect(cli.sent[0].0 == .bot("alpha"))
        #expect(cli.sent[0].1 == "hello\nthere")
        #expect(chat.draft.isEmpty)
        // The send re-fetched, so the queued message is on screen without waiting for a snapshot.
        #expect(chat.items.last?.state == "queued")
        #expect(chat.sendStatus == .idle)
    }

    @Test func aFailedSendKeepsTheDraftAndSaysWhy() async {
        let cli = FakeCLI()
        cli.failure = "no bot alpha"
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        chat.draft = "keep me"
        #expect(!(await chat.send()))
        #expect(chat.sendStatus == .failed("no bot alpha"))
        #expect(chat.draft == "keep me")
        chat.dismissSendFailure()
        #expect(chat.sendStatus == .idle)
        // Dismissing is only for failures.
        cli.failure = nil
        _ = await chat.send()
        chat.dismissSendFailure()
        #expect(chat.sendStatus == .idle)
    }

    @Test func eachConversationKeepsItsOwnDraft() async {
        let cli = FakeCLI()
        let (chat, _) = make(cli)
        await chat.select(.bot("alpha"))
        chat.draft = "for alpha"
        await chat.select(.bot("beta"))
        #expect(chat.draft.isEmpty)
        chat.draft = "for beta"
        await chat.select(.bot("alpha"))
        #expect(chat.draft == "for alpha")
    }

    @Test func nothingToSendWithoutASelectionOrText() async {
        let (chat, _) = make(FakeCLI())
        chat.draft = "ignored: nothing is selected"
        #expect(chat.draft.isEmpty)
        #expect(!chat.canSend)
        #expect(!(await chat.send()))
        await chat.select(.bot("alpha"))
        chat.draft = " \n "
        #expect(!chat.canSend)
    }

    @Test func daemonDownWarnsButDoesNotBlockSending() async {
        let cli = FakeCLI()
        let (chat, _) = make(cli, daemon: "down")
        await chat.select(.bot("alpha"))
        chat.draft = "hi"
        #expect(chat.daemonDown)
        #expect(await chat.send())
    }

    @Test func anAgentMapsToItsBot() async {
        let menu = MenuState()
        menu.apply(line(snapshot(runs: 1, bots: [BotView(slug: "a", title: "A", up: true)])))
        let chat = ChatState(menu: menu, cli: FakeCLI())
        #expect(chat.bot(forAgent: "a") == .bot("a"))
        #expect(chat.bot(forAgent: "a-r0") == .bot("a"))
        #expect(chat.bot(forAgent: "stranger") == nil)
    }
}

@MainActor
@Suite struct Unread {
    func make(items: Int, defaults: UserDefaults? = nil) -> (UnreadTracker, MenuState, UserDefaults) {
        let menu = MenuState()
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "A", up: true, items: items)],
                                 rooms: [RoomView(slug: "crew", name: "Crew", mode: "channel", paused: false, items: items)])))
        let store = defaults ?? UserDefaults(suiteName: "unread-\(UUID().uuidString)")!
        return (UnreadTracker(menu: menu, defaults: store), menu, store)
    }

    @Test func existingHistoryIsReadTheFirstTime() {
        let (tracker, _, _) = make(items: 40)
        #expect(tracker.unread(.bot("alpha")) == 0)
        tracker.sync()
        #expect(tracker.unread(.bot("alpha")) == 0)
        #expect(tracker.unread(.room("crew")) == 0)
    }

    @Test func newItemsAreUnreadUntilSeen() {
        let (tracker, menu, _) = make(items: 4)
        tracker.sync()
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "A", up: true, items: 7)])))
        #expect(tracker.unread(.bot("alpha")) == 3)
        tracker.markSeen(.bot("alpha"))
        #expect(tracker.unread(.bot("alpha")) == 0)
        // A conversation that appears later starts as read too.
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "A", up: true, items: 7), BotView(slug: "new", title: "N", up: true, items: 12)])))
        tracker.sync()
        #expect(tracker.unread(.bot("new")) == 0)
    }

    @Test func whatYouHaveSeenSurvivesARestart() {
        let (tracker, menu, store) = make(items: 4)
        tracker.sync()
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "A", up: true, items: 9)])))
        tracker.markSeen(.bot("alpha"))
        let reopened = UnreadTracker(menu: menu, defaults: store)
        #expect(reopened.unread(.bot("alpha")) == 0)
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "A", up: true, items: 10)])))
        #expect(reopened.unread(.bot("alpha")) == 1)
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run (from `ui/menubar`): `timeout 300 ./scripts/test.sh`
Expected: build FAIL (`draft`, `send`, `bot(forAgent:)`, `UnreadTracker` do not exist).

- [ ] **Step 3: Implement**

Create `ui/menubar/Sources/HerdbotKit/Unread.swift`:

```swift
import Foundation
import Observation

/// How many items of each conversation you have seen. A conversation seen for the first time
/// counts as read, so launching the app doesn't light up every bot that already has history.
@MainActor @Observable
public final class UnreadTracker {
    private var seen: [String: Int]
    @ObservationIgnored private let menu: MenuState
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private static let storeKey = "chat.seen"

    public init(menu: MenuState, defaults: UserDefaults = .standard) {
        self.menu = menu
        self.defaults = defaults
        seen = (defaults.dictionary(forKey: Self.storeKey) as? [String: Int]) ?? [:]
    }

    /// New items since you last looked, 0 for a conversation not seen before `sync` ran.
    public func unread(_ recipient: Recipient) -> Int {
        guard let total = items(of: recipient), let known = seen[recipient.key] else { return 0 }
        return max(0, total - known)
    }

    /// Record every conversation the snapshot has that isn't known yet as read.
    public func sync() {
        guard let snapshot = menu.snapshot else { return }
        var changed = false
        let all = snapshot.bots.map { (Recipient.bot($0.slug), $0.items) } + snapshot.rooms.map { (Recipient.room($0.slug), $0.items) }
        for (recipient, total) in all where seen[recipient.key] == nil {
            seen[recipient.key] = total
            changed = true
        }
        if changed { persist() }
    }

    public func markSeen(_ recipient: Recipient) {
        guard let total = items(of: recipient), seen[recipient.key] != total else { return }
        seen[recipient.key] = total
        persist()
    }

    private func items(of recipient: Recipient) -> Int? {
        switch recipient {
        case .bot(let slug): menu.snapshot?.bots.first { $0.slug == slug }?.items
        case .room(let slug): menu.snapshot?.rooms.first { $0.slug == slug }?.items
        }
    }

    private func persist() { defaults.set(seen, forKey: Self.storeKey) }
}
```

```diff
--- a/ui/menubar/Sources/HerdbotKit/ChatState.swift
+++ b/ui/menubar/Sources/HerdbotKit/ChatState.swift
@@ -25,11 +25,64 @@
         let working: Bool
     }
 
+    public enum SendStatus: Equatable, Sendable {
+        case idle
+        case sending
+        case failed(String)
+    }
+
+    public private(set) var sendStatus: SendStatus = .idle
+    private var drafts: [Recipient: String] = [:]
+
     public init(menu: MenuState, cli: (any HerdbotCLI)?) {
         self.menu = menu
         self.cli = cli
     }
 
+    /// What you are typing to the open conversation; each conversation keeps its own.
+    public var draft: String {
+        get { selection.flatMap { drafts[$0] } ?? "" }
+        set { if let selection { drafts[selection] = newValue } }
+    }
+
+    public var canSend: Bool {
+        selection != nil && cli != nil && sendStatus != .sending
+            && !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
+    }
+
+    public var daemonDown: Bool { menu.snapshot?.daemon == "down" }
+
+    /// Queue the draft for the open conversation, then re-fetch so it shows at once as queued.
+    /// Never called by dictation: only the person's Return. A failure keeps the draft.
+    @discardableResult
+    public func send() async -> Bool {
+        guard canSend, let cli, let target = selection else { return false }
+        let message = draft.trimmingCharacters(in: .whitespacesAndNewlines)
+        sendStatus = .sending
+        do {
+            try await cli.send(to: target, text: message)
+            drafts[target] = ""
+            sendStatus = .idle
+            await refresh()
+            return true
+        } catch {
+            sendStatus = .failed(error.localizedDescription)
+            return false
+        }
+    }
+
+    public func dismissSendFailure() {
+        if case .failed = sendStatus { sendStatus = .idle }
+    }
+
+    /// The bot a blocked event's agent belongs to: a bot's main agent is its slug, a subagent's
+    /// agent maps to the run's bot. nil for an agent herdbot doesn't know.
+    public func bot(forAgent agent: String) -> Recipient? {
+        guard let snapshot = menu.snapshot else { return nil }
+        if snapshot.bots.contains(where: { $0.slug == agent }) { return .bot(agent) }
+        return snapshot.runs.first { $0.agent == agent }.map { .bot($0.bot) }
+    }
+
     /// Persisted items oldest first, then pending ones.
     public var items: [HistoryItem] { persisted + pending }
 
```

```diff
--- a/ui/menubar/Sources/HerdbotKit/QuickEntry.swift
+++ b/ui/menubar/Sources/HerdbotKit/QuickEntry.swift
@@ -7,7 +7,7 @@
     case room(String)
 
     /// Stable text form, for remembering the last choice.
-    var key: String {
+    public var key: String {
         switch self {
         case .bot(let slug): "bot:\(slug)"
         case .room(let slug): "room:\(slug)"
```

- [ ] **Step 4: Run to see it pass**

Run: `timeout 300 ./scripts/test.sh`
Expected: PASS, 54 tests (45 plus 9 new).

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotKit/Unread.swift ui/menubar/Sources/HerdbotKit/ChatState.swift ui/menubar/Sources/HerdbotKit/QuickEntry.swift ui/menubar/Tests/HerdbotKitTests/ComposerTests.swift
git commit -m "feat(menubar): chat drafts, sending and unread tracking"
```

---

### Task 2: composer, unread badges and notification-to-chat

No unit tests: this is SwiftUI and AppKit wiring. The gate is a warning-free build and a bundle; a person checks it (Task 3).

**Files:**
- Create: `ui/menubar/Sources/HerdbotMenu/WindowOpener.swift`
- Modify: `ui/menubar/Sources/HerdbotMenu/ChatView.swift`, `SidebarView.swift`, `App.swift`, `Notifier.swift`

**Interfaces:**
- Consumes: Task 1's `ChatState` composer API and `UnreadTracker`.
- Produces: a composer pinned under the chat (Return sends); unread counts as accent capsules in the sidebar; a blocked-notification click that opens that bot's chat (or focuses the pane for an agent herdbot can't place).

Design notes the code encodes (do not change them without asking):
- The composer is a multi-line `TextField` like the quick-entry panel: Return sends, the system's own newline key adds a line, the note above it shows "herdbot not found", a send error, or the daemon-down warning. Focus is taken when a conversation opens, after a short wait so the window is key first.
- What is on screen counts as read only while the window is the active scene (`scenePhase == .active`); a conversation first seen counts as read.
- `WindowOpener` exists because `openWindow` is only available inside a view: the menu bar item's icon (always alive) hands it over.

- [ ] **Step 1: Add `WindowOpener.swift`**

`ui/menubar/Sources/HerdbotMenu/WindowOpener.swift`:

```swift
import SwiftUI

/// `openWindow` only exists inside a view. A view that is always alive (the menu bar item's icon)
/// hands it over here, so a notification click can open the chat window too.
@MainActor
final class WindowOpener {
    var open: () -> Void = {}
}

struct OpenerCapture: ViewModifier {
    let opener: WindowOpener
    @Environment(\.openWindow) private var openWindow

    func body(content: Content) -> some View {
        content.task { opener.open = { openWindow(id: "chat") } }
    }
}
```

- [ ] **Step 2: Apply the view and app changes**

```diff
--- a/ui/menubar/Sources/HerdbotMenu/ChatView.swift
+++ b/ui/menubar/Sources/HerdbotMenu/ChatView.swift
@@ -15,6 +15,7 @@
                 Divider()
                 banner(selection)
                 messages(selection)
+                Composer(chat: chat, title: info(for: selection).title, hasHerdbot: cli != nil)
             }
         } else {
             Text("Choose a bot or room").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
@@ -118,21 +119,72 @@
     }
 }
 
+/// Where you type. Return sends; the system's own newline key adds a line. Nothing is sent without
+/// a keypress, so dictated text can be read first.
+struct Composer: View {
+    let chat: ChatState
+    let title: String
+    let hasHerdbot: Bool
+    @FocusState private var focused: Bool
+
+    var body: some View {
+        VStack(alignment: .leading, spacing: 4) {
+            note
+            TextField("Message \(title)", text: Binding(get: { chat.draft }, set: { chat.draft = $0 }), axis: .vertical)
+                .textFieldStyle(.plain)
+                .font(.system(size: 15))
+                .lineLimit(1...6)
+                .focused($focused)
+                .disabled(chat.sendStatus == .sending)
+                .onSubmit { Task { await chat.send() } }
+                .onChange(of: chat.draft) { chat.dismissSendFailure() }
+                .padding(.horizontal, 14)
+                .padding(.vertical, 10)
+                .background(.quaternary, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
+        }
+        .padding(.horizontal, 16)
+        .padding(.vertical, 10)
+        // Take focus when a conversation opens; the short wait lets the window become key first.
+        .task(id: chat.selection) {
+            try? await Task.sleep(for: .milliseconds(80))
+            focused = true
+        }
+    }
+
+    @ViewBuilder private var note: some View {
+        if !hasHerdbot {
+            Text("herdbot not found").font(.caption).foregroundStyle(.red)
+        } else if case .failed(let message) = chat.sendStatus {
+            Text(message).font(.caption).foregroundStyle(.red).lineLimit(2)
+        } else if chat.daemonDown {
+            Text("daemon not running; messages wait until it is").font(.caption).foregroundStyle(.orange)
+        }
+    }
+}
+
 /// The window: sidebar and chat. The Dock icon shows only while it is open.
 struct ChatWindow: View {
     let menu: MenuState
     let chat: ChatState
+    let unread: UnreadTracker
     let cli: ProcessCLI?
+    @Environment(\.scenePhase) private var scenePhase
 
     var body: some View {
         NavigationSplitView {
-            SidebarView(menu: menu, chat: chat)
+            SidebarView(menu: menu, chat: chat, unread: unread)
                 .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 340)
         } detail: {
             ChatView(menu: menu, chat: chat, cli: cli)
         }
         .frame(minWidth: 720, minHeight: 480)
-        .task(id: menu.snapshot) { await chat.observe() }
+        .task(id: menu.snapshot) {
+            await chat.observe()
+            unread.sync()
+            markSelectionSeen()
+        }
+        .onChange(of: chat.selection) { markSelectionSeen() }
+        .onChange(of: scenePhase) { markSelectionSeen() }
         .onAppear {
             NSApp.setActivationPolicy(.regular)
             NSApp.activate(ignoringOtherApps: true)
@@ -141,4 +193,9 @@
             if (note.object as? NSWindow)?.title == "herdbot" { NSApp.setActivationPolicy(.accessory) }
         }
     }
+
+    /// What is on screen counts as read: only while this window is the active one.
+    private func markSelectionSeen() {
+        if scenePhase == .active, let selection = chat.selection { unread.markSeen(selection) }
+    }
 }
```

```diff
--- a/ui/menubar/Sources/HerdbotMenu/SidebarView.swift
+++ b/ui/menubar/Sources/HerdbotMenu/SidebarView.swift
@@ -16,18 +16,28 @@
 
 private struct Row: View {
     let row: SidebarRow
+    let unread: Int
 
     var body: some View {
         HStack(spacing: 10) {
             AvatarView(slug: row.slug, title: row.title)
             VStack(alignment: .leading, spacing: 1) {
-                Text(row.title).font(.body.weight(.medium)).lineLimit(1)
+                Text(row.title).font(.body.weight(unread > 0 ? .bold : .medium)).lineLimit(1)
                 Text(row.preview.isEmpty ? " " : row.preview).font(.caption).foregroundStyle(.secondary).lineLimit(1)
             }
             Spacer(minLength: 4)
             VStack(alignment: .trailing, spacing: 3) {
                 if let at = row.lastAt { Text(time(at)).font(.caption2).foregroundStyle(.secondary) }
-                if row.working { ProgressView().controlSize(.mini) }
+                if unread > 0 {
+                    Text(unread > 99 ? "99+" : "\(unread)")
+                        .font(.caption2.weight(.semibold))
+                        .foregroundStyle(.white)
+                        .padding(.horizontal, 6)
+                        .padding(.vertical, 1)
+                        .background(Color.accentColor, in: Capsule())
+                } else if row.working {
+                    ProgressView().controlSize(.mini)
+                }
             }
         }
         .opacity(row.dimmed ? 0.55 : 1)
@@ -46,15 +56,16 @@
 struct SidebarView: View {
     let menu: MenuState
     let chat: ChatState
+    let unread: UnreadTracker
 
     var body: some View {
         let rows = menu.snapshot.map { Sidebar.rows(from: $0, filter: chat.filter) } ?? (bots: [], rooms: [])
         List(selection: Binding(get: { chat.selection }, set: { choice in Task { await chat.select(choice) } })) {
             if !rows.bots.isEmpty {
-                Section("Bots") { ForEach(rows.bots) { Row(row: $0).tag(Optional($0.id)) } }
+                Section("Bots") { ForEach(rows.bots) { Row(row: $0, unread: unread.unread($0.id)).tag(Optional($0.id)) } }
             }
             if !rows.rooms.isEmpty {
-                Section("Rooms") { ForEach(rows.rooms) { Row(row: $0).tag(Optional($0.id)) } }
+                Section("Rooms") { ForEach(rows.rooms) { Row(row: $0, unread: unread.unread($0.id)).tag(Optional($0.id)) } }
             }
         }
         .searchable(text: Binding(get: { chat.filter }, set: { chat.filter = $0 }), placement: .sidebar, prompt: "Filter")
```

```diff
--- a/ui/menubar/Sources/HerdbotMenu/App.swift
+++ b/ui/menubar/Sources/HerdbotMenu/App.swift
@@ -11,6 +11,8 @@
     private let notifier: Notifier
     private let quickEntry: QuickEntryController
     private let chat: ChatState
+    private let unread: UnreadTracker
+    private let opener = WindowOpener()
     private let hotkeyRegistered: Bool
 
     init() {
@@ -30,19 +32,30 @@
         self.cli = cli
         self.notifier = notifier
         self.quickEntry = quickEntry
-        self.chat = ChatState(menu: state, cli: cli)
+        let chat = ChatState(menu: state, cli: cli)
+        self.chat = chat
+        unread = UnreadTracker(menu: state)
+        let opener = self.opener
+        // A blocked notification opens that bot's chat; an agent we can't place still focuses its pane.
+        notifier.openChat = { agent in
+            guard let bot = chat.bot(forAgent: agent) else { return false }
+            Task { await chat.select(bot) }
+            NSApp.activate(ignoringOtherApps: true)
+            opener.open()
+            return true
+        }
     }
 
     var body: some Scene {
         // Opened from the menu, not at launch.
         Window("herdbot", id: "chat") {
-            ChatWindow(menu: state, chat: chat, cli: cli)
+            ChatWindow(menu: state, chat: chat, unread: unread, cli: cli)
         }
         .defaultLaunchBehavior(.suppressed)
         MenuBarExtra {
             MenuContent(state: state, cli: cli, hotkeyRegistered: hotkeyRegistered, openQuickEntry: { quickEntry.show() })
         } label: {
-            MenuIcon(icon: state.icon)
+            MenuIcon(icon: state.icon).modifier(OpenerCapture(opener: opener))
         }
     }
 }
```

```diff
--- a/ui/menubar/Sources/HerdbotMenu/Notifier.swift
+++ b/ui/menubar/Sources/HerdbotMenu/Notifier.swift
@@ -1,12 +1,15 @@
 import HerdbotKit
 import UserNotifications
 
-/// Posts a notification per newly blocked agent; clicking one focuses that agent's pane.
+/// Posts a notification per newly blocked agent; clicking one opens that bot's chat, or focuses the
+/// agent's pane when it isn't a bot herdbot knows.
 /// Needs an app bundle: a bare `swift run` binary has no bundle id, so it stays silent.
 @MainActor
 final class Notifier: NSObject, UNUserNotificationCenterDelegate {
     private let cli: ProcessCLI?
     private let enabled = Bundle.main.bundleIdentifier != nil
+    /// Opens the chat for an agent and says whether it could; set once the chat exists.
+    var openChat: (String) -> Bool = { _ in false }
 
     init(cli: ProcessCLI?) {
         self.cli = cli
@@ -40,10 +43,11 @@
         didReceive response: UNNotificationResponse
     ) async {
         guard let agent = response.notification.request.content.userInfo["agent"] as? String else { return }
-        await focus(agent)
+        await MainActor.run { openOrFocus(agent) }
     }
 
-    private func focus(_ agent: String) async {
-        try? await cli?.focus(agent: agent)
+    private func openOrFocus(_ agent: String) {
+        if openChat(agent) { return }
+        Task { try? await cli?.focus(agent: agent) }
     }
 }
```

- [ ] **Step 3: Verify**

Run (from `ui/menubar`): `swift build 2>&1 | grep -E "error|warning: [^s]|Build"`, `timeout 300 ./scripts/test.sh`, `timeout 300 ./scripts/build-app.sh`
Expected: `Build complete!` with no Swift errors or warnings, 54 tests pass, `built build/HerdbotMenu.app`. Do not launch it.

- [ ] **Step 4: Commit**

```bash
git add ui/menubar/Sources/HerdbotMenu/WindowOpener.swift ui/menubar/Sources/HerdbotMenu/ChatView.swift ui/menubar/Sources/HerdbotMenu/SidebarView.swift ui/menubar/Sources/HerdbotMenu/App.swift ui/menubar/Sources/HerdbotMenu/Notifier.swift
git commit -m "feat(menubar): chat composer, unread badges, notification opens chat"
```

---

### Task 3: docs and the manual check

**Files:**
- Modify: `ui/menubar/README.md`, `README.md`, `docs/superpowers/plans/README.md`, `docs/superpowers/specs/2026-10-22-chat-app-design.md`

- [ ] **Step 1: `ui/menubar/README.md`**

In the "## Chat window" section (added by plan 2): replace its last two sentences ("This window is read-only; sending is the quick-entry panel for now. The Dock icon shows only while the window is open.") with:

> Type in the composer at the bottom and press Return to send: it runs `herdbot send` or `herdbot room send`, and the message shows at once as "queued", then "working" once delivered. Each conversation keeps its own draft. Nothing is sent without Return, so dictated text can be read first. Conversations with new messages show an unread count in the sidebar; what you have open in the active window counts as read. Clicking a blocked notification opens that bot's chat (use "Jump to pane" there to answer in herdr). The Dock icon shows only while the window is open.

Then insert these steps into that section's manual check list before its final "Quit" step, and renumber the Quit step to 13:

> 9. Composer: select a bot, type `hello "quoted" world`, press Return. A lighter "queued" bubble appears and `cat $HB/.queue/alpha.jsonl` shows the text. Select another bot and back: each keeps its draft. Type right after selecting a conversation: the first characters are not lost (the field has focus).
> 10. Unread: with the window open on alpha, `HERDBOT_HOME=$HB herdbot send beta "x"` does nothing to unread (queued items are not counted), but appending a line to `$HB/beta/turns.jsonl` gives beta an unread count in the sidebar; clicking beta clears it. A bot with history you have never opened starts at no count.
> 11. Notification: append `{"at":1,"agent":"alpha","kind":"blocked","reason":"approval: demo","harness":"claude"}` to `$HB/.state/events/alpha.jsonl` (create the directory). Click the notification: the window opens on alpha with the blocked banner. Clear it with a later `turn_end` line.
> 12. Dictation (MacWhisper, hold Right ⌘): click in the composer, dictate, read the text, press Return.

- [ ] **Step 2: root `README.md`**

In the status paragraph where the menu bar app is mentioned, add "and a chat window for your bots and rooms" after "a ⌥Space quick-entry panel".

- [ ] **Step 3: plans index and spec**

In `docs/superpowers/plans/README.md` add three rows after the phase 2 rows:

`| [Desktop UI, phase 3.1: chat data seam](2026-10-22-chat-data-seam.md) ([spec](../specs/2026-10-22-chat-app-design.md)) | done (committed on feature/ui-chat), 2026-10-22 | `herdbot-core` (`history.rs`, `rooms.rs`, `home.rs`, `snapshot.rs`), `main.rs`, `daemon.rs`, README |`
`| [Desktop UI, phase 3.2: chat window](2026-10-22-chat-window.md) | done (committed on feature/ui-chat), 2026-10-22 | `ui/menubar` sidebar, chat view, history models; macOS floor 15 |`
`| [Desktop UI, phase 3.3: composer, unread, notifications](2026-10-22-chat-composer.md) | done (committed on feature/ui-chat), 2026-10-22 | `ui/menubar` composer, unread tracker, notifier |`

In the chat app spec, change its first line under the title to "Approved 2026-10-22; built across three plans (data seam, window, composer)." (it currently reads "Approved 2026-10-22. Follows ..."; keep the rest of that sentence).

- [ ] **Step 4: Verify and commit**

Run: `git diff --stat` (only these four files), `timeout 300 ./scripts/test.sh` from `ui/menubar`.

```bash
git add ui/menubar/README.md README.md docs/superpowers/plans/README.md docs/superpowers/specs/2026-10-22-chat-app-design.md
git commit -m "docs: document the chat composer, unread counts and notifications"
```

## Self-review

- Spec coverage: composer (Return sends, never auto-sends, failure keeps the draft, daemon-down warning), pending bubbles via the re-fetch after send, unread counts with first-seen-is-read, notification click opens the chat, per-conversation drafts (Tasks 1-2).
- Not verified: the first characters typed right after selecting a conversation were lost in one scratch run (a scripted keystroke test typed `composer test message` and the field held `mposer test message`); the focus is taken after an 80 ms wait on selection, which may be too early or late. Task 3's step 9 checks it; if it reproduces, report it rather than changing the design.
- Not verifiable without a person: unread behaviour while the window is in the background, the notification click path (needs the notification permission and a real blocked event), and dictation into the composer.
