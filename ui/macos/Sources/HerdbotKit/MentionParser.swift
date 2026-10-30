import Foundation

public enum MentionParser {
    public enum Failure: Error, Equatable {
        case unknown(String)
        case ambiguous(String)
    }

    /// Resolve @slug tokens case-insensitively. Email addresses are left as ordinary text.
    public static func resolve(_ text: String, options: [RecipientOption]) throws -> [Recipient] {
        let pattern = #"(?<![A-Za-z0-9._%+-])@([A-Za-z0-9_-]+)"#
        let expression = try NSRegularExpression(pattern: pattern)
        let range = NSRange(text.startIndex..., in: text)
        var results: [Recipient] = []
        for match in expression.matches(in: text, range: range) {
            guard let tokenRange = Range(match.range(at: 1), in: text) else { continue }
            let token = String(text[tokenRange]).lowercased()
            let matches = options.filter { option in
                let slug: String
                switch option.recipient {
                case .bot(let value), .room(let value): slug = value
                }
                let label = option.label.replacingOccurrences(of: " (room)", with: "")
                return slug.lowercased() == token || label.lowercased() == token
            }
            let unique = Set(matches.map(\.recipient))
            guard !unique.isEmpty else { throw Failure.unknown(token) }
            guard unique.count == 1, let recipient = unique.first else { throw Failure.ambiguous(token) }
            if !results.contains(recipient) { results.append(recipient) }
        }
        return results
    }

    public static func suggestions(for query: String, options: [RecipientOption]) -> [RecipientOption] {
        let query = query.lowercased()
        return options.filter { option in
            let slug: String
            switch option.recipient {
            case .bot(let value), .room(let value): slug = value
            }
            let label = option.label.replacingOccurrences(of: " (room)", with: "")
            return query.isEmpty || slug.lowercased().hasPrefix(query) || label.lowercased().hasPrefix(query)
        }
    }
}
