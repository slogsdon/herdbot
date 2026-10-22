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
