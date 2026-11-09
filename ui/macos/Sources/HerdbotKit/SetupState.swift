import Foundation
import Observation

/// Injects the app target's ServiceManagement integration into the shared setup state.
public struct LoginItemControl {
    public let status: @MainActor () -> LoginItemStatus
    public let register: @MainActor () throws -> Void

    public init(status: @escaping @MainActor () -> LoginItemStatus, register: @escaping @MainActor () throws -> Void) {
        self.status = status
        self.register = register
    }
}

/// The launchd agent as the app shows it. `disabled` is `service disable`: the plist stays and the
/// launchd override keeps the job off across logins, so enable loads the same install again.
public enum DaemonState: Sendable, Equatable {
    case unknown, enabled, disabled, notInstalled
}

/// A lifecycle command in flight; the UI disables its controls while one runs.
public enum LifecycleAction: Sendable, Equatable {
    case daemon(enabled: Bool)
    case uninstall(purge: Bool)
}

/// The confirmation Settings is showing. Purge also needs `purgeDraft` to equal the data root.
public enum LifecyclePrompt: Sendable, Equatable {
    case uninstall, purge
}

@MainActor @Observable
public final class SetupState {
    public private(set) var report: DoctorReport?
    public private(set) var problem: String?
    public private(set) var fixing: Set<String> = []
    public private(set) var fixError: String?
    public private(set) var fixingAll = false
    public private(set) var lastResult: String?
    public private(set) var daemon: DaemonState = .unknown
    public private(set) var pending: LifecycleAction?
    public private(set) var lifecycleResult: String?
    public private(set) var lifecycleError: String?
    /// Confirmation dialogs live here rather than in view `@State`, so the flow is testable.
    public var prompt: LifecyclePrompt?
    public var purgeDraft = ""
    private var loginItemStatus: LoginItemStatus = .unavailable
    @ObservationIgnored private let cli: (any SetupCLI)?
    @ObservationIgnored private let loginItem: LoginItemControl?

    public init(cli: (any SetupCLI)?, loginItem: LoginItemControl? = nil) {
        self.cli = cli
        self.loginItem = loginItem
    }

    public var checks: [DoctorCheck] {
        let doctorChecks = (report?.checks ?? []).map { check in
            guard check.id == "daemon", daemon == .disabled else { return check }
            return DoctorCheck(id: check.id, status: .ok, message: "disabled by user")
        }
        return doctorChecks + (report == nil ? [] : [loginItemCheck(loginItemStatus)].compactMap { $0 })
    }

    /// The data root the doctor reports. Purge is only offered when it is known, since the typed path must match it.
    public var dataRoot: String? {
        guard let home = report?.checks.first(where: { $0.id == "home" }), home.status == .ok else { return nil }
        return home.message
    }

    /// The typed path matches the data root, ignoring surrounding whitespace the way the CLI does.
    public var purgeMatches: Bool {
        dataRoot != nil && purgeDraft.trimmingCharacters(in: .whitespacesAndNewlines) == dataRoot
    }

    public var worst: DoctorStatus { checks.map(\.status).max() ?? .ok }
    public var needsAttention: Bool { worst != .ok }
    /// Shows the menu action only after the first report if setup needs attention or doctor failed.
    public var showsSetupInMenu: Bool { problem != nil || (report != nil && needsAttention) }
    /// Something the "Set up everything" button can actually do.
    public var hasFixable: Bool { checks.contains { $0.status != .ok && $0.fix != nil } }

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
        await refreshDaemon()
        fixing.remove(id)
    }

    public func fixAll() async {
        guard !fixingAll else { return }
        fixingAll = true
        fixError = nil
        lastResult = nil
        var messages: [String] = []
        do {
            let output = try await cli?.setup(fix: nil) ?? ""
            messages = output.split(separator: "\n").map(String.init)
        } catch {
            fixError = "setup: \(error.localizedDescription)"
            await refresh()
            await refreshDaemon()
            fixingAll = false
            return
        }
        if case .notRegistered = loginItemStatus {
            do {
                try loginItem?.register()
            } catch {
                fixError = "login-item: \(error.localizedDescription)"
            }
        }
        await refresh()
        await refreshDaemon()
        lastResult = messages.isEmpty ? "Nothing to fix automatically." : messages.joined(separator: "\n")
        fixingAll = false
    }

    /// Reads the launchd state, which the doctor report does not carry: its `service` check only says
    /// whether an install exists, not whether launchd has the job loaded.
    public func refreshDaemon() async {
        guard let cli, let report else {
            daemon = .unknown
            return
        }
        guard report.checks.first(where: { $0.id == "service" })?.status == .ok else {
            daemon = .notInstalled
            return
        }
        do {
            // `service status` prints `state = …` lines while launchd has the job, else one line saying
            // whether the service is installed; the doctor's service check decides which state that is.
            daemon = try await cli.serviceStatus().contains("state =") ? .enabled : .disabled
        } catch {
            daemon = .unknown
            // Keep an earlier action's error when there is one; a status error must not replace it.
            lifecycleError = lifecycleError ?? error.localizedDescription
        }
    }

    public func setDaemon(enabled: Bool) async {
        let verb = enabled ? "enable" : "disable"
        await runLifecycle(.daemon(enabled: enabled), failurePrefix: "service \(verb)") { cli in
            try await cli.setService(enabled: enabled)
        }
    }

    /// Uninstall keeps bot data. Purge deletes it, and only when `confirmation` is the data root.
    public func uninstall(purge: Bool, confirmation: String = "") async {
        if purge {
            guard let dataRoot else {
                lifecycleError = "The herdbot home is unknown, so bot data cannot be deleted from here."
                return
            }
            guard confirmation.trimmingCharacters(in: .whitespacesAndNewlines) == dataRoot else {
                lifecycleError = "Type \(dataRoot) exactly to delete bot data."
                return
            }
        }
        let root = dataRoot
        await runLifecycle(.uninstall(purge: purge), failurePrefix: "uninstall") { cli in
            try await cli.uninstall(purge: purge, confirmedPath: purge ? root : nil)
        }
    }

    /// Shows only what the CLI confirmed: a failure leaves the result empty, then re-reads the real state.
    private func runLifecycle(
        _ action: LifecycleAction,
        failurePrefix: String,
        _ call: (any SetupCLI) async throws -> String
    ) async {
        guard pending == nil else { return }
        guard let cli else {
            lifecycleError = CLIError.notFound.localizedDescription
            return
        }
        pending = action
        lifecycleError = nil
        lifecycleResult = nil
        do {
            lifecycleResult = try await call(cli)
        } catch {
            lifecycleError = "\(failurePrefix): \(error.localizedDescription)"
        }
        await refresh()
        await refreshDaemon()
        pending = nil
    }

    public func follow(every interval: Duration = .seconds(60)) async {
        while !Task.isCancelled {
            await refresh()
            // Re-read the launchd state each pass too, so a setup run or a `service disable` in a
            // terminal is not shown stale. It is one `launchctl print` next to the doctor's own work.
            await refreshDaemon()
            try? await Task.sleep(for: interval)
        }
    }
}
