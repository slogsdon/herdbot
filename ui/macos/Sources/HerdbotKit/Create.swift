import Foundation

/// Known routing classes offered by the form. A nil slug leaves `--class` out, using the routing default.
public struct TaskClass: Hashable, Sendable, Identifiable {
    /// The `--class` value, or nil for the routing default.
    public let slug: String?
    public let label: String

    public var id: String { slug ?? "default" }

    public init(slug: String?, label: String) {
        self.slug = slug
        self.label = label
    }

    public static let all: [TaskClass] = [
        TaskClass(slug: nil, label: "Default (routing table)"),
        TaskClass(slug: "explore", label: "explore"),
        TaskClass(slug: "shell", label: "shell"),
        TaskClass(slug: "bash", label: "bash"),
        TaskClass(slug: "browser_use", label: "browser_use"),
        TaskClass(slug: "computer_use", label: "computer_use"),
        TaskClass(slug: "debug", label: "debug"),
        TaskClass(slug: "media_review", label: "media_review"),
        TaskClass(slug: "custom", label: "custom"),
        TaskClass(slug: "code", label: "code"),
        TaskClass(slug: "review", label: "review"),
        TaskClass(slug: "plan", label: "plan"),
        TaskClass(slug: "research", label: "research"),
    ]

    /// The entry for a `--class` value, or nil for the default.
    public static func named(_ slug: String?) -> TaskClass? {
        all.first { $0.slug == slug }
    }
}

/// A room's turn-taking mode, spelled the way `herdbot room new --mode` spells it (`rooms::Mode`).
public enum RoomMode: String, CaseIterable, Sendable, Identifiable {
    case channel
    case roundRobin = "round_robin"
    case moderated

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .channel: "Channel"
        case .roundRobin: "Round robin"
        case .moderated: "Moderated"
        }
    }

    /// Who speaks next, from `rooms::route`, so the form says what each mode does.
    public var summary: String {
        switch self {
        case .channel: "Members answer only when you name them with @."
        case .roundRobin: "Each of your messages starts a round: every member once, in order."
        case .moderated: "Every new message goes to the moderator, who names who speaks."
        }
    }
}

/// The bot form's draft. Empty options are left out of the command, so the CLI's own defaults apply.
public struct NewBotDraft: Equatable, Sendable {
    public var name = ""
    /// Empty means the bot's own folder under the herdbot home, which is the CLI default.
    public var cwd = ""
    /// The `--class` value; nil applies the routing table's `default` class.
    public var taskClass: String?
    /// Written verbatim to `BOT.md`.
    public var instructions = ""

    public init(name: String = "", cwd: String = "", taskClass: String? = nil, instructions: String = "") {
        self.name = name
        self.cwd = cwd
        self.taskClass = taskClass
        self.instructions = instructions
    }
}

/// The room form's draft. `members` keeps the order the bots were picked in, because round-robin
/// rooms take their turns in that order.
public struct NewRoomDraft: Equatable, Sendable {
    public var name = ""
    public var members: [String] = []
    public var mode: RoomMode = .channel
    public var moderator: String?

    public init(name: String = "", members: [String] = [], mode: RoomMode = .channel, moderator: String? = nil) {
        self.name = name
        self.members = members
        self.mode = mode
        self.moderator = moderator
    }
}

/// What `herdbot new --json` prints.
public struct CreatedBot: Codable, Hashable, Sendable {
    public let slug: String
    /// The bot's folder under the herdbot home.
    public let path: String

    public init(slug: String, path: String) {
        self.slug = slug
        self.path = path
    }
}

/// What `herdbot room new --json` prints.
public struct CreatedRoom: Codable, Hashable, Sendable {
    public let slug: String

    public init(slug: String) {
        self.slug = slug
    }
}

/// The public parts of a template returned by `import-template --dry-run`.
public struct TemplatePreview: Codable, Equatable, Sendable {
    public let name: String
    public let description: String
    public let memories: [String]
    public let components: [String]
    public let notImported: [String]

    enum CodingKeys: String, CodingKey {
        case name, description, memories, components
        case notImported = "not_imported"
    }

    public init(name: String, description: String, memories: [String], components: [String], notImported: [String]) {
        self.name = name
        self.description = description
        self.memories = memories
        self.components = components
        self.notImported = notImported
    }
}

/// The CLI is the only writer for bots, rooms and imported templates.
public protocol CreateCLI: Sendable {
    func previewTemplate(url: String) async throws -> TemplatePreview
    func importTemplate(url: String, name: String, cwd: String?, taskClass: String?, paste: String) async throws -> CreatedBot
    /// `herdbot new <name> [--cwd DIR] [--class CLASS] [--instructions TEXT] --json`.
    func createBot(name: String, cwd: String?, taskClass: String?, instructions: String) async throws -> CreatedBot
    /// `herdbot room new <name> --members a,b[,c] [--mode MODE] [--moderator BOT] --json`.
    func createRoom(name: String, members: [String], mode: RoomMode, moderator: String?) async throws -> CreatedRoom
}

/// The checks both the form and `CreateState` use, so a disabled Create button and a refusal to run
/// always agree. The CLI validates again; these only catch what the form can see.
public enum CreateForm {
    /// Slugs herdbot keeps in its home, so a bot may not take them (`setup::bot_slug`).
    public static let reservedSlugs = ["rooms", "routing"]

    /// Mirrors `home::slug`: lower-case, non-alphanumerics to one `-`, at most 20 characters, and a
    /// leading `b` when the result would not start with a letter. Used for the preview and the
    /// reserved-name check; the CLI's own slug is what the app opens afterwards.
    public static func slug(for name: String) -> String {
        var out = ""
        for scalar in name.unicodeScalars {
            if scalar.isASCII && CharacterSet.alphanumerics.contains(scalar) {
                out.append(String(scalar).lowercased())
            } else if !out.isEmpty && !out.hasSuffix("-") {
                out.append("-")
            }
        }
        while out.hasSuffix("-") { out.removeLast() }
        if !(out.first?.isASCII == true && out.first?.isLowercase == true) { out = "b" + out }
        out = String(out.prefix(20))
        while out.hasSuffix("-") { out.removeLast() }
        return out
    }

    /// Why the bot form cannot be submitted, or nil when it can.
    public static func problem(for draft: NewBotDraft) -> String? {
        let name = draft.name.trimmingCharacters(in: .whitespacesAndNewlines)
        if name.isEmpty { return "A bot needs a name." }
        let slug = slug(for: name)
        if reservedSlugs.contains(slug) {
            return "\"\(slug)\" is a folder herdbot keeps in its home; pick another name."
        }
        if TaskClass.named(draft.taskClass) == nil {
            return "Unknown task class \"\(draft.taskClass ?? "")\"; pick one from the list."
        }
        return nil
    }

    /// Why the room form cannot be submitted, or nil when it can.
    public static func problem(for draft: NewRoomDraft) -> String? {
        if draft.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "A room needs a name." }
        if Set(draft.members).count < 2 { return "Pick at least two bots for the room's members." }
        if draft.mode == .moderated {
            guard let moderator = draft.moderator else { return "A moderated room needs a moderator." }
            if !draft.members.contains(moderator) { return "The moderator must be one of the room's members." }
        }
        return nil
    }

    /// The `--members` argument: one comma-separated argv entry.
    public static func membersArgument(_ members: [String]) -> String {
        members.joined(separator: ",")
    }
}
