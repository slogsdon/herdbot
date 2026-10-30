import Foundation
import Observation

/// The open conversation: the newest page, older pages on demand, and what is still pending.
@MainActor @Observable
public final class ChatState {
    public static let pageSize = 50

    public private(set) var selection: Recipient?
    public private(set) var problem: String?
    public private(set) var loading = false
    /// The sidebar's search text.
    public var filter = ""
    /// Whether the newest message is on screen; set by the chat view.
    public var atBottom = true
    private var persisted: [HistoryItem] = []
    private var pending: [HistoryItem] = []
    private var lastKey: Counts?
    @ObservationIgnored private let menu: MenuState
    @ObservationIgnored private let cli: (any HerdbotCLI)?

    /// What changes in a snapshot when the open conversation has news.
    struct Counts: Equatable {
        let items: Int
        let working: Bool
        let queued: Int
    }

    public enum SendStatus: Equatable, Sendable {
        case idle
        case sending
        case failed(String)
    }

    public private(set) var sendStatus: SendStatus = .idle
    private var drafts: [Recipient: String] = [:]
    private var collaboratorResultsByConversation: [Recipient: [Recipient: CollaborationTargetResult]] = [:]
    private var collaborators: [Recipient: [Recipient]] = [:]
    private var pendingCollaboration: [Recipient: (id: String, text: String, targets: [Recipient])] = [:]

    public init(menu: MenuState, cli: (any HerdbotCLI)?) {
        self.menu = menu
        self.cli = cli
    }

    /// What you are typing to the open conversation; each conversation keeps its own.
    public var draft: String {
        get { selection.flatMap { drafts[$0] } ?? "" }
        set {
            if let selection {
                if drafts[selection] != newValue {
                    pendingCollaboration[selection] = nil
                    collaboratorResultsByConversation[selection] = nil
                }
                drafts[selection] = newValue
            }
        }
    }

    public var selectedCollaborators: [Recipient] { selection.flatMap { collaborators[$0] } ?? [] }
    public var collaboratorResults: [Recipient: CollaborationTargetResult] { selection.flatMap { collaboratorResultsByConversation[$0] } ?? [:] }
    public var hasPendingCollaboration: Bool { selection.flatMap { pendingCollaboration[$0] } != nil }
    public var mentionOptions: [RecipientOption] {
        guard let snapshot = menu.snapshot else { return [] }
        return snapshot.bots.map { RecipientOption(recipient: .bot($0.slug), label: $0.title.isEmpty ? $0.slug : $0.title) }
            + snapshot.rooms.map { RecipientOption(recipient: .room($0.slug), label: "\($0.name.isEmpty ? $0.slug : $0.name) (room)") }
    }
    public var mentionQuery: String? {
        guard let selection, let text = drafts[selection],
              let range = text.range(of: #"(?:^|\s)@([A-Za-z0-9_-]*)$"#, options: .regularExpression) else { return nil }
        let token = text[range]
        return String(token.drop { $0 == " " || $0 == "\n" || $0 == "\t" }.dropFirst())
    }

    public func addCollaborator(_ recipient: Recipient) {
        guard let selection, recipient != selection, pendingCollaboration[selection] == nil else { return }
        var selected = collaborators[selection] ?? []
        if !selected.contains(recipient) {
            selected.append(recipient)
            pendingCollaboration[selection] = nil
            collaboratorResultsByConversation[selection] = nil
        }
        collaborators[selection] = selected
        if let text = drafts[selection], let range = text.range(of: #"(?:^|\s)@[A-Za-z0-9_-]*$"#, options: .regularExpression) {
            drafts[selection] = String(text[..<range.lowerBound]).trimmingCharacters(in: .whitespaces)
        }
    }

    public func removeCollaborator(_ recipient: Recipient) {
        guard let selection, pendingCollaboration[selection] == nil else { return }
        collaborators[selection]?.removeAll { $0 == recipient }
        pendingCollaboration[selection] = nil
        collaboratorResultsByConversation[selection] = nil
    }

    public var canSend: Bool {
        selection != nil && cli != nil && sendStatus != .sending
            && !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var daemonDown: Bool { menu.snapshot?.daemon == "down" }

    /// Queue the draft for the open conversation, then re-fetch so it shows at once as queued.
    /// Never called by dictation: only the person's Return. A failure keeps the draft.
    @discardableResult
    public func send() async -> Bool {
        guard canSend, let cli, let target = selection else { return false }
        let message = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        let parsedMentions: [Recipient]
        do {
            parsedMentions = try MentionParser.resolve(message, options: mentionOptions)
        } catch MentionParser.Failure.unknown(let name) {
            sendStatus = .failed("No bot or room matches @\(name)")
            return false
        } catch MentionParser.Failure.ambiguous(let name) {
            sendStatus = .failed("@\(name) matches more than one bot or room")
            return false
        } catch {
            sendStatus = .failed(error.localizedDescription)
            return false
        }
        let request: (id: String, text: String, targets: [Recipient])
        if let pending = pendingCollaboration[target], pending.text == message {
            request = pending
        } else {
            collaboratorResultsByConversation[target] = nil
            var targets = [target] + (collaborators[target] ?? []) + parsedMentions
            targets = targets.reduce(into: []) { if !$0.contains($1) { $0.append($1) } }
            request = (UUID().uuidString.replacingOccurrences(of: "-", with: ""), message, targets)
            pendingCollaboration[target] = request
        }
        let priorResults = collaboratorResultsByConversation[target] ?? [:]
        let retryTargets = request.targets.filter { priorResults[$0]?.status != "queued" }
        sendStatus = .sending
        do {
            let results = try await cli.collaborate(requestID: request.id, text: request.text, targets: retryTargets)
            var currentResults = collaboratorResultsByConversation[target] ?? [:]
            for result in results { currentResults[result.target] = result }
            collaboratorResultsByConversation[target] = currentResults
            let failures = request.targets.compactMap { recipient -> String? in
                guard currentResults[recipient]?.status == "queued" else {
                    return "\(recipient.key): \(currentResults[recipient]?.error ?? "delivery failed")"
                }
                return nil
            }
            guard failures.isEmpty else {
                sendStatus = .failed(failures.joined(separator: "; "))
                return false
            }
            drafts[target] = ""
            collaborators[target] = []
            pendingCollaboration[target] = nil
            collaboratorResultsByConversation[target] = nil
            sendStatus = .idle
            await refresh()
            return true
        } catch {
            sendStatus = .failed(error.localizedDescription)
            return false
        }
    }

    public func dismissSendFailure() {
        if case .failed = sendStatus { sendStatus = .idle }
    }

    /// The bot a blocked event's agent belongs to: a bot's main agent is its slug, a subagent's
    /// agent maps to the run's bot. nil for an agent herdbot doesn't know.
    public func bot(forAgent agent: String) -> Recipient? {
        guard let snapshot = menu.snapshot else { return nil }
        if snapshot.bots.contains(where: { $0.slug == agent }) { return .bot(agent) }
        return snapshot.runs.first { $0.agent == agent }.map { .bot($0.bot) }
    }

    /// Persisted items oldest first, then pending ones.
    public var items: [HistoryItem] { persisted + pending }

    public func cards(for recipient: Recipient) -> [Card] {
        guard case .bot(let slug) = recipient else { return [] }
        return (menu.snapshot?.cards ?? []).filter { $0.bot == slug }
    }

    public var canLoadOlder: Bool { (persisted.first?.n ?? 0) > 0 }

    public func select(_ recipient: Recipient?) async {
        guard recipient != selection else { return }
        selection = recipient
        persisted = []
        pending = []
        problem = nil
        lastKey = counts(for: recipient)
        await refresh()
    }

    /// Re-fetch the newest page when the snapshot says the open conversation changed.
    public func observe() async {
        let key = counts(for: selection)
        guard key != lastKey else { return }
        lastKey = key
        await refresh()
    }

    public func refresh() async {
        guard let cli, let target = selection else { return }
        do {
            let page = try await cli.history(of: target, before: nil, limit: Self.pageSize)
            guard target == selection else { return }
            let fresh = page.items.filter { !$0.isPending }
            var older = persisted
            if let firstNew = fresh.first?.n {
                older = older.filter { ($0.n ?? 0) < firstNew }
                // New items past a whole page leave a gap; drop what we had rather than show it.
                if let last = older.last?.n, last + 1 < firstNew { older = [] }
            }
            persisted = older + fresh
            pending = page.items.filter(\.isPending)
            problem = nil
        } catch {
            guard target == selection else { return }
            problem = Self.describe(error)
        }
    }

    /// Prepend the page before the oldest loaded item. Returns the id of the item that was first,
    /// so the view can keep its place.
    @discardableResult
    public func loadOlder() async -> String? {
        guard let cli, let target = selection, !loading, let oldest = persisted.first, let n = oldest.n, n > 0 else { return nil }
        loading = true
        defer { loading = false }
        do {
            let page = try await cli.history(of: target, before: n, limit: Self.pageSize)
            guard target == selection else { return nil }
            persisted = page.items.filter { !$0.isPending } + persisted
            return oldest.id
        } catch {
            problem = Self.describe(error)
            return nil
        }
    }

    private func counts(for recipient: Recipient?) -> Counts? {
        guard let snapshot = menu.snapshot, let recipient else { return nil }
        switch recipient {
        case .bot(let slug):
            return snapshot.bots.first { $0.slug == slug }.map { Counts(items: $0.items, working: $0.working, queued: $0.queued) }
        case .room(let slug):
            return snapshot.rooms.first { $0.slug == slug }.map { Counts(items: $0.items, working: false, queued: 0) }
        }
    }

    /// An older herdbot has no `history`; it answers with its usage text.
    static func describe(_ error: Error) -> String {
        let message = error.localizedDescription
        if message.contains("usage:") { return "update herdbot: this version has no `history` command" }
        return message
    }
}
