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

    @Test func observeRefetchesWhenOnlyTheQueuedCountChanges() async {
        let cli = FakeCLI()
        let (chat, menu) = make(cli)
        await chat.select(.bot("alpha"))
        #expect(cli.historyCalls.count == 1)
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "Alpha", up: true, items: 0, queued: 1)])))
        await chat.observe()
        #expect(cli.historyCalls.count == 2)
        menu.apply(line(snapshot(bots: [BotView(slug: "alpha", title: "Alpha", up: true, items: 0, queued: 1)])))
        await chat.observe()
        #expect(cli.historyCalls.count == 2)
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
