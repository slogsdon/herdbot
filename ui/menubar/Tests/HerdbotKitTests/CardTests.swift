import Foundation
import Testing
@testable import HerdbotKit

let cardJSON = #"{"id":"9f3a000000000001","kind":"approval","adapter":"claude-trust","bot":"helper","agent":"helper","title":"Trust this folder?","body":"Accessing workspace:\\n /Users/x/Bots/helper","options":[{"id":"trust","label":"Yes, I trust this folder","style":"default"},{"id":"exit","label":"No, exit","style":"destructive"}],"at":100}"#

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
