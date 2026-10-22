import Foundation
import Testing
@testable import HerdbotKit

@Suite struct ProcessCLITests {
    @Test func watchStreamsLinesThenEnds() async throws {
        let script = try fakeHerdbot("echo '\(snapshotLine)'\necho 'junk'\necho '\(eventLine)'")
        var types: [String] = []
        do {
            for try await parsed in ProcessCLI(binary: script).watch() { types.append(parsed.type) }
            Issue.record("stream should end with an error")
        } catch {
            #expect(error as? CLIError == .failed("herdbot watch ended"))
        }
        #expect(types == ["snapshot", "event"])
    }

    @Test func watchReportsAnUnknownVersion() async throws {
        let script = try fakeHerdbot(#"echo '{"v":9,"type":"snapshot"}'"#)
        do {
            for try await _ in ProcessCLI(binary: script).watch() {}
            Issue.record("expected an error")
        } catch {
            #expect(error as? CLIError == .unsupported(9))
        }
    }

    @Test func focusPassesTheAgentAndReportsFailure() async throws {
        let ok = try fakeHerdbot(#"[ "$1 $2" = "focus a-1" ]"#)
        try await ProcessCLI(binary: ok).focus(agent: "a-1")
        let bad = try fakeHerdbot("echo 'herdr agent_not_found' >&2; exit 1")
        await #expect(throws: CLIError.failed("herdr agent_not_found")) {
            try await ProcessCLI(binary: bad).focus(agent: "nope")
        }
    }

    @Test func sendPassesTheTextAsOneArgument() async throws {
        let bot = try fakeHerdbot(##"[ "$#" = 3 ] && [ "$1" = send ] && [ "$2" = alpha ] && [ "$3" = "$(printf 'two words "quoted"\nsecond line')" ]"##)
        try await ProcessCLI(binary: bot).send(to: .bot("alpha"), text: "two words \"quoted\"\nsecond line")
        let room = try fakeHerdbot(#"[ "$#" = 4 ] && [ "$1 $2 $3 $4" = "room send crew hi there" ]"#)
        try await ProcessCLI(binary: room).send(to: .room("crew"), text: "hi there")
        let bad = try fakeHerdbot("echo 'no bot alpha' >&2; exit 1")
        await #expect(throws: CLIError.failed("no bot alpha")) {
            try await ProcessCLI(binary: bad).send(to: .bot("alpha"), text: "x")
        }
    }

    @Test func historyAsksForAPageAndDecodesIt() async throws {
        let page = ##"{"v":1,"target":"room:crew","total":1,"items":[{"n":0,"at":1,"kind":"user","text":"hi"}],"warnings":[]}"##
        let newest = try fakeHerdbot("[ \"$*\" = 'history room:crew --json --limit 3' ] && echo '\(page)'")
        let result = try await ProcessCLI(binary: newest).history(of: .room("crew"), before: nil, limit: 3)
        #expect(result.items.map(\.text) == ["hi"])
        let older = try fakeHerdbot("[ \"$*\" = 'history alpha --json --limit 5 --before 9' ] && echo '\(page)'")
        _ = try await ProcessCLI(binary: older).history(of: .bot("alpha"), before: 9, limit: 5)
        let bad = try fakeHerdbot("echo 'no bot \"x\"' >&2; exit 1")
        await #expect(throws: CLIError.failed(#"no bot "x""#)) {
            _ = try await ProcessCLI(binary: bad).history(of: .bot("x"), before: nil, limit: 3)
        }
        let future = try fakeHerdbot(##"echo '{"v":9,"target":"a","total":0,"items":[],"warnings":[]}'"##)
        await #expect(throws: CLIError.unsupported(9)) {
            _ = try await ProcessCLI(binary: future).history(of: .bot("a"), before: nil, limit: 3)
        }
    }

    @Test func aHugePageDoesNotStallTheProcess() async throws {
        let big = try fakeHerdbot(##"printf '{"v":1,"target":"a","total":1,"items":[{"n":0,"at":1,"kind":"bot","text":"'; head -c 300000 /dev/zero | tr '\0' x; printf '"}],"warnings":[]}'"##)
        let result = try await ProcessCLI(binary: big).history(of: .bot("a"), before: nil, limit: 50)
        #expect(result.items[0].text.count == 300_000)
    }

    @Test func locatePrefersHerdbotBin() throws {
        let script = try fakeHerdbot("true")
        #expect(ProcessCLI.locate(environment: ["HERDBOT_BIN": script.path], home: "/nonexistent") == script)
        #expect(ProcessCLI.locate(environment: [:], home: "/nonexistent") == nil)
    }

    /// Only when HERDBOT_BIN points at a real, new-enough herdbot: the live contract check.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["HERDBOT_BIN"] != nil))
    func realBinaryStreamsASnapshotFirst() async throws {
        let binary = try #require(ProcessCLI.locate())
        var first: StreamLine?
        for try await parsed in ProcessCLI(binary: binary).watch() {
            first = parsed
            break
        }
        #expect(first?.type == "snapshot")
        #expect(first?.snapshot?.v == supportedVersion)
    }
}
