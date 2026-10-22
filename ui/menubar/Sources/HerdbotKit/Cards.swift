import Foundation

public enum CardStyle: String, Codable, Sendable {
    case standard
    case destructive

    public init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = raw == "destructive" ? .destructive : .standard
    }
}

public struct CardOption: Codable, Hashable, Sendable, Identifiable {
    public let id: String
    public let label: String
    public let style: CardStyle

    public init(id: String, label: String, style: CardStyle = .standard) {
        self.id = id
        self.label = label
        self.style = style
    }
}

/// A prompt waiting for the user; `kind` picks the view.
public struct Card: Codable, Hashable, Sendable, Identifiable {
    public let id: String
    public let kind: String
    public let adapter: String
    public let bot: String
    public let agent: String
    public let title: String
    public let body: String
    public let options: [CardOption]
    public let at: Int

    public init(id: String, kind: String, adapter: String, bot: String, agent: String, title: String, body: String, options: [CardOption], at: Int) {
        self.id = id
        self.kind = kind
        self.adapter = adapter
        self.bot = bot
        self.agent = agent
        self.title = title
        self.body = body
        self.options = options
        self.at = at
    }
}
