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

    @Test func blockedRunStatusIsOptionalAndVisible() throws {
        let json = #"{"v":1,"type":"snapshot","snapshot":{"v":1,"at":1,"daemon":"up","bots":[],"rooms":[],"runs":[{"id":"r1","bot":"alpha","agent":"alpha-r1","route":"codex:gpt","description":"inspect","status":"blocked"}],"cooldowns":[],"blocked":[],"warnings":[]}}"#
        let run = try #require(try StreamParser.parse(json)?.snapshot?.runs.first)
        #expect(run.status == "blocked")
        #expect(try #require(try StreamParser.parse(snapshotLine)?.snapshot?.runs.first).status == nil)
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
        "bots":[{"slug":"a","title":"A","up":true,"items":3,"last_at":7,"preview":"hi","working":true,"queued":2}],\
        "rooms":[{"slug":"c","name":"C","mode":"channel","paused":false,"items":2,"last_at":9,"preview":"yo"}],\
        "runs":[],"cooldowns":[],"blocked":[],"warnings":[]}}
        """
        let snap = try #require(try StreamParser.parse(new)?.snapshot)
        #expect(snap.bots == [BotView(slug: "a", title: "A", up: true, items: 3, lastAt: 7, preview: "hi", working: true, queued: 2)])
        #expect(snap.rooms == [RoomView(slug: "c", name: "C", mode: "channel", paused: false, items: 2, lastAt: 9, preview: "yo")])
        let old = try #require(try StreamParser.parse(snapshotLine)?.snapshot)
        #expect(old.bots == [BotView(slug: "alpha", title: "Alpha", up: true)])
        #expect(old.bots[0].lastAt == nil)
        #expect(old.bots[0].queued == 0)
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

    @Test func historyLifecycleFieldsAreOptionalAndDecodeEveryTerminalState() throws {
        let old = try JSONDecoder().decode(HistoryItem.self, from: Data(#"{"n":0,"at":1,"kind":"notice","text":"legacy"}"#.utf8))
        #expect(old.eventID == nil)
        #expect(old.source == nil)
        #expect(old.detail == nil)
        #expect(old.id == "n0")

        let states = ["completed", "blocked", "failed", "stopped", "expired"]
        for (index, state) in states.enumerated() {
            let detail = String(repeating: "private result ", count: 100)
            let json = """
            {"n":null,"at":2,"id":"run-event-\(index)","kind":"notice","source":"lifecycle","text":"\(state)","state":"\(state)","detail":"\(detail)"}
            """
            let item = try JSONDecoder().decode(HistoryItem.self, from: Data(json.utf8))
            #expect(item.state == state)
            #expect(item.source == "lifecycle")
            #expect(item.detail == detail)
            #expect(item.id == "run-event-\(index)")
        }
    }

    @Test func provenanceStatusLabelsStayTypedAndLegacyTextIsNotInspected() {
        let botMessage = HistoryItem(n: 0, at: 1, kind: "notice", text: "Message from beta", source: "bot_message", detail: "[message from beta]\nPlease inspect this output.")
        #expect(botMessage.statusTitle == "Message from beta")
        #expect(botMessage.detailDisclosureTitle == "Show message")

        let legacy = HistoryItem(n: 1, at: 2, kind: "unknown", text: "[message from beta]\nPlease inspect this output.", source: "unknown", detail: "[message from beta]\nPlease inspect this output.")
        #expect(legacy.statusTitle == "Unknown sender")
        #expect(legacy.detailDisclosureTitle == "Show original text")
    }

    @Test func globalSettingsDecodeMetadataValuesAndApplyTiming() throws {
        let settings = try settings(from: globalSettingsJSON)
        #expect(settings.map(\.key) == ["approvals.mode", "memory.max_compactors", "paths", "cua.host"])

        let approvals = try #require(settings.first)
        #expect(approvals.key == "approvals.mode")
        #expect(approvals.type == .enumeration)
        #expect(approvals.choices == ["ask", "auto"])
        #expect(approvals.defaultValue == .text("ask"))
        #expect(approvals.value == .text("auto"))
        #expect(approvals.apply == .nextBotStart)
        #expect(approvals.description.contains("automatic review"))

        let compactors = settings[1]
        #expect(compactors.type == .integer)
        #expect(compactors.defaultValue == .integer(2))
        #expect(compactors.value == .integer(4))
        #expect(compactors.choices.isEmpty)
        #expect(compactors.apply == .immediate)
    }

    @Test func settingsDecodeTablesAndUnsetValues() throws {
        let settings = try settings(from: globalSettingsJSON)
        let paths = settings[2]
        #expect(paths.type == .map)
        #expect(paths.defaultValue == .table([:]))
        #expect(paths.value == .table(["/workspace/vault": "~/Documents/vault"]))
        #expect(paths.value.text == "/workspace/vault=~/Documents/vault")

        let host = settings[3]
        #expect(host.type == .string)
        #expect(host.defaultValue == .unset)
        #expect(host.value == .unset)
        #expect(host.value.text == "")
    }

    @Test func botSettingsDecodePerBotMetadata() throws {
        let settings = try settings(from: botSettingsJSON)
        #expect(settings.map(\.key) == ["title", "approvals", "memory"])

        let title = settings[0]
        #expect(title.type == .string)
        #expect(title.defaultValue == .text(""))
        #expect(title.value == .text("Helper"))
        #expect(title.apply == .immediate)

        let approvals = settings[1]
        #expect(approvals.choices == ["ask", "auto"])
        #expect(approvals.value == .text("auto"))

        let memory = settings[2]
        #expect(memory.type == .boolean)
        #expect(memory.defaultValue == .flag(false))
        #expect(memory.value == .flag(true))
        #expect(memory.value.text == "true")
    }

    @Test func oneSettingDecodesFromAGetEnvelope() throws {
        let memory = try setting(from: botMemorySettingJSON)
        #expect(memory.key == "memory")
        #expect(memory.type == .boolean)
        #expect(memory.value == .flag(true))
        #expect(memory.defaultValue == .flag(false))
        #expect(memory.apply == .nextBotStart)
    }

    @Test func applyTimingSaysWhenAValueTakesEffect() throws {
        #expect(SettingApply.immediate.label == "Takes effect immediately")
        #expect(SettingApply.nextBotStart.label == "Takes effect when the bot next starts")
        #expect(SettingApply.daemonRestart.label == "Takes effect after the daemon restarts")
    }

    @Test func aSettingsPayloadFromAnotherContractVersionThrows() {
        #expect(throws: CLIError.unsupported(2)) {
            _ = try settings(from: #"{"version":2,"scope":"global","settings":[]}"#)
        }
        #expect(throws: CLIError.unsupported(2)) {
            _ = try setting(from: #"{"version":2,"scope":"global","setting":null}"#)
        }
    }
}
