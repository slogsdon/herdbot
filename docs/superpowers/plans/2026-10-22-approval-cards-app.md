# Approval cards, app (plan 2 of 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pending cards from `herdbot status|watch --json` appear in the owning bot's chat as a card component (an approval card with the real dialog text and one button per choice), notify once, mark the menu bar as "waiting on you", and a click runs `herdbot card answer`. Also: the setup window's "Set up everything" button uses the standard button style.

**Architecture:** Spec: `docs/superpowers/specs/2026-10-22-approval-cards-design.md` ("App (SwiftUI)"). Needs plan 1 (`2026-10-22-approval-cards-cli.md`): `cards` in the snapshot and `herdbot card answer <card-id> <option-id>`. All logic is in `HerdbotKit` with Swift Testing; the views render it. One `CardView` switches on `kind`, with a generic fallback for kinds the app does not know.

**Tech Stack:** Swift 6, SwiftUI, Swift Testing (`./scripts/test.sh`). Package `ui/menubar`. No new dependencies.

## Global Constraints

- The app never sends keys or talks to herdr; it only runs `herdbot card answer <card-id> <option-id>` when the user clicks.
- No automatic answer, no default-answer-on-Return: no option button is the default action, and the destructive option is never the default.
- Decoding is tolerant: a missing `cards` is empty; unknown fields, unknown `kind` and unknown `style` values must not fail the snapshot decode.
- `"v": 1` stays. Existing tests keep passing; the pre-existing `ProcessCLITests` hang was fixed (`11f239f`); run suites with `timeout 120` and report a hang as a hang.
- Do not launch the app, script clicks or keystrokes, or kill a process by name. Never run `herdbot service …`, `setup` without `--only`, `uninstall` or `launchctl`.
- Commits: conventional messages, no AI attribution, explicit `git add` paths, no push.

**Branch/worktree:** from `main` after plan 1 is merged: `git -C ~/Code/herdbot worktree add .worktrees/cards-app -b feature/cards-app main`. Work from `ui/menubar`.

## File map

- Create `Sources/HerdbotKit/Cards.swift` (types, `CardTracker`, `CardAnswers`).
- Modify `Sources/HerdbotKit/Models.swift` (`Snapshot.cards`), `Sources/HerdbotKit/Stream.swift` (`CardCLI`), `Sources/HerdbotKit/ProcessCLI.swift`, `Sources/HerdbotKit/MenuState.swift`, `Sources/HerdbotKit/ChatState.swift`.
- Create `Sources/HerdbotMenu/CardViews.swift`; modify `ChatView.swift`, `MenuContent.swift`, `Notifier.swift`, `App.swift`, `SetupView.swift`.
- Tests: create `Tests/HerdbotKitTests/CardTests.swift`; extend `Fixtures.swift`, `StateTests.swift`.
- Modify `README.md` (this package).

---

### Task 1: Card types, snapshot decoding, CLI call

**Files:** Create `Sources/HerdbotKit/Cards.swift`; modify `Models.swift`, `Stream.swift`, `ProcessCLI.swift`; tests in `CardTests.swift`, `Fixtures.swift`.

**Interfaces:**
- Produces: `struct Card: Codable, Hashable, Sendable, Identifiable { id, kind, adapter, bot, agent, title, body, options: [CardOption], at }`, `struct CardOption: Codable, Hashable, Sendable, Identifiable { id, label, style: CardStyle }`, `enum CardStyle: String { case standard, destructive }` (decodes `"default"` as `.standard`, unknown as `.standard`), `Snapshot.cards: [Card]` (default `[]`), `protocol CardCLI: Sendable { func answerCard(id: String, option: String) async throws }`, `ProcessCLI: CardCLI`.

- [ ] **Step 1: Failing tests** (`CardTests.swift`):

```swift
import Foundation
import Testing
@testable import HerdbotKit

let cardJSON = """
{"id":"9f3a000000000001","kind":"approval","adapter":"claude-trust","bot":"helper","agent":"helper",\
"title":"Trust this folder?","body":"Accessing workspace:\\n /Users/x/Bots/helper",\
"options":[{"id":"trust","label":"Yes, I trust this folder","style":"default"},\
{"id":"exit","label":"No, exit","style":"destructive"}],"at":100}
"""

func card(id: String = "c1", bot: String = "helper", kind: String = "approval", at: Int = 1) -> Card {
    Card(id: id, kind: kind, adapter: "claude-trust", bot: bot, agent: bot, title: "Trust this folder?", body: "dialog",
         options: [CardOption(id: "trust", label: "Yes", style: .standard), CardOption(id: "exit", label: "No, exit", style: .destructive)], at: at)
}

@Suite struct CardDecoding {
    @Test func decodesACard() throws {
        let value = try JSONDecoder().decode(Card.self, from: Data(cardJSON.utf8))
        #expect(value.kind == "approval" && value.bot == "helper" && value.options.count == 2)
        #expect(value.options.map(\.style) == [.standard, .destructive])
    }

    @Test func unknownKindStyleAndFieldsDoNotFail() throws {
        let json = #"{"id":"x","kind":"poll","adapter":"a","bot":"b","agent":"b","title":"t","body":"","options":[{"id":"o","label":"l","style":"fancy","extra":1}],"at":1,"future":true}"#
        let value = try JSONDecoder().decode(Card.self, from: Data(json.utf8))
        #expect(value.kind == "poll")
        #expect(value.options[0].style == .standard)
    }

    @Test func aSnapshotWithoutCardsHasNone() throws {
        let line = #"{"v":1,"type":"snapshot","snapshot":{"v":1,"at":1,"daemon":"up","bots":[],"runs":[],"cooldowns":[],"blocked":[],"warnings":[]}}"#
        #expect(try JSONDecoder().decode(StreamLine.self, from: Data(line.utf8)).snapshot?.cards == [])
    }

    @Test func aSnapshotWithCardsKeepsThem() throws {
        let line = #"{"v":1,"type":"snapshot","snapshot":{"v":1,"at":1,"daemon":"up","bots":[],"runs":[],"cooldowns":[],"blocked":[],"warnings":[],"cards":[\#(cardJSON)]}}"#
        #expect(try JSONDecoder().decode(StreamLine.self, from: Data(line.utf8)).snapshot?.cards.map(\.id) == ["9f3a000000000001"])
    }
}
```

Add to `ProcessCLITests.swift`:

```swift
    @Test func answerCardPassesTheIdAndOption() async throws {
        let script = try fakeHerdbot(#"[ "$*" = "card answer 9f3a trust" ] && echo 'answered trust'"#)
        try await ProcessCLI(binary: script).answerCard(id: "9f3a", option: "trust")
        let bad = try fakeHerdbot("echo 'the prompt changed or is gone; nothing was sent' >&2; exit 1")
        await #expect(throws: CLIError.failed("the prompt changed or is gone; nothing was sent")) {
            try await ProcessCLI(binary: bad).answerCard(id: "x", option: "trust")
        }
    }
```

Add a `FakeCardCLI` to `Fixtures.swift`:

```swift
/// Records card answers; `failure` makes the next one throw.
final class FakeCardCLI: CardCLI, @unchecked Sendable {
    nonisolated(unsafe) var answers: [(String, String)] = []
    nonisolated(unsafe) var failure: String?
    func answerCard(id: String, option: String) async throws {
        if let failure { throw CLIError.failed(failure) }
        answers.append((id, option))
    }
}
```

Also update the `snapshot(...)` fixture helper to take `cards: [Card] = []`. Run `timeout 120 ./scripts/test.sh --filter Card` → FAIL.

- [ ] **Step 2: Implement `Cards.swift` (types)**:

```swift
import Foundation

public enum CardStyle: String, Codable, Sendable {
    case standard
    case destructive

    /// "default" on the wire; anything unknown reads as standard.
    public init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = raw == "destructive" ? .destructive : .standard
    }
}

public struct CardOption: Codable, Hashable, Sendable, Identifiable {
    public let id: String
    public let label: String
    public let style: CardStyle

    public init(id: String, label: String, style: CardStyle = .standard) {
        self.id = id
        self.label = label
        self.style = style
    }
}

/// A prompt waiting for the user (mirrors crates/herdbot-core/src/cards.rs). `kind` picks the view.
public struct Card: Codable, Hashable, Sendable, Identifiable {
    public let id: String
    public let kind: String
    public let adapter: String
    public let bot: String
    public let agent: String
    public let title: String
    public let body: String
    public let options: [CardOption]
    public let at: Int

    public init(id: String, kind: String, adapter: String, bot: String, agent: String, title: String, body: String, options: [CardOption], at: Int) {
        self.id = id
        self.kind = kind
        self.adapter = adapter
        self.bot = bot
        self.agent = agent
        self.title = title
        self.body = body
        self.options = options
        self.at = at
    }
}
```

`Models.swift`: add `public let cards: [Card]` to `Snapshot`, give it an explicit memberwise `init(v:at:daemon:bots:rooms:runs:cooldowns:blocked:warnings:cards: [Card] = [])` (existing callers and the fixture keep working), and read it in the custom decoder with `try c.decodeIfPresent([Card].self, forKey: .cards) ?? []`. `Stream.swift`: add `public protocol CardCLI: Sendable { func answerCard(id: String, option: String) async throws }`. `ProcessCLI.swift`: `extension ProcessCLI: CardCLI { public func answerCard(id: String, option: String) async throws { _ = try await output(["card", "answer", id, option]) } }`.

Run the tests → PASS; run every suite once (`timeout 120` each or the full `./scripts/test.sh`).

- [ ] **Step 3: Commit.**

```bash
git add ui/menubar/Sources/HerdbotKit/Cards.swift ui/menubar/Sources/HerdbotKit/Models.swift ui/menubar/Sources/HerdbotKit/Stream.swift ui/menubar/Sources/HerdbotKit/ProcessCLI.swift ui/menubar/Tests/HerdbotKitTests/CardTests.swift ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift ui/menubar/Tests/HerdbotKitTests/Fixtures.swift
git commit -m "feat(menubar): decode cards and answer them through the CLI"
```

---

### Task 2: Answer state, per-bot cards, notify-once, icon

**Files:** Modify `Sources/HerdbotKit/Cards.swift`, `MenuState.swift`, `ChatState.swift`; extend `CardTests.swift`, `StateTests.swift`.

**Interfaces:**
- Produces: `struct CardTracker { mutating func fresh(_ cards: [Card]) -> [Card] }`; `@MainActor @Observable final class CardAnswers { func state(for card: Card) -> CardAnswerState; func answer(_ card: Card, option: CardOption, using cli: (any CardCLI)?) async }`; `enum CardAnswerState: Equatable { case idle, answering(String), failed(String) }`; `MenuState.onCard: (Card) -> Void`; `MenuState.icon` counts cards with blocked agents; `ChatState.cards(for: Recipient) -> [Card]`.

- [ ] **Step 1: Failing tests:**

```swift
@MainActor
@Suite struct Cards {
    @Test func notifiesOncePerCardIdAndNotOnLaunch() {
        var tracker = CardTracker()
        #expect(tracker.fresh([card(id: "a")]).isEmpty, "the first call only records")
        #expect(tracker.fresh([card(id: "a")]).isEmpty)
        #expect(tracker.fresh([card(id: "a"), card(id: "b")]).map(\.id) == ["b"])
        #expect(tracker.fresh([]).isEmpty)
        #expect(tracker.fresh([card(id: "a")]).map(\.id) == ["a"], "a card that went away and came back is new")
    }

    @Test func menuStateAnnouncesNewCardsOnly() {
        let state = MenuState()
        var announced: [String] = []
        state.onCard = { announced.append($0.id) }
        state.apply(line(snapshot(cards: [])))
        state.apply(line(snapshot(cards: [card(id: "a")])))
        state.apply(line(snapshot(cards: [card(id: "a")])))
        #expect(announced == ["a"])
    }

    @Test func aPendingCardCountsAsWaitingOnYou() {
        let state = MenuState()
        state.apply(line(snapshot(cards: [card(id: "a")])))
        #expect(state.icon == .blocked(1))
        state.apply(line(snapshot(blocked: [blockedEvent("x")], cards: [card(id: "a")])))
        #expect(state.icon == .blocked(2))
        state.apply(line(snapshot()))
        #expect(state.icon == .idle)
    }

    @Test func chatShowsOnlyTheOpenBotsCards() async {
        let menu = MenuState()
        menu.apply(line(snapshot(bots: [BotView(slug: "helper", title: "Helper", up: true), BotView(slug: "other", title: "Other", up: true)],
                                 cards: [card(id: "a", bot: "helper"), card(id: "b", bot: "other")])))
        let chat = ChatState(menu: menu, cli: FakeCLI())
        #expect(chat.cards(for: .bot("helper")).map(\.id) == ["a"])
        #expect(chat.cards(for: .bot("other")).map(\.id) == ["b"])
        #expect(chat.cards(for: .room("crew")).isEmpty)
    }

    @Test func answeringTracksProgressAndFailure() async {
        let answers = CardAnswers()
        let cli = FakeCardCLI()
        let item = card(id: "a")
        #expect(answers.state(for: item) == .idle)
        await answers.answer(item, option: item.options[0], using: cli)
        #expect(cli.answers.count == 1 && cli.answers[0] == ("a", "trust"))
        cli.failure = "the prompt changed or is gone; nothing was sent"
        await answers.answer(item, option: item.options[1], using: cli)
        #expect(answers.state(for: item) == .failed("the prompt changed or is gone; nothing was sent"))
        #expect(cli.answers.count == 1, "a failed answer records nothing")
        await answers.answer(item, option: item.options[0], using: nil)
        #expect(answers.state(for: item) == .failed(CLIError.notFound.localizedDescription))
    }
}
```

Run → FAIL.

- [ ] **Step 2: Implement.**
  - `CardTracker` in `Cards.swift`, a copy of `NotificationTracker`'s logic keyed by `card.id` (first call only records).
  - `MenuState`: `@ObservationIgnored public var onCard: (Card) -> Void = { _ in }`, `private var cardTracker = CardTracker()`; in `apply`, `for card in cardTracker.fresh(snapshot.cards) { onCard(card) }`; `icon`: `let waiting = snapshot.blocked.count + snapshot.cards.count; if waiting > 0 { return .blocked(waiting) }`.
  - `ChatState`: `public func cards(for recipient: Recipient) -> [Card] { guard case .bot(let slug) = recipient else { return [] }; return (menu.snapshot?.cards ?? []).filter { $0.bot == slug } }`.
  - `CardAnswers` (`@MainActor @Observable`): `private var states: [String: CardAnswerState]`; `state(for:)` returns `states[card.id] ?? .idle`; `answer`: ignore when already `.answering`; set `.answering(option.id)`; `guard let cli else { states[id] = .failed(CLIError.notFound.localizedDescription); return }`; `do { try await cli.answerCard(id: card.id, option: option.id) } catch { states[id] = .failed(error.localizedDescription); return }`; on success leave `.answering(option.id)` (the card disappears from the next snapshot; the view shows a spinner until then).
- [ ] **Step 3: Verify** all suites pass.
- [ ] **Step 4: Commit.**

```bash
git add ui/menubar/Sources/HerdbotKit/Cards.swift ui/menubar/Sources/HerdbotKit/MenuState.swift ui/menubar/Sources/HerdbotKit/ChatState.swift ui/menubar/Tests/HerdbotKitTests/CardTests.swift ui/menubar/Tests/HerdbotKitTests/StateTests.swift
git commit -m "feat(menubar): card answer state, per-bot cards and notify-once"
```

---

### Task 3: Views, menu, notification, wiring, button style

**Files:** Create `Sources/HerdbotMenu/CardViews.swift`; modify `ChatView.swift`, `MenuContent.swift`, `Notifier.swift`, `App.swift`, `SetupView.swift`.

- [ ] **Step 1: `CardViews.swift`.**

```swift
import HerdbotKit
import SwiftUI

/// One card in a conversation. `kind` picks the shape; a kind this build does not know is shown generically.
struct CardView: View {
    let card: Card
    let answers: CardAnswers
    let cli: ProcessCLI?

    var body: some View {
        switch card.kind {
        case "approval": ApprovalCardView(card: card, answers: answers, cli: cli)
        default: GenericCardView(card: card, cli: cli)
        }
    }
}

struct ApprovalCardView: View {
    let card: Card
    let answers: CardAnswers
    let cli: ProcessCLI?

    var body: some View {
        let state = answers.state(for: card)
        VStack(alignment: .leading, spacing: 10) {
            Label(card.title, systemImage: "hand.raised.fill").font(.headline).foregroundStyle(.orange)
            ScrollView(.horizontal, showsIndicators: false) {
                Text(card.body).font(.system(.callout, design: .monospaced)).textSelection(.enabled).padding(8)
            }
            .background(.black.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            HStack {
                ForEach(card.options) { option in
                    Button(role: option.style == .destructive ? .destructive : nil) {
                        Task { await answers.answer(card, option: option, using: cli) }
                    } label: { Text(option.label) }
                    .disabled(isAnswering(state))
                }
                if isAnswering(state) { ProgressView().controlSize(.small) }
                Spacer()
                Button("Jump to pane") { Task { try? await cli?.focus(agent: card.agent) } }.controlSize(.small)
            }
            if case .failed(let message) = state {
                Text(message).font(.caption).foregroundStyle(.red).textSelection(.enabled)
            }
        }
        .padding(12)
        .background(.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Approval needed: \(card.title)")
    }

    private func isAnswering(_ state: CardAnswerState) -> Bool {
        if case .answering = state { true } else { false }
    }
}

struct GenericCardView: View {
    let card: Card
    let cli: ProcessCLI?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(card.title).font(.headline)
            if !card.body.isEmpty { Text(card.body).font(.callout).textSelection(.enabled) }
            Button("Jump to pane") { Task { try? await cli?.focus(agent: card.agent) } }.controlSize(.small)
        }
        .padding(12)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}
```

No button is a default action (`.keyboardShortcut(.defaultAction)` must not appear). If `Button(role:)` renders the destructive option in red by default that is fine; do not add any prominent style.

- [ ] **Step 2: Chat.** In `ChatView.messages(_:)`, after the `ForEach` of entries and before `Color.clear…id("bottom")`, add `ForEach(chat.cards(for: selection)) { card in CardView(card: card, answers: answers, cli: cli).id(card.id) }`. `ChatView` takes a new `let answers: CardAnswers`; `ChatWindow` passes it through; `App` owns `private let answers = CardAnswers()`. Keep the existing "Waiting on you" banner for blocked events as is. Make `chat.atBottom` scrolling include cards: also `.onChange(of: chat.cards(for: selection).map(\.id)) { if chat.atBottom { proxy.scrollTo("bottom", anchor: .bottom) } }`.

- [ ] **Step 3: Menu and notification.** `MenuContent`: in the "Waiting on you" section also list `snapshot.cards` as `Button("\(card.bot): \(card.title)") { open the chat on that bot }`, using the same `openWindow(id: "chat")` plus selecting the bot (look at how `Notifier.openChat` in `App.swift` does it and reuse that closure through a new `openChatForBot: (String) -> Void` parameter). The section shows when `!blocked.isEmpty || !cards.isEmpty`. `Notifier`: add `func post(_ card: Card)` mirroring `post(_ event:)` with title `"\(card.bot) needs your approval"`, body `card.title`, `userInfo["agent"] = card.agent`, identifier `"card-\(card.id)"`; the existing click handler already opens the chat for the agent. `App.init`: `state.onCard = { notifier.post($0) }`.

- [ ] **Step 4: Button style.** In `SetupView.swift` remove `.buttonStyle(.borderedProminent)` from "Set up everything" so it matches "Fix" and "Done" (Shane: it still looks different from the other buttons). Keep the spinner, `.disabled` and `.help`.

- [ ] **Step 5: Verify.** `timeout 200 ./scripts/test.sh` passes; `./scripts/build-app.sh` builds with no new warnings. State plainly that the views were compiled and never looked at.
- [ ] **Step 6: Commit.**

```bash
git add ui/menubar/Sources/HerdbotMenu/CardViews.swift ui/menubar/Sources/HerdbotMenu/ChatView.swift ui/menubar/Sources/HerdbotMenu/MenuContent.swift ui/menubar/Sources/HerdbotMenu/Notifier.swift ui/menubar/Sources/HerdbotMenu/App.swift ui/menubar/Sources/HerdbotMenu/SetupView.swift
git commit -m "feat(menubar): approval cards in the chat, menu and notifications"
```

---

### Task 4: Docs and the manual check

**Files:** Modify `ui/menubar/README.md`.

- [ ] **Step 1:** Add a **Cards** section: pending prompts (v1: claude's folder-trust dialog) appear in the bot's chat as a card showing the dialog's real text and one button per choice; clicking runs `herdbot card answer`; nothing is answered automatically and no button is the default; "Jump to pane" is always there; a notification and the menu's "Waiting on you" list announce a new card; unknown card kinds show their title, text and Jump to pane.
- [ ] **Step 2:** Add a **Manual check (for Shane, not the agent)**: with the new herdbot installed and a bot whose directory has no trusted parent (for example `herdbot new demo --class plan --cwd /Users/Shared/carddemo`), send it a message from the chat; expect the bot's conversation to show an "Trust this folder?" card with the dialog text, a notification, and the card persisting until you click; click "Yes, I trust this folder" and expect the card to disappear and the message to be delivered; repeat with "No, exit" and expect the card to come back on the next message. Remove the demo bot afterwards.
- [ ] **Step 3: Commit.**

```bash
git add ui/menubar/README.md
git commit -m "docs(menubar): approval cards"
```

---

## Self-review (done)

- Covers: card model and tolerant decoding (T1), `answer` over the CLI only (T1), notify-once and the waiting icon (T2), per-bot placement (T2-3), the generic fallback for unknown kinds (T3), no default button (T3), the setup button style (T3).
- Names: `Card`, `CardOption`, `CardStyle`, `CardCLI`, `CardTracker`, `CardAnswers`, `CardAnswerState`, `FakeCardCLI`, `CardView`, `ApprovalCardView`, `GenericCardView`.
- Not covered: cards in rooms (a card belongs to a bot), a card for any prompt other than claude's trust dialog (plan 1 has only that adapter).
- Unverified: the visual result of the views (compiled only).
