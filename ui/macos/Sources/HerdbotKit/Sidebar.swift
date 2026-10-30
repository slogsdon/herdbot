import Foundation

public enum Avatar {
    /// A stable hue in 0..<1 for a slug (FNV-1a), so a bot keeps its colour between launches.
    /// Swift's own `hashValue` is randomised per process.
    public static func hue(for seed: String) -> Double {
        var hash: UInt32 = 2_166_136_261
        for byte in seed.utf8 {
            hash = (hash ^ UInt32(byte)) &* 16_777_619
        }
        return Double(hash % 360) / 360
    }

    /// The first letter, upper-cased, for the avatar disc.
    public static func initial(for name: String) -> String {
        name.first { $0.isLetter || $0.isNumber }.map { String($0).uppercased() } ?? "?"
    }
}

public struct SidebarRow: Identifiable, Hashable, Sendable {
    public let id: Recipient
    public let title: String
    public let preview: String
    public let lastAt: Int?
    public let working: Bool
    /// A bot that isn't running; its history is still readable.
    public let dimmed: Bool
    /// Colours the avatar and, in rooms, the speaker's name.
    public let slug: String
}

public enum Sidebar {
    /// Bots and rooms for the list, most recently active first (never-active ones last, by name).
    /// `filter` matches the title or slug, ignoring case.
    public static func rows(from snapshot: Snapshot, filter: String = "") -> (bots: [SidebarRow], rooms: [SidebarRow]) {
        let bots = snapshot.bots.map {
            SidebarRow(id: .bot($0.slug), title: $0.title.isEmpty ? $0.slug : $0.title, preview: $0.preview,
                       lastAt: $0.lastAt, working: $0.working, dimmed: !$0.up, slug: $0.slug)
        }
        let rooms = snapshot.rooms.map {
            SidebarRow(id: .room($0.slug), title: $0.name.isEmpty ? $0.slug : $0.name, preview: $0.preview,
                       lastAt: $0.lastAt, working: false, dimmed: $0.paused, slug: $0.slug)
        }
        return (ordered(matching(bots, filter)), ordered(matching(rooms, filter)))
    }

    /// The status menu's Bots section: at most `limit` bots, most recently active first.
    public static func topBots(from snapshot: Snapshot, limit: Int = 5) -> [BotView] {
        ordered(snapshot.bots) { ($0.lastAt, $0.title.isEmpty ? $0.slug : $0.title, $0.slug) }
            .prefix(limit)
            .map { $0 }
    }

    private static func matching(_ rows: [SidebarRow], _ filter: String) -> [SidebarRow] {
        let needle = filter.trimmingCharacters(in: .whitespaces)
        guard !needle.isEmpty else { return rows }
        return rows.filter { $0.title.localizedCaseInsensitiveContains(needle) || $0.slug.localizedCaseInsensitiveContains(needle) }
    }

    private static func ordered(_ rows: [SidebarRow]) -> [SidebarRow] {
        ordered(rows) { ($0.lastAt, $0.title, $0.slug) }
    }

    /// Recent activity first, then display name, then slug, so ties and missing timestamps keep a stable order.
    private static func ordered<T>(_ items: [T], key: (T) -> (lastAt: Int?, title: String, slug: String)) -> [T] {
        items.sorted {
            let before = key($0), after = key($1)
            switch (before.lastAt, after.lastAt) {
            case let (a?, b?) where a != b: return a > b
            case (_?, nil): return true
            case (nil, _?): return false
            default: break
            }
            let byName = before.title.localizedCaseInsensitiveCompare(after.title)
            if byName != .orderedSame { return byName == .orderedAscending }
            return before.slug.localizedCaseInsensitiveCompare(after.slug) == .orderedAscending
        }
    }
}
