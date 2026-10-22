import Foundation
import Testing
@testable import HerdbotKit

@Suite struct Decoding {
    @Test func snapshotLineDecodes() throws {
        let parsed = try #require(try StreamParser.parse(snapshotLine))
        #expect(parsed.type == "snapshot")
        let snap = try #require(parsed.snapshot)
        #expect(snap.daemon == "up")
        #expect(snap.bots.map(\.slug) == ["alpha"])
        #expect(snap.rooms.map(\.slug) == ["crew"])
        #expect(snap.rooms.first?.mode == "round_robin")
        #expect(snap.runs.first?.agent == "alpha-r1")
        #expect(snap.cooldowns.first?.until == 4102444800)
        #expect(snap.blocked.first?.reason == "approval: Bash")
        #expect(snap.blocked.first?.text == nil)
    }

    @Test func aSnapshotFromAnOlderHerdbotHasNoRooms() throws {
        let old = """
        {"v":1,"type":"snapshot","snapshot":{"v":1,"at":1,"daemon":"up","bots":[],"runs":[],"cooldowns":[],"blocked":[],"warnings":[]}}
        """
        #expect(try #require(try StreamParser.parse(old)?.snapshot).rooms.isEmpty)
    }

    @Test func eventLineDecodes() throws {
        let parsed = try #require(try StreamParser.parse(eventLine))
        #expect(parsed.event?.kind == "turn_end")
        #expect(parsed.event?.text == "done")
    }

    @Test func unknownVersionThrows() {
        #expect(throws: CLIError.unsupported(2)) { try StreamParser.parse(#"{"v":2,"type":"snapshot"}"#) }
    }

    @Test func garbageIsSkipped() throws {
        #expect(try StreamParser.parse("not json") == nil)
        #expect(try StreamParser.parse(#"{"hello":1}"#) == nil)
    }

    @Test func conversationCountsDecodeAndDefaultForAnOlderHerdbot() throws {
        let new = """
        {"v":1,"type":"snapshot","snapshot":{"v":1,"at":1,"daemon":"up",\
        "bots":[{"slug":"a","title":"A","up":true,"items":3,"last_at":7,"preview":"hi","working":true}],\
        "rooms":[{"slug":"c","name":"C","mode":"channel","paused":false,"items":2,"last_at":9,"preview":"yo"}],\
        "runs":[],"cooldowns":[],"blocked":[],"warnings":[]}}
        """
        let snap = try #require(try StreamParser.parse(new)?.snapshot)
        #expect(snap.bots == [BotView(slug: "a", title: "A", up: true, items: 3, lastAt: 7, preview: "hi", working: true)])
        #expect(snap.rooms == [RoomView(slug: "c", name: "C", mode: "channel", paused: false, items: 2, lastAt: 9, preview: "yo")])
        let old = try #require(try StreamParser.parse(snapshotLine)?.snapshot)
        #expect(old.bots == [BotView(slug: "alpha", title: "Alpha", up: true)])
        #expect(old.bots[0].lastAt == nil)
    }

    @Test func historyPageDecodesPersistedAndPendingItems() throws {
        let json = """
        {"v":1,"target":"alpha","total":2,"items":[
        {"n":0,"at":1,"kind":"user","text":"hi"},
        {"n":1,"at":2,"kind":"bot","speaker":"alpha","text":"hello"},
        {"n":null,"at":3,"kind":"user","text":"later","state":"queued"}],"warnings":["w"]}
        """
        let page = try JSONDecoder().decode(HistoryPage.self, from: Data(json.utf8))
        #expect(page.total == 2)
        #expect(page.items.map(\.id) == ["n0", "n1", "p:queued:3:later"])
        #expect(page.items.map(\.isPending) == [false, false, true])
        #expect(page.items[1].speaker == "alpha")
        #expect(page.warnings == ["w"])
    }
}
