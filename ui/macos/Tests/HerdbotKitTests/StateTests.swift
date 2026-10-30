import Foundation
import Testing
@testable import HerdbotKit

@Suite struct Tracking {
    @Test func firstCallOnlyRecords() {
        var tracker = NotificationTracker()
        #expect(tracker.fresh([blockedEvent("a")]).isEmpty)
        #expect(tracker.fresh([blockedEvent("a")]).isEmpty)
    }

    @Test func announcesNewBlocksOnce() {
        var tracker = NotificationTracker()
        _ = tracker.fresh([])
        #expect(tracker.fresh([blockedEvent("a")]).map(\.agent) == ["a"])
        #expect(tracker.fresh([blockedEvent("a")]).isEmpty)
        #expect(tracker.fresh([]).isEmpty)
        // The same agent blocking again later is a new event.
        #expect(tracker.fresh([blockedEvent("a", at: 9)]).map(\.agent) == ["a"])
    }
}

@MainActor
@Suite struct State {
    @Test func iconFollowsTheSnapshot() {
        let state = MenuState()
        #expect(state.icon == .unavailable)
        state.apply(line(snapshot()))
        #expect(state.icon == .idle)
        state.apply(line(snapshot(runs: 1)))
        #expect(state.icon == .running)
        state.apply(line(snapshot(daemon: "down")))
        #expect(state.icon == .daemonDown)
        state.apply(line(snapshot(daemon: "down", blocked: [blockedEvent("a"), blockedEvent("b")])))
        #expect(state.icon == .blocked(2))
        state.fail("boom")
        #expect(state.icon == .unavailable)
        #expect(state.problem == "boom")
    }

    @Test func onBlockedFiresForNewBlocksOnly() {
        let state = MenuState()
        var announced: [String] = []
        state.onBlocked = { announced.append($0.agent) }
        state.apply(line(snapshot(blocked: [blockedEvent("old")])))
        state.apply(line(snapshot(blocked: [blockedEvent("old"), blockedEvent("new")])))
        #expect(announced == ["new"])
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

    @Test func eventLinesAreIgnored() throws {
        let state = MenuState()
        state.apply(try #require(try StreamParser.parse(eventLine)))
        #expect(state.snapshot == nil)
    }

    @Test func attentionReplacesQuietIconsButNotBlockedOrUnavailable() {
        #expect(IconState.idle.with(attention: true) == .attention)
        #expect(IconState.running.with(attention: true) == .attention)
        #expect(IconState.daemonDown.with(attention: true) == .attention)
        #expect(IconState.blocked(2).with(attention: true) == .blocked(2))
        #expect(IconState.unavailable.with(attention: true) == .unavailable)
        #expect(IconState.idle.with(attention: false) == .idle)
    }
}

@MainActor
@Suite struct MenuBots {
    private func bot(_ slug: String, _ title: String, at lastAt: Int? = nil) -> BotView {
        BotView(slug: slug, title: title, up: true, lastAt: lastAt)
    }

    @Test func anEmptyRosterShowsNothing() {
        let state = MenuState()
        #expect(state.topBots.isEmpty)
        state.apply(line(snapshot(bots: [])))
        #expect(state.topBots.isEmpty)
        #expect(Sidebar.topBots(from: snapshot(bots: [])).isEmpty)
    }

    @Test func aRosterOfFiveShowsEveryBotMostRecentFirst() {
        let state = MenuState()
        state.apply(line(snapshot(bots: [
            bot("alpha", "Alpha", at: 1),
            bot("bravo", "Bravo", at: 5),
            bot("charlie", "Charlie", at: 3),
            bot("delta", "Delta", at: 9),
            bot("echo", "Echo", at: 7),
        ])))
        #expect(state.topBots.map(\.slug) == ["delta", "echo", "bravo", "charlie", "alpha"])
    }

    @Test func overFiveKeepsOnlyTheFiveMostRecent() {
        let state = MenuState()
        state.apply(line(snapshot(bots: [
            bot("a", "A", at: 1),
            bot("b", "B", at: 7),
            bot("c", "C", at: 3),
            bot("d", "D", at: 9),
            bot("e", "E", at: 5),
            bot("f", "F", at: 8),
            bot("g", "G", at: 2),
        ])))
        #expect(state.topBots.map(\.slug) == ["d", "f", "b", "e", "c"])
    }

    @Test func botsWithoutActivitySortAfterActiveOnes() {
        let state = MenuState()
        state.apply(line(snapshot(bots: [
            bot("zed", "Zed"),
            bot("amy", "Amy"),
            bot("old", "Old", at: 2),
            bot("new", "New", at: 4),
            bot("bob", ""),
        ])))
        #expect(state.topBots.map(\.slug) == ["new", "old", "amy", "bob", "zed"])
    }

    @Test func tiesOnActivitySortByDisplayNameThenSlug() {
        let state = MenuState()
        state.apply(line(snapshot(bots: [
            bot("zulu", "Amy", at: 4),
            bot("bravo", "Bob", at: 4),
            bot("alpha", "Amy", at: 4),
        ])))
        #expect(state.topBots.map(\.slug) == ["alpha", "zulu", "bravo"])
    }

    @Test func missingTitlesFallBackToSlugWhenTimestampsTie() {
        let state = MenuState()
        state.apply(line(snapshot(bots: [
            bot("zeta", "Alpha"),
            bot("yen", ""),
            bot("xray", ""),
        ])))
        // Empty titles sort by slug (xray before yen), not as an empty display name.
        #expect(state.topBots.map(\.slug) == ["zeta", "xray", "yen"])
    }

    @Test func selectingATopBotTargetsThatBot() async {
        let state = MenuState()
        state.apply(line(snapshot(bots: [
            bot("helper", "Helper", at: 2),
            bot("atlas", "Atlas", at: 6),
        ])))
        let chat = ChatState(menu: state, cli: FakeCLI())
        // Each row is a Button calling openChatForBot(bot.slug), which selects .bot(slug).
        for slug in state.topBots.map(\.slug) {
            await chat.select(.bot(slug))
            #expect(chat.selection == .bot(slug))
        }
        #expect(state.topBots.first?.slug == "atlas")
    }

    @Test func botsOutsideTheTopFiveStayInTheChatSidebar() {
        let snap = snapshot(bots: [
            bot("a", "A", at: 9),
            bot("b", "B", at: 8),
            bot("c", "C", at: 7),
            bot("d", "D", at: 6),
            bot("e", "E", at: 5),
            bot("hidden", "Hidden", at: 4),
        ])
        let state = MenuState()
        state.apply(line(snap))
        #expect(state.topBots.count == 5)
        #expect(!state.topBots.map(\.slug).contains("hidden"))
        #expect(Sidebar.rows(from: snap).bots.map(\.id).contains(.bot("hidden")))
    }
}
