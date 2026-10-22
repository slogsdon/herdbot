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

@MainActor @Observable
public final class SetupState {
    public private(set) var report: DoctorReport?
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

    public func fixAll() async {
        fixError = nil
        do {
            _ = try await cli?.setup(fix: nil)
        } catch {
            fixError = error.localizedDescription
        }
        await refresh()
    }

    public func follow(every interval: Duration = .seconds(60)) async {
        while !Task.isCancelled {
            await refresh()
            try? await Task.sleep(for: interval)
        }
    }
}
