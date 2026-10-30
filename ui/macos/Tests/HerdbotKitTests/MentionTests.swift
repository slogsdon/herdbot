import Testing
@testable import HerdbotKit

@Suite struct Mentions {
    let options = [
        RecipientOption(recipient: .bot("alpha"), label: "Alpha"),
        RecipientOption(recipient: .bot("beta"), label: "Bee"),
        RecipientOption(recipient: .room("crew"), label: "Crew (room)")
    ]

    @Test func resolvesSlugsAndLabelsWithoutCaseSensitivityAndDeduplicates() throws {
        #expect(try MentionParser.resolve("@ALPHA @Alpha @Crew @crew", options: options) == [.bot("alpha"), .room("crew")])
        #expect(try MentionParser.resolve("@Bee", options: options) == [.bot("beta")])
    }

    @Test func doesNotTreatAnEmailAsAMention() throws {
        #expect(try MentionParser.resolve("email me at name@alpha.com", options: options).isEmpty)
    }

    @Test func unknownAndAmbiguousNamesAreErrors() {
        #expect(throws: MentionParser.Failure.unknown("missing")) { try MentionParser.resolve("@missing", options: options) }
        let duplicate = options + [RecipientOption(recipient: .room("alpha"), label: "Other")]
        #expect(throws: MentionParser.Failure.ambiguous("alpha")) { try MentionParser.resolve("@alpha", options: duplicate) }
    }

    @Test func suggestionsMatchSlugOrDisplayLabel() {
        #expect(MentionParser.suggestions(for: "cr", options: options).map(\.recipient) == [.room("crew")])
        #expect(MentionParser.suggestions(for: "be", options: options).map(\.recipient) == [.bot("beta")])
    }
}
