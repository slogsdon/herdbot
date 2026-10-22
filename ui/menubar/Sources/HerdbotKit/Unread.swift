import Foundation
import Observation

/// How many items of each conversation you have seen. A conversation seen for the first time
/// counts as read, so launching the app doesn't light up every bot that already has history.
@MainActor @Observable
public final class UnreadTracker {
    private var seen: [String: Int]
    @ObservationIgnored private let menu: MenuState
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let storeKey: String

    /// `home` scopes the stored counts to one herdbot home, so a throwaway home with the same bot names starts clean.
    public init(
        menu: MenuState,
        defaults: UserDefaults = .standard,
        home: String = ProcessInfo.processInfo.environment["HERDBOT_HOME"] ?? ""
    ) {
        self.menu = menu
        self.defaults = defaults
        storeKey = "chat.seen." + home
        seen = (defaults.dictionary(forKey: storeKey) as? [String: Int]) ?? [:]
    }

    /// New items since you last looked, 0 for a conversation not seen before `sync` ran.
    public func unread(_ recipient: Recipient) -> Int {
        guard let total = items(of: recipient), let known = seen[recipient.key] else { return 0 }
        return max(0, total - known)
    }

    /// Record every conversation the snapshot has that isn't known yet as read, and pull back a seen
    /// count that is above the current total (the history was reset).
    public func sync() {
        guard let snapshot = menu.snapshot else { return }
        var changed = false
        let all = snapshot.bots.map { (Recipient.bot($0.slug), $0.items) } + snapshot.rooms.map { (Recipient.room($0.slug), $0.items) }
        for (recipient, total) in all where seen[recipient.key].map({ $0 > total }) ?? true {
            seen[recipient.key] = total
            changed = true
        }
        if changed { persist() }
    }

    public func markSeen(_ recipient: Recipient) {
        guard let total = items(of: recipient), seen[recipient.key] != total else { return }
        seen[recipient.key] = total
        persist()
    }

    private func items(of recipient: Recipient) -> Int? {
        switch recipient {
        case .bot(let slug): menu.snapshot?.bots.first { $0.slug == slug }?.items
        case .room(let slug): menu.snapshot?.rooms.first { $0.slug == slug }?.items
        }
    }

    private func persist() { defaults.set(seen, forKey: storeKey) }
}
