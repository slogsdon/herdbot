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
    /// History items, as `HistoryPage.total`. Zero from a herdbot that doesn't send it.
    public let items: Int
    /// Unix seconds of the last item.
    public let lastAt: Int?
    /// First line of the last item.
    public let preview: String
    /// A prompt was delivered and its turn has not been logged yet.
    public let working: Bool
    /// Messages waiting in the bot's queue, not yet delivered.
    public let queued: Int

    enum CodingKeys: String, CodingKey {
        case slug, title, up, items, lastAt = "last_at", preview, working, queued
    }

    public init(slug: String, title: String, up: Bool, items: Int = 0, lastAt: Int? = nil, preview: String = "", working: Bool = false, queued: Int = 0) {
        self.slug = slug
        self.title = title
        self.up = up
        self.items = items
        self.lastAt = lastAt
        self.preview = preview
        self.working = working
        self.queued = queued
    }
}

public struct RoomView: Codable, Hashable, Sendable {
    public let slug: String
    public let name: String
    /// "channel", "round_robin" or "moderated".
    public let mode: String
    public let paused: Bool
    public let items: Int
    public let lastAt: Int?
    public let preview: String

    enum CodingKeys: String, CodingKey {
        case slug, name, mode, paused, items, lastAt = "last_at", preview
    }

    public init(slug: String, name: String, mode: String, paused: Bool, items: Int = 0, lastAt: Int? = nil, preview: String = "") {
        self.slug = slug
        self.name = name
        self.mode = mode
        self.paused = paused
        self.items = items
        self.lastAt = lastAt
        self.preview = preview
    }
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

extension BotView {
    /// A herdbot older than conversation counts sends only slug, title and up.
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            slug: try c.decode(String.self, forKey: .slug),
            title: try c.decode(String.self, forKey: .title),
            up: try c.decode(Bool.self, forKey: .up),
            items: try c.decodeIfPresent(Int.self, forKey: .items) ?? 0,
            lastAt: try c.decodeIfPresent(Int.self, forKey: .lastAt),
            preview: try c.decodeIfPresent(String.self, forKey: .preview) ?? "",
            working: try c.decodeIfPresent(Bool.self, forKey: .working) ?? false,
            queued: try c.decodeIfPresent(Int.self, forKey: .queued) ?? 0)
    }
}

extension RoomView {
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            slug: try c.decode(String.self, forKey: .slug),
            name: try c.decode(String.self, forKey: .name),
            mode: try c.decode(String.self, forKey: .mode),
            paused: try c.decode(Bool.self, forKey: .paused),
            items: try c.decodeIfPresent(Int.self, forKey: .items) ?? 0,
            lastAt: try c.decodeIfPresent(Int.self, forKey: .lastAt),
            preview: try c.decodeIfPresent(String.self, forKey: .preview) ?? "")
    }
}

/// One message in a conversation, as `herdbot history --json` prints it.
public struct HistoryItem: Codable, Hashable, Sendable, Identifiable {
    /// Index among the persisted items, stable as new ones append. nil while pending.
    public let n: Int?
    public let at: Int
    /// "user", "bot" or "notice".
    public let kind: String
    /// A room's speaker (a bot slug) on `bot` items.
    public let speaker: String?
    public let text: String
    /// "inflight" or "queued" on pending items.
    public let state: String?

    public init(n: Int?, at: Int, kind: String, speaker: String? = nil, text: String, state: String? = nil) {
        self.n = n
        self.at = at
        self.kind = kind
        self.speaker = speaker
        self.text = text
        self.state = state
    }

    public var id: String { n.map { "n\($0)" } ?? "p:\(state ?? ""):\(at):\(text)" }
    public var isPending: Bool { n == nil }
}

/// `herdbot history <target> --json`.
public struct HistoryPage: Codable, Hashable, Sendable {
    public let v: Int
    public let target: String
    /// Persisted items in the whole conversation; pending ones are not counted.
    public let total: Int
    public let items: [HistoryItem]
    public let warnings: [String]

    public init(v: Int = supportedVersion, target: String, total: Int, items: [HistoryItem], warnings: [String] = []) {
        self.v = v
        self.target = target
        self.total = total
        self.items = items
        self.warnings = warnings
    }
}
