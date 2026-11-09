import Foundation

/// Notification text. A body names the bot and what it is waiting on, cut short; the chat holds the
/// rest. The constants stay generic for when the event says nothing.
public enum NotificationCopy {
    public static let blockedTitle = "herdbot needs input"
    public static let blockedBody = "An agent needs your input. Open herdbot to review."
    public static let approvalTitle = "herdbot needs approval"
    public static let approvalBody = "An agent needs your approval. Open herdbot to review."
    /// How much of a reason or text one notification carries.
    public static let detailLimit = 140

    /// What a blocked event is stuck on: its reason, else its text, first non-blank line only, cut to
    /// `limit` characters. nil when the event carries neither.
    public static func detail(for event: Event, limit: Int = detailLimit) -> String? {
        guard let detail = firstLine(event.reason) ?? firstLine(event.text) else { return nil }
        return truncate(detail, to: limit)
    }

    /// "Alpha: approval: Bash" — the bot and what it waits on, or the generic copy when the event
    /// says nothing. `bot` is its display name; nil when herdbot doesn't know the agent.
    public static func body(bot: String?, event: Event, limit: Int = detailLimit) -> String {
        guard let detail = detail(for: event, limit: limit) else { return blockedBody }
        return bot.map { "\($0): \(detail)" } ?? detail
    }

    /// "helper: Trust this folder?" — the card's bot and title, or the generic approval copy.
    public static func body(bot: String?, card: Card, limit: Int = detailLimit) -> String {
        guard let title = firstLine(card.title) else { return approvalBody }
        let detail = truncate(title, to: limit)
        return bot.map { "\($0): \(detail)" } ?? detail
    }

    /// The first line with surrounding whitespace removed; nil when every line is blank.
    private static func firstLine(_ text: String?) -> String? {
        text?.split(separator: "\n", omittingEmptySubsequences: false)
            .lazy.map { $0.trimmingCharacters(in: .whitespaces) }
            .first { !$0.isEmpty }
    }

    private static func truncate(_ text: String, to limit: Int) -> String {
        guard limit > 0, text.count > limit else { return text }
        return String(text.prefix(limit - 1)) + "…"
    }
}

extension Snapshot {
    /// The name a bot goes by: its title, or its slug when the title is empty. nil for an unknown slug.
    public func botName(_ slug: String) -> String? {
        bots.first { $0.slug == slug }.map { $0.title.isEmpty ? $0.slug : $0.title }
    }
}
