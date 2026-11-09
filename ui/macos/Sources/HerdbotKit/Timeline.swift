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

public enum MarkdownListMarker: Hashable, Sendable {
    case bullet
    case ordered(Int)
}

public enum MarkdownBlock: Hashable, Sendable {
    case text(String)
    case heading(level: Int, text: String)
    case listItem(depth: Int, marker: MarkdownListMarker, text: String)
    case code(language: String?, body: String)
}

public enum Markdown {
    /// Splits fenced code blocks (```), ATX headings and list items from the text around them.
    /// An unclosed fence runs to the end. Everything outside a fence keeps its source text, for inline markdown.
    public static func blocks(_ source: String) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        var text: [Substring] = []
        var code: [Substring] = []
        var language: String?
        var inFence = false
        // The indent of each open list level, outermost first. Only list items feed it.
        var indents: [Int] = []

        func flushText() {
            let joined = text.joined(separator: "\n").trimmingCharacters(in: .newlines)
            if !joined.isEmpty { blocks.append(.text(joined)) }
            text = []
        }

        func flushPending() {
            flushText()
            indents = []
        }

        for line in source.split(separator: "\n", omittingEmptySubsequences: false) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") {
                if inFence {
                    blocks.append(.code(language: language, body: code.joined(separator: "\n")))
                    code = []
                    inFence = false
                } else {
                    flushPending()
                    let name = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces)
                    language = name.isEmpty ? nil : name
                    inFence = true
                }
            } else if inFence {
                code.append(line)
            } else if trimmed.isEmpty {
                flushPending()
            } else if let heading = heading(line) {
                flushPending()
                blocks.append(.heading(level: heading.level, text: heading.text))
            } else if let item = listItem(line) {
                flushText()
                while let last = indents.last, last > item.indent { indents.removeLast() }
                if indents.last != item.indent { indents.append(item.indent) }
                blocks.append(.listItem(depth: indents.count - 1, marker: item.marker, text: item.text))
            } else {
                // Unsupported source stays visible, and also ends the open list indentation.
                indents = []
                text.append(line)
            }
        }
        if inFence { blocks.append(.code(language: language, body: code.joined(separator: "\n"))) }
        flushText()
        return blocks
    }

    /// `#` through `######` followed by a space, as in CommonMark. `#tag` is not a heading.
    private static func heading(_ line: Substring) -> (level: Int, text: String)? {
        let indent = line.prefix(while: { $0 == " " }).count
        guard indent <= 3 else { return nil }
        let content = line.dropFirst(indent)
        let level = content.prefix(while: { $0 == "#" }).count
        guard (1...6).contains(level) else { return nil }
        let rest = content.dropFirst(level)
        guard let space = rest.first, space == " " || space == "\t" else { return nil }
        let text = rest.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return nil }
        return (level, text)
    }

    /// `- item`, `* item`, `1. item` and `1) item`, with the indentation that nests them.
    private static func listItem(_ line: Substring) -> (indent: Int, marker: MarkdownListMarker, text: String)? {
        let indent = line.prefix(while: { $0 == " " || $0 == "\t" }).count
        var rest = line.dropFirst(indent)
        let marker: MarkdownListMarker
        if let first = rest.first, "-*+".contains(first) {
            marker = .bullet
            rest = rest.dropFirst()
        } else {
            let digits = rest.prefix(while: { $0.isASCII && $0.isNumber })
            guard (1...9).contains(digits.count), let number = Int(digits) else { return nil }
            guard let separator = rest.dropFirst(digits.count).first, separator == "." || separator == ")" else { return nil }
            marker = .ordered(number)
            rest = rest.dropFirst(digits.count + 1)
        }
        // The marker needs a space and some content; a bare `-` or `1.` stays text.
        guard let space = rest.first, space == " " || space == "\t" else { return nil }
        let content = rest.trimmingCharacters(in: .whitespaces)
        guard !content.isEmpty else { return nil }
        return (indent, marker, content)
    }
}
