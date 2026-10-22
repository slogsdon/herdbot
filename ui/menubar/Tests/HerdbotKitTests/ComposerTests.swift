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
