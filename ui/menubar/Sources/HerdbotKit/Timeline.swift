import Foundation

public enum TimelineEntry: Hashable, Identifiable, Sendable {
    case day(String)
    case item(HistoryItem)

    public var id: String {
        switch self {
        case .day(let label): "day:\(label)"
        case .item(let item): item.id
        }
    }
}

public enum Timeline {
    /// The items with a day label before the first item of each calendar day.
    public static func entries(_ items: [HistoryItem], calendar: Calendar = .current, now: Date = .now) -> [TimelineEntry] {
        var out: [TimelineEntry] = []
        var lastDay: DateComponents?
        for item in items {
            let date = Date(timeIntervalSince1970: TimeInterval(item.at))
            let day = calendar.dateComponents([.year, .month, .day], from: date)
            if day != lastDay {
                out.append(.day(dayLabel(for: date, now: now, calendar: calendar)))
                lastDay = day
            }
            out.append(.item(item))
        }
        return out
    }

    public static func dayLabel(for date: Date, now: Date, calendar: Calendar) -> String {
        if calendar.isDate(date, inSameDayAs: now) { return "Today" }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(date, inSameDayAs: yesterday) {
            return "Yesterday"
        }
        var style = Date.FormatStyle(date: .omitted, time: .omitted, calendar: calendar, timeZone: calendar.timeZone)
        style = calendar.isDate(date, equalTo: now, toGranularity: .year)
            ? style.weekday(.wide).month(.wide).day()
            : style.year().month(.wide).day()
        return date.formatted(style)
    }
}

public enum MarkdownBlock: Hashable, Sendable {
    case text(String)
    case code(language: String?, body: String)
}

public enum Markdown {
    /// Splits fenced code blocks (```) from the text around them. An unclosed fence runs to the end.
    /// Everything outside a fence is left for inline markdown.
    public static func blocks(_ source: String) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        var text: [Substring] = []
        var code: [Substring] = []
        var language: String?
        var inFence = false

        func flushText() {
            let joined = text.joined(separator: "\n").trimmingCharacters(in: .newlines)
            if !joined.isEmpty { blocks.append(.text(joined)) }
            text = []
        }

        for line in source.split(separator: "\n", omittingEmptySubsequences: false) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") {
                if inFence {
                    blocks.append(.code(language: language, body: code.joined(separator: "\n")))
                    code = []
                    inFence = false
                } else {
                    flushText()
                    let name = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces)
                    language = name.isEmpty ? nil : name
                    inFence = true
                }
            } else if inFence {
                code.append(line)
            } else {
                text.append(line)
            }
        }
        if inFence { blocks.append(.code(language: language, body: code.joined(separator: "\n"))) }
        flushText()
        return blocks
    }
}
