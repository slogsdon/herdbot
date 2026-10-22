import Foundation
import Testing
@testable import HerdbotKit

let doctorJSON = """
{"v":1,"version":"0.1.0","checks":[
{"id":"herdr","status":"ok","message":"herdr 0.9.1"},
{"id":"service","status":"fail","message":"the launchd service is not installed","fix":"service"},
{"id":"cli-link","status":"warn","message":"no herdbot on PATH","fix":"cli-link"}]}
"""

@Suite struct DoctorDecoding {
    @Test func decodesChecksAndOptionalFix() throws {
        let report = try JSONDecoder().decode(DoctorReport.self, from: Data(doctorJSON.utf8))
        #expect(report.version == "0.1.0")
        #expect(report.checks.map(\.id) == ["herdr", "service", "cli-link"])
        #expect(report.checks[0].fix == nil)
        #expect(report.checks[1].fix == "service")
        #expect(report.checks.map(\.status) == [.ok, .fail, .warn])
    }

    @Test func anUnknownStatusIsAWarning() throws {
        let json = #"{"v":1,"version":"9","checks":[{"id":"x","status":"fatal","message":"m"}]}"#
        #expect(try JSONDecoder().decode(DoctorReport.self, from: Data(json.utf8)).checks[0].status == .warn)
    }

    @Test func statusesOrderBySeverity() {
        #expect(DoctorStatus.ok < .warn && .warn < .fail)
    }
}
