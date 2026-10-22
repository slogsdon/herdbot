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
        #expect(snap.runs.first?.agent == "alpha-r1")
        #expect(snap.cooldowns.first?.until == 4102444800)
        #expect(snap.blocked.first?.reason == "approval: Bash")
        #expect(snap.blocked.first?.text == nil)
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
}
