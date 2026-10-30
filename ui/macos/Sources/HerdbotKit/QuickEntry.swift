import Foundation
import Observation

/// Where a quick-entry message goes.
public enum Recipient: Hashable, Sendable {
    case bot(String)
    case room(String)

    /// Stable text form, for remembering the last choice.
    public var key: String {
        switch self {
        case .bot(let slug): "bot:\(slug)"
        case .room(let slug): "room:\(slug)"
        }
    }

    /// The `<target>` argument of `herdbot history`.
    var historyTarget: String {
        switch self {
        case .bot(let slug): slug
        case .room(let slug): "room:\(slug)"
        }
    }

    init?(key: String) {
        if key.hasPrefix("bot:") { self = .bot(String(key.dropFirst(4))) }
        else if key.hasPrefix("room:") { self = .room(String(key.dropFirst(5))) }
        else { return nil }
    }
}

public struct RecipientOption: Hashable, Identifiable, Sendable {
    public let recipient: Recipient
    public let label: String
    public var id: Recipient { recipient }
}

@MainActor @Observable
public final class QuickEntryState {
    public enum Status: Equatable, Sendable {
        case idle
        case sending
        case queued
        case failed(String)
    }

    public var text = ""
    public private(set) var status: Status = .idle
    private var chosen: Recipient?
    @ObservationIgnored private let menu: MenuState
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private static let lastKey = "quickEntry.lastRecipient"

    public init(menu: MenuState, defaults: UserDefaults = .standard) {
        self.menu = menu
        self.defaults = defaults
    }

    /// Bots first, then rooms.
    public var options: [RecipientOption] {
        guard let snapshot = menu.snapshot else { return [] }
        let bots = snapshot.bots.map {
            RecipientOption(recipient: .bot($0.slug), label: $0.title.isEmpty ? $0.slug : $0.title)
        }
        let rooms = snapshot.rooms.map {
            RecipientOption(recipient: .room($0.slug), label: "\($0.name.isEmpty ? $0.slug : $0.name) (room)")
        }
        return bots + rooms
    }

    /// This session's pick, else the last one used, else the first option.
    public var recipient: Recipient? {
        let available = Set(options.map(\.recipient))
        let remembered = defaults.string(forKey: Self.lastKey).flatMap(Recipient.init(key:))
        return [chosen, remembered].compactMap { $0 }.first(where: available.contains)
            ?? options.first?.recipient
    }

    /// What the person sees for the current recipient, e.g. "Alpha" or "Crew (room)".
    public var recipientLabel: String? {
        options.first { $0.recipient == recipient }?.label
    }

    public var daemonDown: Bool { menu.snapshot?.daemon == "down" }

    public var canSend: Bool {
        recipient != nil && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && status != .sending
    }

    public func choose(_ recipient: Recipient) {
        chosen = recipient
        defaults.set(recipient.key, forKey: Self.lastKey)
    }

    /// Start a fresh message. The recipient choice carries over.
    public func reset() {
        text = ""
        status = .idle
        chosen = nil
    }

    /// Clear a failure so the person can edit and try again; the text was kept.
    public func dismissFailure() {
        if case .failed = status { status = .idle }
    }

    /// True when the message was queued. Dictated text is never sent on its own: this runs only
    /// when the person submits.
    @discardableResult
    public func send(using cli: any HerdbotCLI) async -> Bool {
        guard canSend, let recipient else { return false }
        let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
        status = .sending
        do {
            try await cli.send(to: recipient, text: message)
            text = ""
            status = .queued
            return true
        } catch {
            status = .failed(error.localizedDescription)
            return false
        }
    }
}

/// The quick-entry panel's focus-loss policy: focus leaving the panel only dismisses it once it has
/// stayed away for `grace`. Anything that brings focus back before then cancels the dismissal, so a
/// menu opened over the panel or a dictation HUD taking a moment doesn't close it.
///
/// Deliberately free of AppKit and of timers: the caller supplies the instant and does the
/// scheduling, so the timing here is deterministic and testable.
public struct QuickEntryFocusPolicy: Sendable {
    /// Long enough to outlast a menu or a dictation HUD taking focus, short enough to read as
    /// "you left the panel".
    public static let grace: Duration = .milliseconds(300)

    private var lostFocusAt: ContinuousClock.Instant?

    public init() {}

    /// Focus left. Repeat reports while it stays away don't push the deadline back; only regaining
    /// focus and then losing it again starts a new grace period.
    public mutating func focusLost(at now: ContinuousClock.Instant) {
        if lostFocusAt == nil { lostFocusAt = now }
    }

    /// Focus came back, so nothing is due.
    public mutating func focusRegained() {
        lostFocusAt = nil
    }

    /// When the dismissal is due, or nil while focus is where it belongs.
    public var dismissalDeadline: ContinuousClock.Instant? {
        lostFocusAt?.advanced(by: Self.grace)
    }

    /// True once focus has been away for the whole grace period.
    public func isDue(at now: ContinuousClock.Instant) -> Bool {
        guard let lostFocusAt else { return false }
        return now - lostFocusAt >= Self.grace
    }
}
