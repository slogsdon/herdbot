import Foundation

/// The JSON `herdbot status --json` and `herdbot watch --json` print. Mirrors
/// crates/herdbot-core/src/snapshot.rs; bump `supportedVersion` with `snapshot::VERSION`.
public let supportedVersion = 1

public struct Event: Codable, Hashable, Sendable {
    public let at: Int
    public let agent: String
    public let kind: String
    public let text: String?
    public let reason: String?
    public let harness: String
}

public struct BotView: Codable, Hashable, Sendable {
    public let slug: String
    public let title: String
    public let up: Bool
}

public struct RunView: Codable, Hashable, Sendable {
    public let id: String
    public let bot: String
    public let agent: String
    public let route: String
    public let description: String
}

public struct CooldownView: Codable, Hashable, Sendable {
    public let harness: String
    /// Unix seconds.
    public let until: Int
    public let reason: String
}

public struct Snapshot: Codable, Hashable, Sendable {
    public let v: Int
    public let at: Int
    /// "up" or "down".
    public let daemon: String
    public let bots: [BotView]
    public let runs: [RunView]
    public let cooldowns: [CooldownView]
    public let blocked: [Event]
    public let warnings: [String]
}

/// One line of `herdbot watch --json`: `type` is "snapshot" or "event".
public struct StreamLine: Codable, Hashable, Sendable {
    public let v: Int
    public let type: String
    public let snapshot: Snapshot?
    public let event: Event?
}
