import Foundation
import Observation

/// The sheet behind the sidebar's "+": one draft per form, the checks that gate Create, and the two
/// CLI calls. Everything the CLI writes is reported back through `created`, so the window can open
/// the new bot or room; this type never reads or writes a file itself.
@MainActor @Observable
public final class CreateState {
    public enum Kind: Hashable, Sendable {
        case bot
        case room
    }

    /// What the last Create did. A failure never discards the draft, so the fix can be made in place.
    public enum Status: Equatable, Sendable {
        case idle
        case creating
        case failed(String)
    }

    public private(set) var isPresented = false
    public private(set) var kind: Kind = .bot
    public var bot = NewBotDraft() {
        didSet { if bot != oldValue { clearFailure() } }
    }
    public var room = NewRoomDraft() {
        didSet { if room != oldValue { clearFailure() } }
    }
    public private(set) var status: Status = .idle
    /// The conversation to open after a successful Create; nil until there is one.
    public private(set) var created: Recipient?

    @ObservationIgnored private let cli: (any CreateCLI)?
    @ObservationIgnored private let menu: MenuState?
    /// Identifies the create in flight, so a result that arrives after Cancel cannot reopen the sheet.
    @ObservationIgnored private var attempt = UUID()

    /// A nil `cli` means herdbot was not found: the form still opens, and Create stays disabled.
    public init(menu: MenuState? = nil, cli: (any CreateCLI)? = nil) {
        self.menu = menu
        self.cli = cli
    }

    /// The bots the room form offers: everything in the watch snapshot, by title.
    public var bots: [BotView] {
        (menu?.snapshot?.bots ?? []).sorted {
            $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending
        }
    }

    /// The display name for a slug, from the snapshot when it is there.
    public func title(for slug: String) -> String {
        guard let bot = bots.first(where: { $0.slug == slug }) else { return slug }
        return bot.title.isEmpty ? slug : "\(bot.title) (\(slug))"
    }

    public var isCreating: Bool { status == .creating }

    /// What is wrong with the draft on screen, or nil. Shown under the form, and why Create is off.
    public var problem: String? {
        guard cli != nil else { return "herdbot was not found. Finish setup before creating a bot or room." }
        return switch kind {
        case .bot: CreateForm.problem(for: bot)
        case .room: CreateForm.problem(for: room)
        }
    }

    /// The CLI's own answer to the last Create, shown verbatim.
    public var failure: String? {
        if case .failed(let message) = status { message } else { nil }
    }

    public var canCreate: Bool {
        cli != nil && !isCreating && problem == nil
    }

    // MARK: - Navigation

    /// Open the sheet on a fresh form; the kind comes from the "+" menu.
    public func present(_ kind: Kind) {
        self.kind = kind
        bot = NewBotDraft()
        room = NewRoomDraft()
        status = .idle
        created = nil
        attempt = UUID()
        isPresented = true
    }

    /// Close without creating. A create still in flight is abandoned: the CLI finishes it, the watch
    /// stream shows it, and the sheet does not reopen.
    public func dismiss() {
        isPresented = false
        status = .idle
        attempt = UUID()
    }

    // MARK: - Editing rooms

    /// Pick or unpick a member. The moderator always stays a member, so unpicking it moves the
    /// moderator to the first bot still picked.
    public func setMember(_ slug: String, isOn: Bool) {
        if isOn {
            guard !room.members.contains(slug) else { return }
            room.members.append(slug)
            if room.mode == .moderated, room.moderator == nil { room.moderator = slug }
        } else {
            room.members.removeAll { $0 == slug }
            if room.moderator == slug { room.moderator = room.members.first }
        }
    }

    /// Choose a mode. A moderated room needs a moderator, so the first member takes the role.
    public func setMode(_ mode: RoomMode) {
        room.mode = mode
        if mode == .moderated, room.moderator == nil { room.moderator = room.members.first }
    }

    // MARK: - Creating

    /// Run `herdbot new` or `herdbot room new` for the draft on screen. On success the sheet closes
    /// and `created` names what to open; a failure stays on screen with the draft intact.
    public func create() async {
        guard let cli, canCreate else { return }
        let id = UUID()
        attempt = id
        status = .creating
        do {
            let recipient: Recipient
            switch kind {
            case .bot:
                let name = bot.name.trimmingCharacters(in: .whitespacesAndNewlines)
                let cwd = bot.cwd.trimmingCharacters(in: .whitespacesAndNewlines)
                let created = try await cli.createBot(
                    name: name, cwd: cwd.isEmpty ? nil : cwd,
                    taskClass: bot.taskClass, instructions: bot.instructions)
                recipient = .bot(created.slug)
            case .room:
                let created = try await cli.createRoom(
                    name: room.name.trimmingCharacters(in: .whitespacesAndNewlines),
                    members: room.members, mode: room.mode,
                    // Only a moderated room has one; the CLI would store the field for any mode.
                    moderator: room.mode == .moderated ? room.moderator : nil)
                recipient = .room(created.slug)
            }
            guard attempt == id else { return }
            status = .idle
            isPresented = false
            self.created = recipient
        } catch {
            guard attempt == id else { return }
            status = .failed(error.localizedDescription)
        }
    }

    /// Editing after a rejection clears the message, so a second attempt reads as a fresh one.
    private func clearFailure() {
        if case .failed = status { status = .idle }
    }
}
