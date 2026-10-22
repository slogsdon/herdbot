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

@MainActor
@Suite struct Setup {
    @Test func worstStatusAndAttention() async {
        let state = SetupState(cli: FakeSetupCLI([report(okCheck, warnLink, failService)]))
        #expect(!state.needsAttention)
        await state.refresh()
        #expect(state.worst == .fail && state.needsAttention)
        let healthy = SetupState(cli: FakeSetupCLI([report(okCheck)]))
        await healthy.refresh()
        #expect(!healthy.needsAttention)
    }

    @Test func fixRunsTheSetupStepThenRefreshes() async {
        let cli = FakeSetupCLI([report(failService), report(okCheck)])
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fix("service")
        #expect(cli.fixes == ["service"])
        #expect(!state.needsAttention)
        #expect(state.fixing.isEmpty)
    }

    @Test func aFailedFixKeepsItsErrorAndTheCheck() async {
        let cli = FakeSetupCLI([report(failService)])
        cli.failure = "service: build output"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fix("service")
        #expect(state.fixError == "service: service: build output")
        #expect(state.needsAttention)
    }

    @Test func fixAllRunsSetupWithNoOnlyFlag() async {
        let cli = FakeSetupCLI([report(failService), report(okCheck)])
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fixAll()
        #expect(cli.fixes.count == 1 && cli.fixes[0] == nil)
    }

    @Test func fixAllAlsoRegistersAnUnregisteredLoginItem() async {
        let cli = FakeSetupCLI([report(failService), report(okCheck)])
        var status = LoginItemStatus.notRegistered
        var registered = 0
        let control = LoginItemControl(status: { status }, register: { registered += 1; status = .enabled })
        let state = SetupState(cli: cli, loginItem: control)
        await state.refresh()
        await state.fixAll()
        #expect(cli.fixes.count == 1 && cli.fixes[0] == nil)
        #expect(registered == 1)
        #expect(state.worst == .ok)
    }

    @Test func aMissingCLIIsAProblemNotACrash() async {
        let state = SetupState(cli: nil)
        await state.refresh()
        #expect(state.problem == CLIError.notFound.localizedDescription)
        #expect(!state.needsAttention)
    }

    @Test func aFailingDoctorIsAProblem() async {
        let cli = FakeSetupCLI([report(okCheck)])
        cli.doctorFailure = "herdbot: usage"
        let state = SetupState(cli: cli)
        await state.refresh()
        #expect(state.problem == "herdbot: usage")
    }

    @Test func autoOpensOnceForAFailureAndNotForWarnings() async {
        let failing = SetupState(cli: FakeSetupCLI([report(failService)]))
        await failing.refresh()
        #expect(failing.shouldAutoOpen(dismissedVersion: nil))
        #expect(failing.shouldAutoOpen(dismissedVersion: "0.0.9"))
        #expect(!failing.shouldAutoOpen(dismissedVersion: "0.1.0"))
        let warning = SetupState(cli: FakeSetupCLI([report(warnLink)]))
        await warning.refresh()
        #expect(!warning.shouldAutoOpen(dismissedVersion: nil))
    }

    @Test func theLoginItemJoinsTheChecksAndRegisters() async {
        var status = LoginItemStatus.notRegistered
        var registered = 0
        let control = LoginItemControl(status: { status }, register: { registered += 1; status = .enabled })
        let state = SetupState(cli: FakeSetupCLI([report(okCheck)]), loginItem: control)
        await state.refresh()
        #expect(state.checks.map(\.id) == ["herdr", "login-item"])
        #expect(state.worst == .warn)
        await state.fix("login-item")
        #expect(registered == 1)
        #expect(state.worst == .ok)
    }

    @Test func loginItemStatusesMapToChecks() {
        #expect(loginItemCheck(.enabled)?.status == .ok)
        #expect(loginItemCheck(.notRegistered)?.fix == "login-item")
        #expect(loginItemCheck(.requiresApproval)?.fix == nil)
        #expect(loginItemCheck(.requiresApproval)?.message.contains("Login Items") == true)
        #expect(loginItemCheck(.unavailable) == nil)
    }

    @Test func fixLabelsAreHumanAndTotal() {
        for fix in ["home", "cli-link", "service", "restart-daemon", "login-item", "something-new"] {
            #expect(!fixLabel(fix).isEmpty)
        }
        #expect(fixLabel("restart-daemon") == "Restart daemon")
    }
}
