# Packaging app (plan 2 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The menu bar app finds the CLI bundled inside it, shows a first-run checklist from `herdbot doctor --json` with fix buttons, marks the menu bar icon when setup is needed, and can register itself as a login item.

**Architecture:** Spec: `docs/superpowers/specs/2026-10-22-packaging-design.md` ("App additions"). Everything testable lives in `HerdbotKit` (decoding, `SetupState`, icon merge, login-item mapping, fix labels, binary lookup); the SwiftUI views stay thin. The app's only seam to herdbot stays the CLI: `doctor --json` and `setup --only <fix> --yes`, both added by plan 1 (`docs/superpowers/plans/2026-10-22-packaging-cli.md`).

**Tech Stack:** Swift 6, SwiftUI, Swift Testing (`./scripts/test.sh`), `ServiceManagement` (`SMAppService`). No new dependencies. Package: `ui/menubar`.

## Global Constraints

- macOS 15 floor; builds with Command Line Tools (no full Xcode, so no `@State` macro: the App struct uses plain `let`, see `App.swift`).
- Never read herdbot's files from Swift; only run the CLI.
- `doctor --json` shape (plan 1): `{"v":1,"version":"x.y.z","checks":[{"id","status":"ok|warn|fail","message","fix"?}]}`. Unknown `v` throws `CLIError.unsupported(v)`. An unknown `status` string decodes as `warn`, so a newer herdbot degrades instead of failing.
- `doctor` exits 1 when a check fails but still prints the report; the app must accept exit 0 and 1.
- Fix ids the app knows: `home`, `cli-link`, `service`, `restart-daemon` (run via `setup --only <id> --yes`) and `login-item` (done in the app, never sent to the CLI).
- Keep existing behaviour: blocked agents still win the menu bar icon; `HERDBOT_BIN` still wins binary lookup.
- Commits: conventional messages, no AI attribution, explicit `git add` paths, no push.
- No scripted keystrokes or clicks, and never kill a running HerdbotMenu by name; the manual check is for Shane.

**Branch/worktree:** this plan needs plan 1's CLI. Create it from `feature/packaging-cli` (or from `main` once that is merged): `git -C ~/Code/herdbot worktree add .worktrees/packaging-app -b feature/packaging-app <base>`. Run commands from `ui/menubar` in that worktree; tests with `./scripts/test.sh`.

## File map

- Create `Sources/HerdbotKit/Doctor.swift`: report types, `SetupCLI`, `fixLabel`, `loginItemCheck`, `LoginItemStatus`.
- Create `Sources/HerdbotKit/SetupState.swift`: `SetupState`, `LoginItemControl`.
- Modify `Sources/HerdbotKit/ProcessCLI.swift`: bundled-helper lookup, `SetupCLI` conformance, `output(acceptingExit:)`.
- Modify `Sources/HerdbotKit/MenuState.swift`: `IconState.attention`, `IconState.with(attention:)`.
- Create `Sources/HerdbotMenu/SetupView.swift`, `Sources/HerdbotMenu/LoginItem.swift`.
- Modify `Sources/HerdbotMenu/App.swift`, `MenuContent.swift`, `MenuIcon.swift`, `WindowOpener.swift`.
- Create tests `Tests/HerdbotKitTests/SetupTests.swift`; extend `ProcessCLITests.swift`, `Fixtures.swift`, `StateTests.swift`.
- Modify `README.md` (this package) at the end.

---

### Task 1: Doctor types, CLI calls, bundled-helper lookup

**Files:** Create `Sources/HerdbotKit/Doctor.swift`; modify `Sources/HerdbotKit/ProcessCLI.swift`; tests in `Tests/HerdbotKitTests/SetupTests.swift` and `ProcessCLITests.swift`.

**Interfaces:**
- Produces: `DoctorStatus` (`ok|warn|fail`, `Comparable`), `DoctorCheck` (`id`, `status`, `message`, `fix: String?`), `DoctorReport` (`v`, `version`, `checks`), `supportedDoctorVersion = 1`, `protocol SetupCLI { func doctor() async throws -> DoctorReport; func setup(fix: String?) async throws -> String }`, `ProcessCLI: SetupCLI`, `ProcessCLI.locate(environment:home:bundlePath:)`.

- [ ] **Step 1: Failing tests.** `SetupTests.swift`:

```swift
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

    @Test func statusesOrderByseverity() {
        #expect(DoctorStatus.ok < .warn && .warn < .fail)
    }
}
```

`ProcessCLITests.swift` (append inside the suite):

```swift
    @Test func doctorAcceptsExitOneWhenItPrintsAReport() async throws {
        let script = try fakeHerdbot("[ \"$1 $2\" = \"doctor --json\" ] || exit 2\ncat <<'EOF'\n\(doctorJSON.replacingOccurrences(of: "\n", with: ""))\nEOF\nexit 1")
        let report = try await ProcessCLI(binary: script).doctor()
        #expect(report.checks.count == 3)
    }

    @Test func doctorFromAHerdbotWithoutItReportsTheError() async throws {
        let script = try fakeHerdbot("echo 'herdbot: usage: …' >&2; exit 1")
        await #expect(throws: CLIError.failed("herdbot: usage: …")) { try await ProcessCLI(binary: script).doctor() }
    }

    @Test func doctorRejectsAnUnknownVersion() async throws {
        let script = try fakeHerdbot(#"echo '{"v":9,"version":"x","checks":[]}'"#)
        await #expect(throws: CLIError.unsupported(9)) { try await ProcessCLI(binary: script).doctor() }
    }

    @Test func setupPassesTheFixAndYes() async throws {
        let only = try fakeHerdbot(#"[ "$*" = "setup --yes --only service" ] && echo 'service: installed'"#)
        #expect(try await ProcessCLI(binary: only).setup(fix: "service").contains("installed"))
        let all = try fakeHerdbot(#"[ "$*" = "setup --yes" ]"#)
        _ = try await ProcessCLI(binary: all).setup(fix: nil)
    }

    @Test func locatePrefersTheEnvOverrideThenTheBundledHelper() throws {
        let bundle = FileManager.default.temporaryDirectory.appendingPathComponent("hk-\(UUID().uuidString)/Herdbot.app")
        let helpers = bundle.appendingPathComponent("Contents/Helpers")
        try FileManager.default.createDirectory(at: helpers, withIntermediateDirectories: true)
        let helper = helpers.appendingPathComponent("herdbot")
        try "#!/bin/sh\n".write(to: helper, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)
        let override = try fakeHerdbot("")
        #expect(ProcessCLI.locate(environment: ["HERDBOT_BIN": override.path], home: "/nonexistent", bundlePath: bundle.path) == override)
        #expect(ProcessCLI.locate(environment: [:], home: "/nonexistent", bundlePath: bundle.path)?.path == helper.path)
        #expect(ProcessCLI.locate(environment: [:], home: "/nonexistent", bundlePath: "/nonexistent") == nil)
    }
```

Run `./scripts/test.sh` → FAIL (types missing). (If an existing `locate` test calls it without `bundlePath`, the new parameter must have a default.)

- [ ] **Step 2: `Doctor.swift`.**

```swift
import Foundation

/// Mirrors `herdbot doctor --json` (crates/herdbot/src/doctor.rs); bump with its `v`.
public let supportedDoctorVersion = 1

public enum DoctorStatus: String, Codable, Sendable, Comparable {
    case ok, warn, fail

    /// A status a newer herdbot adds is shown as a warning rather than failing the decode.
    public init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = DoctorStatus(rawValue: raw) ?? .warn
    }

    private var rank: Int {
        switch self {
        case .ok: 0
        case .warn: 1
        case .fail: 2
        }
    }

    public static func < (lhs: Self, rhs: Self) -> Bool { lhs.rank < rhs.rank }
}

public struct DoctorCheck: Codable, Hashable, Sendable, Identifiable {
    public let id: String
    public let status: DoctorStatus
    public let message: String
    /// The `setup --only` fix that repairs this check, when there is one.
    public let fix: String?

    public init(id: String, status: DoctorStatus, message: String, fix: String? = nil) {
        self.id = id
        self.status = status
        self.message = message
        self.fix = fix
    }
}

public struct DoctorReport: Codable, Hashable, Sendable {
    public let v: Int
    public let version: String
    public let checks: [DoctorCheck]

    public init(v: Int = supportedDoctorVersion, version: String, checks: [DoctorCheck]) {
        self.v = v
        self.version = version
        self.checks = checks
    }
}

public protocol SetupCLI: Sendable {
    /// `herdbot doctor --json`.
    func doctor() async throws -> DoctorReport
    /// `herdbot setup --yes [--only <fix>]`; the output is the fixes' messages.
    func setup(fix: String?) async throws -> String
}
```

- [ ] **Step 3: `ProcessCLI`.** In `ProcessCLI.swift`: change `locate` to

```swift
    public static func locate(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        home: String = NSHomeDirectory(),
        bundlePath: String? = Bundle.main.bundlePath
    ) -> URL? {
        // The CLI shipped inside the app comes before the installed ones: it is the version this app was built with.
        let helper = bundlePath.map { $0 + "/Contents/Helpers/herdbot" }
        let candidates = [environment["HERDBOT_BIN"], helper, home + "/.cargo/bin/herdbot", "/opt/homebrew/bin/herdbot", "/usr/local/bin/herdbot"]
```

(the rest unchanged). Change `output` to `private func output(_ arguments: [String], acceptingExit accepted: Set<Int32> = [0]) async throws -> Data` and its success test to `if accepted.contains(process.terminationStatus) && (process.terminationStatus == 0 || !data.isEmpty)`; keep the failure branch (stderr text) otherwise. Then, in the same file:

```swift
extension ProcessCLI: SetupCLI {
    public func doctor() async throws -> DoctorReport {
        // doctor exits 1 when a check fails, and still prints the report.
        let data = try await output(["doctor", "--json"], acceptingExit: [0, 1])
        let report = try JSONDecoder().decode(DoctorReport.self, from: data)
        guard report.v == supportedDoctorVersion else { throw CLIError.unsupported(report.v) }
        return report
    }

    public func setup(fix: String?) async throws -> String {
        var arguments = ["setup", "--yes"]
        if let fix { arguments += ["--only", fix] }
        return String(decoding: try await output(arguments), as: UTF8.self)
    }
}
```

Run `./scripts/test.sh` → all PASS.

- [ ] **Step 4: Commit.**

```bash
git add ui/menubar/Sources/HerdbotKit/Doctor.swift ui/menubar/Sources/HerdbotKit/ProcessCLI.swift ui/menubar/Tests/HerdbotKitTests/SetupTests.swift ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift
git commit -m "feat(menubar): doctor and setup over the CLI, and the bundled helper lookup"
```

---

### Task 2: `SetupState`, icon merge, login-item mapping

**Files:** Create `Sources/HerdbotKit/SetupState.swift`; modify `Sources/HerdbotKit/Doctor.swift` (add `LoginItemStatus`, `loginItemCheck`, `fixLabel`), `Sources/HerdbotKit/MenuState.swift`; extend `Fixtures.swift` (a `FakeSetupCLI`), `SetupTests.swift`, `StateTests.swift`.

**Interfaces:**
- Produces: `enum LoginItemStatus { enabled, notRegistered, requiresApproval, unavailable }`; `func loginItemCheck(_ status: LoginItemStatus) -> DoctorCheck?`; `func fixLabel(_ fix: String) -> String`; `struct LoginItemControl { status: @MainActor () -> LoginItemStatus; register: @MainActor () throws -> Void }`; `@MainActor @Observable final class SetupState` with `report`, `problem`, `fixing`, `fixError`, `checks`, `worst`, `needsAttention`, `refresh() async`, `fix(_:) async`, `fixAll() async`, `shouldAutoOpen(dismissedVersion:) -> Bool`, `follow(every:) async`; `IconState.attention`; `IconState.with(attention:) -> IconState`.

- [ ] **Step 1: Failing tests.** Add to `Fixtures.swift`:

```swift
/// Answers `doctor` from a queue of reports (the last one repeats) and records the fixes asked for.
final class FakeSetupCLI: SetupCLI, @unchecked Sendable {
    nonisolated(unsafe) var reports: [DoctorReport]
    nonisolated(unsafe) var fixes: [String?] = []
    nonisolated(unsafe) var failure: String?
    nonisolated(unsafe) var doctorFailure: String?
    init(_ reports: [DoctorReport]) { self.reports = reports }
    func doctor() async throws -> DoctorReport {
        if let doctorFailure { throw CLIError.failed(doctorFailure) }
        return reports.count > 1 ? reports.removeFirst() : reports[0]
    }
    func setup(fix: String?) async throws -> String {
        fixes.append(fix)
        if let failure { throw CLIError.failed(failure) }
        return "ok"
    }
}

func report(_ checks: DoctorCheck...) -> DoctorReport { DoctorReport(version: "0.1.0", checks: checks) }
let okCheck = DoctorCheck(id: "herdr", status: .ok, message: "herdr 0.9.1")
let failService = DoctorCheck(id: "service", status: .fail, message: "not installed", fix: "service")
let warnLink = DoctorCheck(id: "cli-link", status: .warn, message: "no herdbot on PATH", fix: "cli-link")
```

Add to `SetupTests.swift`:

```swift
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
```

Append to `StateTests.swift` (suite `State`):

```swift
    @Test func attentionReplacesQuietIconsButNotBlockedOrUnavailable() {
        #expect(IconState.idle.with(attention: true) == .attention)
        #expect(IconState.running.with(attention: true) == .attention)
        #expect(IconState.daemonDown.with(attention: true) == .attention)
        #expect(IconState.blocked(2).with(attention: true) == .blocked(2))
        #expect(IconState.unavailable.with(attention: true) == .unavailable)
        #expect(IconState.idle.with(attention: false) == .idle)
    }
```

Run `./scripts/test.sh` → FAIL.

- [ ] **Step 2: Implement.** Append to `Doctor.swift`:

```swift
public enum LoginItemStatus: Sendable { case enabled, notRegistered, requiresApproval, unavailable }

/// The one check the app owns. `unavailable` (not running from an installed bundle) shows nothing.
public func loginItemCheck(_ status: LoginItemStatus) -> DoctorCheck? {
    switch status {
    case .enabled: DoctorCheck(id: "login-item", status: .ok, message: "starts at login")
    case .notRegistered: DoctorCheck(id: "login-item", status: .warn, message: "does not start at login", fix: "login-item")
    case .requiresApproval: DoctorCheck(id: "login-item", status: .warn, message: "allow Herdbot in System Settings → General → Login Items")
    case .unavailable: nil
    }
}

public func fixLabel(_ fix: String) -> String {
    switch fix {
    case "home": "Create herdbot home"
    case "cli-link": "Link herdbot"
    case "service": "Install service"
    case "restart-daemon": "Restart daemon"
    case "login-item": "Start at login"
    default: "Fix"
    }
}
```

`SetupState.swift`:

```swift
import Foundation
import Observation

/// The app's side of the login-item check: SMAppService lives in the app target, so it is injected.
public struct LoginItemControl {
    public let status: @MainActor () -> LoginItemStatus
    public let register: @MainActor () throws -> Void

    public init(status: @escaping @MainActor () -> LoginItemStatus, register: @escaping @MainActor () throws -> Void) {
        self.status = status
        self.register = register
    }
}

@MainActor @Observable
public final class SetupState {
    public private(set) var report: DoctorReport?
    /// Why there is no report: herdbot missing, or a herdbot without `doctor`.
    public private(set) var problem: String?
    public private(set) var fixing: Set<String> = []
    public private(set) var fixError: String?
    private var loginItemStatus: LoginItemStatus = .unavailable
    @ObservationIgnored private let cli: (any SetupCLI)?
    @ObservationIgnored private let loginItem: LoginItemControl?

    public init(cli: (any SetupCLI)?, loginItem: LoginItemControl? = nil) {
        self.cli = cli
        self.loginItem = loginItem
    }

    public var checks: [DoctorCheck] {
        (report?.checks ?? []) + (report == nil ? [] : [loginItemCheck(loginItemStatus)].compactMap { $0 })
    }

    public var worst: DoctorStatus { checks.map(\.status).max() ?? .ok }
    public var needsAttention: Bool { worst != .ok }

    /// Open the setup window by itself for a failure, once per herdbot version (a closed window stays closed).
    public func shouldAutoOpen(dismissedVersion: String?) -> Bool {
        guard let report, worst == .fail else { return false }
        return dismissedVersion != report.version
    }

    public func refresh() async {
        guard let cli else {
            problem = CLIError.notFound.localizedDescription
            return
        }
        do {
            report = try await cli.doctor()
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
        loginItemStatus = loginItem?.status() ?? .unavailable
    }

    public func fix(_ id: String) async {
        guard !fixing.contains(id) else { return }
        fixing.insert(id)
        fixError = nil
        do {
            if id == "login-item" {
                try loginItem?.register()
            } else {
                _ = try await cli?.setup(fix: id)
            }
        } catch {
            fixError = "\(id): \(error.localizedDescription)"
        }
        await refresh()
        fixing.remove(id)
    }

    /// `herdbot setup --yes`: every pending fix, and a starter bot when there are none.
    public func fixAll() async {
        fixError = nil
        do {
            _ = try await cli?.setup(fix: nil)
        } catch {
            fixError = error.localizedDescription
        }
        await refresh()
    }

    /// Re-check periodically: a daemon dying or an update landing changes the answer.
    public func follow(every interval: Duration = .seconds(60)) async {
        while !Task.isCancelled {
            await refresh()
            try? await Task.sleep(for: interval)
        }
    }
}
```

In `MenuState.swift` add `case attention` to `IconState` and:

```swift
extension IconState {
    /// A setup problem replaces the quiet icons; blocked agents and a missing herdbot stay visible.
    public func with(attention: Bool) -> IconState {
        guard attention else { return self }
        switch self {
        case .blocked, .unavailable: return self
        default: return .attention
        }
    }
}
```

Run `./scripts/test.sh` → PASS. (`MenuIcon.swift`'s switch will not compile until Task 3; build only the Kit target here: `swift build --target HerdbotKit`, and run the tests, which do not build the app target.)

- [ ] **Step 3: Commit.**

```bash
git add ui/menubar/Sources/HerdbotKit/Doctor.swift ui/menubar/Sources/HerdbotKit/SetupState.swift ui/menubar/Sources/HerdbotKit/MenuState.swift ui/menubar/Tests/HerdbotKitTests/Fixtures.swift ui/menubar/Tests/HerdbotKitTests/SetupTests.swift ui/menubar/Tests/HerdbotKitTests/StateTests.swift
git commit -m "feat(menubar): setup state, login-item check and attention icon state"
```

---

### Task 3: Views and wiring

**Files:** Create `Sources/HerdbotMenu/SetupView.swift`, `Sources/HerdbotMenu/LoginItem.swift`; modify `App.swift`, `MenuContent.swift`, `MenuIcon.swift`, `WindowOpener.swift`.

**Interfaces:**
- Consumes: everything from Tasks 1-2.
- Produces: `Window("Set up herdbot", id: "setup")`, a menu item, the `.attention` icon, the login-item control.

- [ ] **Step 1: `LoginItem.swift`.**

```swift
import HerdbotKit
import ServiceManagement

@MainActor
func makeLoginItemControl() -> LoginItemControl {
    LoginItemControl(
        status: {
            switch SMAppService.mainApp.status {
            case .enabled: .enabled
            case .notRegistered: .notRegistered
            case .requiresApproval: .requiresApproval
            default: .unavailable // .notFound: not running from an installed bundle
            }
        },
        register: { try SMAppService.mainApp.register() })
}
```

- [ ] **Step 2: `SetupView.swift`.**

```swift
import AppKit
import HerdbotKit
import SwiftUI

struct SetupView: View {
    let setup: SetupState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Set up herdbot").font(.title2.bold())
            if let problem = setup.problem {
                Text(problem)
                Text("Install or update herdbot, then reopen this window.").foregroundStyle(.secondary)
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(setup.checks) { check in row(check) }
                }
            }
            if let error = setup.fixError {
                Text(error).foregroundStyle(.red).textSelection(.enabled)
            }
            HStack {
                Button("Set up everything") { Task { await setup.fixAll() } }
                    .buttonStyle(.borderedProminent)
                    .disabled(!setup.needsAttention || setup.problem != nil)
                Spacer()
                Button("Done") { dismiss() }
            }
        }
        .padding(20)
        .frame(width: 460)
        .onAppear {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
            Task { await setup.refresh() }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willCloseNotification)) { note in
            guard (note.object as? NSWindow)?.title == "Set up herdbot" else { return }
            UserDefaults.standard.set(setup.report?.version, forKey: "setupDismissedVersion")
            // The chat window keeps the Dock presence while it is open.
            if !NSApp.windows.contains(where: { $0.title == "herdbot" && $0.isVisible }) {
                NSApp.setActivationPolicy(.accessory)
            }
        }
    }

    private func row(_ check: DoctorCheck) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            symbol(check.status)
            VStack(alignment: .leading, spacing: 2) {
                Text(check.id).font(.headline)
                Text(check.message).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            if let fix = check.fix, check.status != .ok {
                if setup.fixing.contains(fix) {
                    ProgressView().controlSize(.small)
                } else {
                    Button(fixLabel(fix)) { Task { await setup.fix(fix) } }
                }
            }
        }
    }

    @ViewBuilder
    private func symbol(_ status: DoctorStatus) -> some View {
        switch status {
        case .ok: Image(systemName: "checkmark.circle.fill").foregroundStyle(.green).accessibilityLabel("ok")
        case .warn: Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange).accessibilityLabel("warning")
        case .fail: Image(systemName: "xmark.octagon.fill").foregroundStyle(.red).accessibilityLabel("failed")
        }
    }
}
```

- [ ] **Step 3: Icon.** `MenuIcon` takes `icon: IconState` (already merged) and a new case `.attention: Image(systemName: "exclamationmark.triangle.fill")`. Menu bar items are rendered as template images, so a tint may be ignored (unverified); the distinct symbol carries the meaning. Keep `.foregroundStyle(.orange)` on it anyway.

- [ ] **Step 4: `WindowOpener` and `OpenerCapture`.** Add `var openSetup: () -> Void = {}` to `WindowOpener`. `OpenerCapture` gains `let onReady: () -> Void` and sets both closures: `opener.open = { openWindow(id: "chat") }; opener.openSetup = { openWindow(id: "setup") }; onReady()` in its `.task`.

- [ ] **Step 5: `App.swift`.** In `init`: `let setup = SetupState(cli: cli, loginItem: makeLoginItemControl())`; store as `private let setup: SetupState`; `Task { await setup.follow() }` after creation. In `body`, add before `MenuBarExtra`:

```swift
        Window("Set up herdbot", id: "setup") {
            SetupView(setup: setup)
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)
```

and change the label to `MenuIcon(icon: state.icon.with(attention: setup.needsAttention)).modifier(OpenerCapture(opener: opener, onReady: { autoOpenSetup() }))` where `autoOpenSetup` is a method on the App struct: `Task { await setup.refresh(); if setup.shouldAutoOpen(dismissedVersion: UserDefaults.standard.string(forKey: "setupDismissedVersion")) { opener.openSetup() } }`. (`opener` is a `let` class instance, so capturing it from the struct method is fine.) Pass `setup` into `MenuContent`.

- [ ] **Step 6: `MenuContent.swift`.** Take `let setup: SetupState` and `@Environment(\.openWindow)` is already there. Above the final `Divider()` block's buttons add:

```swift
        Button(setup.needsAttention ? "Finish setup…" : "Setup…") {
            NSApp.activate(ignoringOtherApps: true)
            openWindow(id: "setup")
        }
```

- [ ] **Step 7: Verify.** `./scripts/test.sh` passes; `./scripts/build-app.sh` builds with no new warnings (the `ld: warning: search path` lines are gone on this machine; any other warning is a finding). Run the built app only if Shane asks; it cannot be exercised headlessly. State in the report that the views were compiled but not looked at.

- [ ] **Step 8: Commit.**

```bash
git add ui/menubar/Sources/HerdbotMenu/SetupView.swift ui/menubar/Sources/HerdbotMenu/LoginItem.swift ui/menubar/Sources/HerdbotMenu/App.swift ui/menubar/Sources/HerdbotMenu/MenuContent.swift ui/menubar/Sources/HerdbotMenu/MenuIcon.swift ui/menubar/Sources/HerdbotMenu/WindowOpener.swift
git commit -m "feat(menubar): first-run setup window, attention icon and login item"
```

---

### Task 4: Docs and the manual check

**Files:** Modify `ui/menubar/README.md`.

- [ ] **Step 1:** Replace "the app does not register itself" with the login-item behaviour (Setup window → "Start at login"; `SMAppService.mainApp`; macOS may ask for approval in System Settings → General → Login Items). Add a **Setup** section: the checklist is `herdbot doctor --json`, buttons run `herdbot setup --only <fix> --yes`, "Set up everything" runs `herdbot setup --yes` (which also creates a starter bot `helper` when there are none), the window opens by itself once per herdbot version when a check fails, and the menu bar icon shows `⚠` (a triangle) while any check is not ok. Update the binary lookup sentence: `$HERDBOT_BIN`, then `Herdbot.app/Contents/Helpers/herdbot` inside the bundle, then `~/.cargo/bin`, `/opt/homebrew/bin`, `/usr/local/bin`.
- [ ] **Step 2:** Add a **Manual check** (for Shane, not the agent): with a throwaway `HERDBOT_HOME` (as in the existing check) and `HERDBOT_BIN` pointing at the plan-1 build, launch the app; expect the setup window to open with `service`/`daemon` failing and a `cli-link` warning; press "Link herdbot" with `HERDBOT_LINK_DIR` set to a temp dir in the app's environment and expect the row to turn ok after the refresh; confirm the icon shows the triangle while a check fails and returns to normal after. Do not press "Install service" against a real home: it would replace the running daemon.
- [ ] **Step 3: Commit.**

```bash
git add ui/menubar/README.md
git commit -m "docs(menubar): document the setup window, the login item and the helper lookup"
```

---

## Self-review (done)

- Spec coverage: bundled helper first-candidate (T1, after the dev override), first-run window + fix buttons (T2-3), amber/attention icon (T2-3; symbol-based because template rendering may drop tint, noted), "Restart daemon" via the `restart-daemon` fix label (T2), `SMAppService` login item with the plist fallback left unbuilt (spec's fallback is only needed if `SMAppService` fails on a real bundle, found in plan 3's fresh-user test), auto-open once per version (T2-3).
- Names are consistent across tasks: `SetupCLI`, `DoctorReport/Check/Status`, `SetupState`, `LoginItemControl/Status`, `loginItemCheck`, `fixLabel`, `IconState.attention/with(attention:)`, `WindowOpener.openSetup`.
- The `.attention` case is added in T2 and the exhaustive `MenuIcon` switch is fixed in T3, so T2 only builds the Kit target.
- Not covered here: the release script, signing, the cask and the fresh-user test (plan 3).
