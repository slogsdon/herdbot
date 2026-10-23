import Foundation

public enum CLIError: Error, Equatable, LocalizedError {
    case notFound
    case unsupported(Int)
    case failed(String)

    public var errorDescription: String? {
        switch self {
        case .notFound: "herdbot not found (set HERDBOT_BIN or install it to ~/.cargo/bin)"
        case .unsupported(let v): "herdbot speaks protocol v\(v); update herdbot or this app"
        case .failed(let message): message
        }
    }
}

public protocol HerdbotCLI: Sendable {
    /// `herdbot watch --json`: a snapshot line first, then changes. Ends with an error when the
    /// process exits.
    func watch() -> AsyncThrowingStream<StreamLine, Error>
    /// `herdbot focus <agent>`.
    func focus(agent: String) async throws
    /// `herdbot send <bot> <text>` or `herdbot room send <room> <text>`: queues the text.
    func send(to recipient: Recipient, text: String) async throws
    /// `herdbot history <target> --json`: the newest page, or the `limit` items before item `before`.
    func history(of target: Recipient, before: Int?, limit: Int) async throws -> HistoryPage
}

public protocol CardCLI: Sendable {
    func answerCard(id: String, option: String) async throws
}

/// `herdbot config …`. A nil `bot` is the global scope; otherwise the scope is that bot.
public protocol SettingsCLI: Sendable {
    /// `herdbot config list [--bot <slug>] --json`.
    func listSettings(bot: String?) async throws -> [SettingDescriptor]
    func getSetting(key: String, bot: String?) async throws -> SettingDescriptor
    /// Writes through the CLI, which validates and preserves the TOML.
    func setSetting(key: String, value: String, bot: String?) async throws -> SettingDescriptor
    /// Removes the setting, so its default applies again.
    func unsetSetting(key: String, bot: String?) async throws -> SettingDescriptor
}

/// Mirrors the `herdbot config --json` contract version; bump with `config::CONTRACT_VERSION`.
public let supportedSettingsVersion = 1

/// The control a setting needs. `enum` is spelled `enumeration` in Swift.
public enum SettingType: String, Codable, Hashable, Sendable {
    case enumeration = "enum"
    case string
    case integer
    case boolean
    case map
}

/// When a saved value takes effect; a value never applies by itself mid-conversation.
public enum SettingApply: String, Codable, Hashable, Sendable {
    case immediate
    case nextBotStart = "next-bot-start"
    case daemonRestart = "daemon-restart"

    /// What the user must do for a saved value to take effect.
    public var label: String {
        switch self {
        case .immediate: "Takes effect immediately"
        case .nextBotStart: "Takes effect when the bot next starts"
        case .daemonRestart: "Takes effect after the daemon restarts"
        }
    }
}

/// A setting's `default` or `value`. The contract sends a string, number, boolean or table of
/// string to string, and `null` for a setting the file does not set.
public enum SettingValue: Hashable, Sendable {
    case text(String)
    case integer(Int)
    case flag(Bool)
    case table([String: String])
    case unset

    /// How the value reads in a form field: one `KEY=VALUE` line per table entry, empty when unset.
    public var text: String {
        switch self {
        case .text(let text): text
        case .integer(let number): String(number)
        case .flag(let flag): flag ? "true" : "false"
        case .table(let entries): entries.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }.joined(separator: "\n")
        case .unset: ""
        }
    }
}

/// One supported setting, as `herdbot config list|get|set|unset --json` describes it.
public struct SettingDescriptor: Hashable, Sendable, Identifiable {
    public let key: String
    public let type: SettingType
    public let description: String
    /// The registry's fallback, which applies when the file does not set the key.
    public let defaultValue: SettingValue
    /// What the file resolves to now; the default when the key is unset.
    public let value: SettingValue
    /// The accepted values of an `enumeration`; empty for every other type.
    public let choices: [String]
    public let apply: SettingApply

    public var id: String { key }

    public init(key: String, type: SettingType, description: String, defaultValue: SettingValue, value: SettingValue, choices: [String] = [], apply: SettingApply) {
        self.key = key
        self.type = type
        self.description = description
        self.defaultValue = defaultValue
        self.value = value
        self.choices = choices
        self.apply = apply
    }
}

extension SettingValue: Codable {
    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .unset
        } else if let flag = try? container.decode(Bool.self) {
            self = .flag(flag)
        } else if let number = try? container.decode(Int.self) {
            self = .integer(number)
        } else if let text = try? container.decode(String.self) {
            self = .text(text)
        } else if let entries = try? container.decode([String: String].self) {
            self = .table(entries)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "expected a string, integer, boolean, string table or null")
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .text(let text): try container.encode(text)
        case .integer(let number): try container.encode(number)
        case .flag(let flag): try container.encode(flag)
        case .table(let entries): try container.encode(entries)
        case .unset: try container.encodeNil()
        }
    }
}

extension SettingDescriptor: Decodable {
    enum CodingKeys: String, CodingKey {
        case key, type, description, defaultValue = "default", value, choices, apply
    }

    /// Every field but `choices` is required: a payload this build can't read is an error, not a
    /// silently empty form.
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            key: try c.decode(String.self, forKey: .key),
            type: try c.decode(SettingType.self, forKey: .type),
            description: try c.decode(String.self, forKey: .description),
            defaultValue: try c.decode(SettingValue.self, forKey: .defaultValue),
            value: try c.decode(SettingValue.self, forKey: .value),
            choices: try c.decodeIfPresent([String].self, forKey: .choices) ?? [],
            apply: try c.decode(SettingApply.self, forKey: .apply))
    }
}

/// `herdbot config list --json`.
struct SettingsList: Decodable {
    let settings: [SettingDescriptor]

    private enum CodingKeys: String, CodingKey { case version, settings }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let version = try c.decode(Int.self, forKey: .version)
        guard version == supportedSettingsVersion else { throw CLIError.unsupported(version) }
        settings = try c.decode([SettingDescriptor].self, forKey: .settings)
    }
}

/// `herdbot config get|set|unset --json`: one setting in the same envelope.
struct SettingsEntry: Decodable {
    let setting: SettingDescriptor

    private enum CodingKeys: String, CodingKey { case version, setting }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let version = try c.decode(Int.self, forKey: .version)
        guard version == supportedSettingsVersion else { throw CLIError.unsupported(version) }
        setting = try c.decode(SettingDescriptor.self, forKey: .setting)
    }
}

public enum StreamParser {
    /// nil for a line that isn't ours (malformed, or not a stream line). Throws `unsupported`
    /// for a protocol version this build doesn't know.
    public static func parse(_ line: String) throws -> StreamLine? {
        struct Head: Decodable { let v: Int }
        let data = Data(line.utf8)
        guard let head = try? JSONDecoder().decode(Head.self, from: data) else { return nil }
        guard head.v == supportedVersion else { throw CLIError.unsupported(head.v) }
        return try? JSONDecoder().decode(StreamLine.self, from: data)
    }
}
