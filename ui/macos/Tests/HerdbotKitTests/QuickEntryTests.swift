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

    @Test func labelsTheCurrentRecipient() {
        let (state, _, _) = make()
        #expect(state.recipientLabel == "Alpha")
        state.choose(.room("crew"))
        #expect(state.recipientLabel == "Crew (room)")
    }

    @Test func dismissingAFailureKeepsTheText() async {
        let (state, _, _) = make()
        let cli = FakeCLI()
        cli.failure = "boom"
        state.text = "keep me"
        _ = await state.send(using: cli)
        state.dismissFailure()
        #expect(state.status == .idle)
        #expect(state.text == "keep me")
        // Dismissing is only for failures: it must not clear a queued confirmation.
        cli.failure = nil
        _ = await state.send(using: cli)
        state.dismissFailure()
        #expect(state.status == .queued)
    }

    @Test func resetStartsAFreshMessage() async {
        let (state, _, _) = make()
        state.text = "x"
        _ = await state.send(using: FakeCLI())
        state.reset()
        #expect(state.status == .idle)
        #expect(state.text.isEmpty)
    }

    @Test func focusLossOnlyDismissesAfterTheGracePeriod() {
        var policy = QuickEntryFocusPolicy()
        let lost = ContinuousClock.now
        policy.focusLost(at: lost)
        #expect(policy.dismissalDeadline == lost.advanced(by: QuickEntryFocusPolicy.grace))
        #expect(!policy.isDue(at: lost.advanced(by: .milliseconds(299))))
        #expect(policy.isDue(at: lost.advanced(by: QuickEntryFocusPolicy.grace)))
    }

    @Test func returningFocusCancelsTheDismissal() {
        var policy = QuickEntryFocusPolicy()
        let lost = ContinuousClock.now
        policy.focusLost(at: lost)
        policy.focusRegained()
        #expect(policy.dismissalDeadline == nil)
        #expect(!policy.isDue(at: lost.advanced(by: .seconds(5))))
    }

    @Test func repeatedFocusLossDoesNotExtendTheGracePeriod() {
        var policy = QuickEntryFocusPolicy()
        let lost = ContinuousClock.now
        policy.focusLost(at: lost)
        // The app deactivating and the panel resigning key can both report the same departure.
        policy.focusLost(at: lost.advanced(by: .milliseconds(200)))
        #expect(policy.dismissalDeadline == lost.advanced(by: QuickEntryFocusPolicy.grace))
        #expect(policy.isDue(at: lost.advanced(by: QuickEntryFocusPolicy.grace)))
    }

    @Test func aNewLossAfterRegainingStartsAFreshGracePeriod() {
        var policy = QuickEntryFocusPolicy()
        let lost = ContinuousClock.now
        policy.focusLost(at: lost)
        policy.focusRegained()
        let lostAgain = lost.advanced(by: .seconds(1))
        policy.focusLost(at: lostAgain)
        #expect(policy.dismissalDeadline == lostAgain.advanced(by: QuickEntryFocusPolicy.grace))
        #expect(!policy.isDue(at: lostAgain.advanced(by: .milliseconds(299))))
    }
}
