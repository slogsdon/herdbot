import Foundation
import Testing
@testable import HerdbotKit

@MainActor
@Suite struct NotificationTargets {
    func make(bots: [BotView], runs: Int = 0, blocked: [Event] = []) -> ChatState {
        let menu = MenuState()
        menu.apply(line(snapshot(runs: runs, blocked: blocked, bots: bots)))
        return ChatState(menu: menu, cli: FakeCLI())
    }

    let alpha = BotView(slug: "alpha", title: "Alpha", up: true)

    @Test func aSubagentResolvesThroughItsRun() {
        let chat = make(bots: [alpha], runs: 1)
        #expect(chat.bot(forAgent: "alpha-r0") == .bot("alpha"))
    }

    @Test func aSubagentStillResolvesAfterItsRunIsGone() {
        let chat = make(bots: [alpha])
        #expect(chat.bot(forAgent: "alpha-r0") == .bot("alpha"))
        #expect(chat.bot(forAgent: "alpha-main") == .bot("alpha"))
    }

    @Test func theLongestSlugWins() {
        let chat = make(bots: [BotView(slug: "herd", title: "Herd", up: true), BotView(slug: "herd-bot", title: "Herd Bot", up: true)])
        #expect(chat.bot(forAgent: "herd-bot-r7") == .bot("herd-bot"))
        #expect(chat.bot(forAgent: "herd-r7") == .bot("herd"))
    }

    @Test func anUnknownAgentHasNoBot() {
        let chat = make(bots: [alpha])
        #expect(chat.bot(forAgent: "stranger") == nil)
        // A name only sharing the start of a slug is not that bot's subagent.
        #expect(chat.bot(forAgent: "alphabet") == nil)
        #expect(ChatState(menu: MenuState(), cli: nil).bot(forAgent: "alpha") == nil)
    }

    @Test func blockedEventsForABotIncludeItsSubagents() {
        let chat = make(bots: [alpha, BotView(slug: "beta", title: "Beta", up: true)],
                        blocked: [blockedEvent("alpha", at: 1), blockedEvent("alpha-r2", at: 2), blockedEvent("beta", at: 3), blockedEvent("stranger", at: 4)])
        #expect(chat.blockedEvents(for: "alpha").map(\.agent) == ["alpha", "alpha-r2"])
        #expect(chat.blockedEvents(for: "beta").map(\.agent) == ["beta"])
        #expect(chat.blockedEvents(for: "missing").isEmpty)
    }
}

@Suite struct NotificationBodies {
    @Test func theReasonAndBotNameFillTheBody() {
        let event = Event(at: 1, agent: "alpha-r1", kind: "blocked", text: "ignored", reason: "approval: Bash\nsecond line", harness: "claude")
        #expect(NotificationCopy.body(bot: "Alpha", event: event) == "Alpha: approval: Bash")
        #expect(NotificationCopy.body(bot: nil, event: event) == "approval: Bash")
    }

    @Test func theTextStandsInForAMissingReason() {
        let event = Event(at: 1, agent: "alpha", kind: "blocked", text: "\n\n  Waiting for a folder?\nmore", reason: nil, harness: "claude")
        #expect(NotificationCopy.body(bot: "Alpha", event: event) == "Alpha: Waiting for a folder?")
        let emptyReason = Event(at: 1, agent: "alpha", kind: "blocked", text: "what now?", reason: "  ", harness: "claude")
        #expect(NotificationCopy.detail(for: emptyReason) == "what now?")
    }

    @Test func neitherReasonNorTextFallsBackToGenericCopy() {
        let event = Event(at: 1, agent: "alpha", kind: "blocked", text: nil, reason: nil, harness: "claude")
        #expect(NotificationCopy.body(bot: "Alpha", event: event) == NotificationCopy.blockedBody)
        let blank = Event(at: 1, agent: "alpha", kind: "blocked", text: " \n ", reason: "", harness: "claude")
        #expect(NotificationCopy.body(bot: "Alpha", event: blank) == NotificationCopy.blockedBody)
        #expect(NotificationCopy.detail(for: blank) == nil)
    }

    @Test func aLongDetailIsCutToTheLimit() {
        let event = Event(at: 1, agent: "alpha", kind: "blocked", text: nil, reason: String(repeating: "x", count: 300), harness: "claude")
        #expect(NotificationCopy.detail(for: event) == String(repeating: "x", count: NotificationCopy.detailLimit - 1) + "…")
        #expect(NotificationCopy.body(bot: "Alpha", event: event).hasPrefix("Alpha: xxx"))
        #expect(NotificationCopy.body(bot: "Alpha", event: event, limit: 6) == "Alpha: xxxxx…")
    }

    @Test func aCardBodyNamesTheBotAndTitle() {
        #expect(NotificationCopy.body(bot: "Helper", card: card(bot: "helper")) == "Helper: Trust this folder?")
        let untitled = Card(id: "x", kind: "approval", adapter: "a", bot: "helper", agent: "helper", title: " \n", body: "dialog", options: [], at: 1)
        #expect(NotificationCopy.body(bot: "Helper", card: untitled) == NotificationCopy.approvalBody)
    }

    @Test func aBotNameFallsBackToItsSlug() {
        let snap = snapshot(bots: [BotView(slug: "alpha", title: "", up: true), BotView(slug: "beta", title: "Beta", up: true)])
        #expect(snap.botName("alpha") == "alpha")
        #expect(snap.botName("beta") == "Beta")
        #expect(snap.botName("missing") == nil)
    }
}
