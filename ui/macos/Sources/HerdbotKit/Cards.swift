import Foundation
import Observation

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

public struct CardTracker: Sendable {
    private var seen: Set<String> = []
    private var primed = false

    public init() {}

    public mutating func fresh(_ cards: [Card]) -> [Card] {
        defer {
            seen = Set(cards.map(\.id))
            primed = true
        }
        guard primed else { return [] }
        return cards.filter { !seen.contains($0.id) }
    }
}

public enum CardAnswerState: Equatable {
    case idle
    case answering(String)
    case failed(String)
}

@MainActor @Observable
public final class CardAnswers {
    private var states: [String: CardAnswerState] = [:]

    public init() {}

    public func state(for card: Card) -> CardAnswerState {
        states[card.id] ?? .idle
    }

    public func answer(_ card: Card, option: CardOption, using cli: (any CardCLI)?) async {
        if case .answering = state(for: card) { return }
        states[card.id] = .answering(option.id)
        guard let cli else {
            states[card.id] = .failed(CLIError.notFound.localizedDescription)
            return
        }
        do {
            try await cli.answerCard(id: card.id, option: option.id)
        } catch {
            states[card.id] = .failed(error.localizedDescription)
        }
    }
}
