import Foundation

/// Mirrors `herdbot doctor --json`; bump with the CLI protocol version.
public let supportedDoctorVersion = 1

public enum DoctorStatus: String, Codable, Sendable, Comparable {
    case ok, warn, fail

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
    func doctor() async throws -> DoctorReport
    func setup(fix: String?) async throws -> String
}

public enum LoginItemStatus: Sendable { case enabled, notRegistered, requiresApproval, unavailable }

/// The one check the app owns. An unavailable login item means the app is not installed as a bundle.
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
