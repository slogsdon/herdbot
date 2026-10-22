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
