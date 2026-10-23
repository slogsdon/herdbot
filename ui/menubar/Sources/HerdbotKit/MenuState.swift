import Foundation
import Observation

public enum IconState: Equatable, Sendable {
    case idle
    case running
    case blocked(Int)
    case daemonDown
    case unavailable
    case attention
}

extension IconState {
    /// Setup attention replaces quiet icons while blocked and missing-herdbot states remain visible.
    public func with(attention: Bool) -> IconState {
        guard attention else { return self }
        switch self {
        case .blocked, .unavailable: return self
        default: return .attention
        }
    }
}

/// Blocked events already announced. The first call only records, so launching the app
/// doesn't re-announce agents that were already waiting.
public struct NotificationTracker: Sendable {
    private var seen: Set<String> = []
    private var primed = false

    public init() {}

    public mutating func fresh(_ blocked: [Event]) -> [Event] {
        let keys = blocked.map { "\($0.agent)#\($0.at)" }
        defer {
            seen = Set(keys)
            primed = true
        }
        guard primed else { return [] }
        return zip(blocked, keys).filter { !seen.contains($0.1) }.map { $0.0 }
    }
}

@MainActor @Observable
public final class MenuState {
    public private(set) var snapshot: Snapshot?
    /// Why there is no usable snapshot: herdbot missing, wrong version, or watch ended.
    public private(set) var problem: String?
    /// Called once per newly blocked agent.
    @ObservationIgnored public var onBlocked: (Event) -> Void = { _ in }
    @ObservationIgnored private var tracker = NotificationTracker()
    @ObservationIgnored public var onCard: (Card) -> Void = { _ in }
    @ObservationIgnored private var cardTracker = CardTracker()

    public init() {}

    public var icon: IconState {
        guard problem == nil, let snapshot else { return .unavailable }
        let waiting = snapshot.blocked.count + snapshot.cards.count
        if waiting > 0 { return .blocked(waiting) }
        if snapshot.daemon == "down" { return .daemonDown }
        return snapshot.runs.isEmpty ? .idle : .running
    }

    /// The Bots section: the five most recently active bots, so the menu stays short.
    /// Every bot remains reachable from the chat window.
    public var topBots: [BotView] {
        snapshot.map { Sidebar.topBots(from: $0) } ?? []
    }

    /// Event lines are ignored: a blocked event also changes the next snapshot.
    public func apply(_ line: StreamLine) {
        guard line.type == "snapshot", let snapshot = line.snapshot else { return }
        problem = nil
        self.snapshot = snapshot
        for event in tracker.fresh(snapshot.blocked) { onBlocked(event) }
        for card in cardTracker.fresh(snapshot.cards) { onCard(card) }
    }

    public func fail(_ message: String) {
        problem = message
    }

    /// Follow `watch` until cancelled, reconnecting with a capped backoff.
    public func follow(_ cli: any HerdbotCLI) async {
        var delay = 1
        while !Task.isCancelled {
            do {
                for try await line in cli.watch() {
                    apply(line)
                    delay = 1
                }
            } catch {
                fail(error.localizedDescription)
            }
            try? await Task.sleep(for: .seconds(delay))
            delay = min(delay * 2, 10)
        }
    }
}
