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

    @Test func eventLinesAreIgnored() throws {
        let state = MenuState()
        state.apply(try #require(try StreamParser.parse(eventLine)))
        #expect(state.snapshot == nil)
    }
}
