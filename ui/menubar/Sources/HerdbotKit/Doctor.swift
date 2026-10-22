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
