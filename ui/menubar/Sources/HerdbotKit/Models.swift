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

public struct RoomView: Codable, Hashable, Sendable {
    public let slug: String
    public let name: String
    /// "channel", "round_robin" or "moderated".
    public let mode: String
    public let paused: Bool
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
    public let rooms: [RoomView]
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

extension Snapshot {
    /// A herdbot older than rooms-in-status sends no `rooms`; read that as none.
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            v: try c.decode(Int.self, forKey: .v),
            at: try c.decode(Int.self, forKey: .at),
            daemon: try c.decode(String.self, forKey: .daemon),
            bots: try c.decode([BotView].self, forKey: .bots),
            rooms: try c.decodeIfPresent([RoomView].self, forKey: .rooms) ?? [],
            runs: try c.decode([RunView].self, forKey: .runs),
            cooldowns: try c.decode([CooldownView].self, forKey: .cooldowns),
            blocked: try c.decode([Event].self, forKey: .blocked),
            warnings: try c.decode([String].self, forKey: .warnings))
    }
}
