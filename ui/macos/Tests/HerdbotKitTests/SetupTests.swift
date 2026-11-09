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

    @Test func theAllButtonNeedsSomethingFixable() async {
        let onlyUnfixable = SetupState(cli: FakeSetupCLI([report(okCheck, failHarnesses)]))
        await onlyUnfixable.refresh()
        #expect(onlyUnfixable.needsAttention)
        #expect(!onlyUnfixable.hasFixable)
        let fixable = SetupState(cli: FakeSetupCLI([report(failHarnesses, failService)]))
        await fixable.refresh()
        #expect(fixable.hasFixable)
        let fine = SetupState(cli: FakeSetupCLI([report(okCheck)]))
        await fine.refresh()
        #expect(!fine.hasFixable)
    }

    @Test func fixAllReportsWhatSetupDidAndClearsWhenDone() async {
        let cli = FakeSetupCLI([report(failService), report(okCheck)])
        cli.output = "service: installed\ncli-link: linked /opt/homebrew/bin/herdbot"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fixAll()
        #expect(state.lastResult == "service: installed\ncli-link: linked /opt/homebrew/bin/herdbot")
        #expect(!state.fixingAll)
    }

    @Test func fixAllWithNothingToDoSaysSo() async {
        let cli = FakeSetupCLI([report(failHarnesses)])
        cli.output = ""
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fixAll()
        #expect(state.lastResult == "Nothing to fix automatically.")
    }

    @Test func fixAllKeepsTheErrorAndNoResult() async {
        let cli = FakeSetupCLI([report(failService)])
        cli.failure = "service: build output"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fixAll()
        #expect(state.fixError == "setup: service: build output")
        #expect(state.lastResult == nil)
    }

    @Test func hintsExplainChecksWithoutAFix() {
        #expect(hint(for: failHarnesses)?.contains("PATH") == true)
        #expect(hint(for: DoctorCheck(id: "herdr", status: .fail, message: "x"))?.contains("herdr") == true)
        #expect(hint(for: failService) == nil)
        #expect(hint(for: okCheck) == nil)
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
        #expect(state.showsSetupInMenu)
    }

    @Test func setupIsHiddenBeforeTheFirstReport() {
        let state = SetupState(cli: FakeSetupCLI([report(okCheck)]))
        #expect(!state.showsSetupInMenu)
    }

    @Test func aHealthyReportHidesSetup() async {
        let state = SetupState(cli: FakeSetupCLI([report(okCheck)]))
        await state.refresh()
        #expect(state.report != nil && !state.needsAttention)
        #expect(!state.showsSetupInMenu)
    }

    @Test func aReportNeedingAttentionShowsSetup() async {
        let warning = SetupState(cli: FakeSetupCLI([report(warnLink)]))
        await warning.refresh()
        #expect(warning.showsSetupInMenu)
        let failing = SetupState(cli: FakeSetupCLI([report(failService)]))
        await failing.refresh()
        #expect(failing.showsSetupInMenu)
    }

    @Test func aFailingDoctorIsAProblem() async {
        let cli = FakeSetupCLI([report(okCheck)])
        cli.doctorFailure = "herdbot: usage"
        let state = SetupState(cli: cli)
        await state.refresh()
        #expect(state.problem == "herdbot: usage")
        #expect(state.showsSetupInMenu)
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

    @Test func intentionallyDisabledDaemonDoesNotTriggerSetupAttention() async {
        let disabledDaemon = DoctorCheck(id: "daemon", status: .fail, message: "the daemon is not running", fix: "restart-daemon")
        let cli = FakeSetupCLI([report(okCheck, okService, disabledDaemon)])
        cli.serviceStatusText = "installed but not running"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.refreshDaemon()
        #expect(state.daemon == .disabled)
        #expect(state.checks.first(where: { $0.id == "daemon" })?.status == .ok)
        #expect(!state.needsAttention)
        #expect(!state.showsSetupInMenu)
        #expect(!state.shouldAutoOpen(dismissedVersion: nil))
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

@MainActor
@Suite struct Lifecycle {
    @Test func daemonStateComesFromTheServiceCheckAndLaunchdStatus() async {
        let cli = FakeSetupCLI([report(okCheck, okService)])
        let state = SetupState(cli: cli)
        #expect(state.daemon == .unknown)
        await state.refresh()
        await state.refreshDaemon()
        #expect(state.daemon == .enabled)
        cli.serviceStatusText = "installed but not running"
        await state.refreshDaemon()
        #expect(state.daemon == .disabled)
        let missing = SetupState(cli: FakeSetupCLI([report(okCheck, failService)]))
        await missing.refresh()
        await missing.refreshDaemon()
        #expect(missing.daemon == .notInstalled)
    }

    @Test func disableRunsTheCommandThenShowsTheRealState() async {
        let cli = FakeSetupCLI([report(okCheck, okService)])
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.refreshDaemon()
        cli.serviceStatusText = "installed but not running"
        await state.setDaemon(enabled: false)
        #expect(cli.serviceCalls == [false])
        #expect(state.daemon == .disabled)
        #expect(state.lifecycleResult == "disabled the daemon; the service stays installed, so enable starts it again")
        #expect(state.lifecycleError == nil)
        #expect(state.pending == nil)
    }

    @Test func enableRunsTheEnableCommand() async {
        let cli = FakeSetupCLI([report(okCheck, okService)])
        cli.serviceStatusText = "installed but not running"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.refreshDaemon()
        cli.serviceStatusText = "state = running"
        await state.setDaemon(enabled: true)
        #expect(cli.serviceCalls == [true])
        #expect(state.daemon == .enabled)
    }

    @Test func aFailedDaemonChangeSurfacesTheErrorWithoutFalseSuccess() async {
        let cli = FakeSetupCLI([report(okCheck, okService)])
        cli.lifecycleFailure = "Boot-out failed: 5: Input/output error"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.refreshDaemon()
        await state.setDaemon(enabled: false)
        #expect(state.lifecycleError == "service disable: Boot-out failed: 5: Input/output error")
        #expect(state.lifecycleResult == nil)
        #expect(state.daemon == .enabled)
    }

    @Test func uninstallKeepsBotDataByDefault() async {
        let cli = FakeSetupCLI([report(okCheck, okService, homeCheck)])
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.uninstall(purge: false)
        #expect(cli.uninstalls == [FakeUninstall(purge: false, confirmedPath: nil)])
        #expect(state.lifecycleResult?.contains("kept") == true)
        #expect(state.lifecycleError == nil)
    }

    @Test func purgeNeedsTheTypedDataPath() async {
        let cli = FakeSetupCLI([report(okCheck, okService, homeCheck)])
        let state = SetupState(cli: cli)
        await state.refresh()
        #expect(state.dataRoot == "/Users/x/Bots")
        state.purgeDraft = "/Users/x/Bot"
        #expect(!state.purgeMatches)
        state.purgeDraft = "  /Users/x/Bots\n"
        #expect(state.purgeMatches)
        state.prompt = .purge
        #expect(state.prompt == .purge)
        await state.uninstall(purge: true, confirmation: "/Users/x/Bot")
        #expect(cli.uninstalls.isEmpty)
        #expect(state.lifecycleError == "Type /Users/x/Bots exactly to delete bot data.")
        await state.uninstall(purge: true, confirmation: "  /Users/x/Bots\n")
        #expect(cli.uninstalls == [FakeUninstall(purge: true, confirmedPath: "/Users/x/Bots")])
        #expect(state.lifecycleError == nil)
    }

    @Test func purgeIsRefusedWhenTheDataPathIsUnknown() async {
        let cli = FakeSetupCLI([report(okCheck, okService)])
        let state = SetupState(cli: cli)
        await state.refresh()
        #expect(state.dataRoot == nil)
        await state.uninstall(purge: true, confirmation: "")
        #expect(cli.uninstalls.isEmpty)
        #expect(state.lifecycleError != nil)
    }

    @Test func uninstallRefreshesTheDoctorAndDaemon() async {
        let cli = FakeSetupCLI([report(okCheck, okService, homeCheck), report(okCheck, failService, homeCheck)])
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.uninstall(purge: false)
        #expect(state.worst == .fail)
        #expect(state.daemon == .notInstalled)
    }

    @Test func setupAfterUninstallRefreshesDaemonWithoutLeavingSettings() async {
        let cli = FakeSetupCLI([
            report(okCheck, okService, homeCheck),
            report(okCheck, failService, homeCheck),
            report(okCheck, okService, homeCheck),
        ])
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.refreshDaemon()
        await state.uninstall(purge: false)
        #expect(state.daemon == .notInstalled)

        cli.serviceStatusText = "state = running"
        await state.fixAll()
        #expect(state.daemon == .enabled)
    }

    @Test func aFailedUninstallSurfacesTheErrorWithoutFalseSuccess() async {
        let cli = FakeSetupCLI([report(okCheck, okService, homeCheck)])
        cli.lifecycleFailure = "path did not match; nothing deleted"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.uninstall(purge: true, confirmation: "/Users/x/Bots")
        #expect(state.lifecycleError == "uninstall: path did not match; nothing deleted")
        #expect(state.lifecycleResult == nil)
        #expect(state.checks.map(\.id).contains("home"))
    }

    @Test func aMissingCLIIsAnErrorForLifecycleActions() async {
        let state = SetupState(cli: nil)
        await state.setDaemon(enabled: false)
        #expect(state.lifecycleError == CLIError.notFound.localizedDescription)
        #expect(state.pending == nil)
    }
}
