import Foundation
import Testing
@testable import HerdbotKit

/// Answers `herdbot new` and `herdbot room new` without a filesystem and records what it was asked.
final class FakeCreateCLI: CreateCLI, @unchecked Sendable {
    struct BotCall: Equatable {
        let name: String
        let cwd: String?
        let taskClass: String?
        let instructions: String
    }

    struct RoomCall: Equatable {
        let name: String
        let members: [String]
        let mode: RoomMode
        let moderator: String?
    }

    nonisolated(unsafe) var botCalls: [BotCall] = []
    nonisolated(unsafe) var roomCalls: [RoomCall] = []
    /// The CLI's stderr, surfaced as the sheet's error.
    nonisolated(unsafe) var failure: String?
    /// Runs before an answer, so a test can hold a create open.
    nonisolated(unsafe) var beforeAnswering: (@Sendable () async -> Void)?

    func createBot(name: String, cwd: String?, taskClass: String?, instructions: String) async throws -> CreatedBot {
        botCalls.append(BotCall(name: name, cwd: cwd, taskClass: taskClass, instructions: instructions))
        if let beforeAnswering { await beforeAnswering() }
        if let failure { throw CLIError.failed(failure) }
        let slug = CreateForm.slug(for: name)
        return CreatedBot(slug: slug, path: "/tmp/Bots/\(slug)")
    }

    func createRoom(name: String, members: [String], mode: RoomMode, moderator: String?) async throws -> CreatedRoom {
        roomCalls.append(RoomCall(name: name, members: members, mode: mode, moderator: moderator))
        if let beforeAnswering { await beforeAnswering() }
        if let failure { throw CLIError.failed(failure) }
        return CreatedRoom(slug: CreateForm.slug(for: name))
    }
}

@Suite struct CreateFormTests {
    @Test func slugMatchesTheCLIRule() {
        #expect(CreateForm.slug(for: "Helper Bot") == "helper-bot")
        #expect(CreateForm.slug(for: "  Helper!!  ") == "helper")
        #expect(CreateForm.slug(for: "a very long bot name here") == "a-very-long-bot-name")
        // A name that would not start with a letter gets one, as `home::slug` does.
        #expect(CreateForm.slug(for: "9 lives") == "b9-lives")
        #expect(CreateForm.slug(for: "!!!") == "b")
        #expect(CreateForm.slug(for: "K Helper") == "helper")
    }

    @Test func botFormSaysWhatIsMissing() {
        #expect(CreateForm.problem(for: NewBotDraft()) == "A bot needs a name.")
        #expect(CreateForm.problem(for: NewBotDraft(name: "   ")) == "A bot needs a name.")
        let reserved = CreateForm.problem(for: NewBotDraft(name: "Rooms"))
        #expect(reserved?.contains("herdbot keeps") == true, "\(reserved ?? "")")
        let unknown = CreateForm.problem(for: NewBotDraft(name: "Helper", taskClass: "wizard"))
        #expect(unknown?.contains("Unknown task class") == true, "\(unknown ?? "")")
        // Every class the picker offers, and no class at all, is accepted.
        #expect(CreateForm.problem(for: NewBotDraft(name: "Helper")) == nil)
        for option in TaskClass.all {
            #expect(CreateForm.problem(for: NewBotDraft(name: "Helper", taskClass: option.slug)) == nil)
        }
    }

    @Test func roomFormSaysWhatIsMissing() {
        #expect(CreateForm.problem(for: NewRoomDraft()) == "A room needs a name.")
        #expect(CreateForm.problem(for: NewRoomDraft(name: "Crew")) == "Pick at least two bots for the room's members.")
        #expect(CreateForm.problem(for: NewRoomDraft(name: "Crew", members: ["alpha"]))?.contains("at least two") == true)
        #expect(CreateForm.problem(for: NewRoomDraft(name: "Crew", members: ["alpha", "alpha"]))?.contains("at least two") == true)
        let noModerator = CreateForm.problem(for: NewRoomDraft(name: "Crew", members: ["alpha", "beta"], mode: .moderated))
        #expect(noModerator == "A moderated room needs a moderator.")
        let stranger = CreateForm.problem(for: NewRoomDraft(name: "Crew", members: ["alpha", "beta"], mode: .moderated, moderator: "zed"))
        #expect(stranger == "The moderator must be one of the room's members.")
        #expect(CreateForm.problem(for: NewRoomDraft(name: "Crew", members: ["alpha", "beta"])) == nil)
        #expect(CreateForm.problem(for: NewRoomDraft(name: "Crew", members: ["alpha", "beta"], mode: .moderated, moderator: "beta")) == nil)
        // A channel room ignores a leftover moderator.
        #expect(CreateForm.problem(for: NewRoomDraft(name: "Crew", members: ["alpha", "beta"], moderator: "zed")) == nil)
    }

    @Test func membersAreOneCommaSeparatedArgument() {
        #expect(CreateForm.membersArgument(["alpha", "beta", "cy"]) == "alpha,beta,cy")
    }
}

@MainActor
@Suite struct Create {
    private func state(_ cli: any CreateCLI, bots: [BotView] = [BotView(slug: "alpha", title: "Alpha", up: true)]) -> CreateState {
        let menu = MenuState()
        menu.apply(line(snapshot(bots: bots)))
        return CreateState(menu: menu, cli: cli)
    }

    @Test func creatingABotPassesTheFormAndOpensIt() async {
        let cli = FakeCreateCLI()
        let state = state(cli)
        state.present(.bot)
        #expect(state.isPresented)
        #expect(state.kind == .bot)
        #expect(!state.canCreate, "an empty form cannot be created")
        state.bot.name = "  Helper Bot  "
        state.bot.cwd = "  /tmp/repo  "
        state.bot.taskClass = "explore"
        state.bot.instructions = "Be terse.\nTwo lines."
        #expect(state.canCreate)
        await state.create()
        #expect(cli.botCalls == [FakeCreateCLI.BotCall(name: "Helper Bot", cwd: "/tmp/repo", taskClass: "explore", instructions: "Be terse.\nTwo lines.")])
        #expect(state.created == .bot("helper-bot"))
        #expect(!state.isPresented, "the sheet closes when the bot exists")
        #expect(state.status == .idle)
    }

    @Test func anEmptyDirectoryAndClassAreLeftOut() async {
        let cli = FakeCreateCLI()
        let state = state(cli)
        state.present(.bot)
        state.bot.name = "Helper"
        state.bot.cwd = "   "
        await state.create()
        #expect(cli.botCalls == [FakeCreateCLI.BotCall(name: "Helper", cwd: nil, taskClass: nil, instructions: "")])
    }

    @Test func aRejectionStaysOnScreenWithTheDraft() async {
        let cli = FakeCreateCLI()
        cli.failure = "bot helper already exists; edit /tmp/Bots/helper/BOT.md or pick another name"
        let state = state(cli)
        state.present(.bot)
        state.bot.name = "Helper"
        state.bot.instructions = "Keep me."
        await state.create()
        #expect(state.failure == "bot helper already exists; edit /tmp/Bots/helper/BOT.md or pick another name")
        #expect(state.isPresented, "a failure keeps the sheet open")
        #expect(state.bot.name == "Helper")
        #expect(state.bot.instructions == "Keep me.")
        #expect(state.created == nil)
        // Editing clears the message and Create is available again for a retry.
        state.bot.name = "Helper Two"
        #expect(state.failure == nil)
        #expect(state.canCreate)
    }

    @Test func createIsDisabledWhileACreateIsRunning() async {
        let gate = Gate()
        let cli = FakeCreateCLI()
        cli.beforeAnswering = { await gate.block() }
        let state = state(cli)
        state.present(.bot)
        state.bot.name = "Helper"
        let running = Task { await state.create() }
        await gate.waitUntilBlocked()
        #expect(state.isCreating)
        #expect(!state.canCreate)
        // A second Return while the first is in flight must not create a second bot.
        await state.create()
        #expect(cli.botCalls.count == 1)
        await gate.release()
        await running.value
        #expect(state.created == .bot("helper"))
    }

    @Test func cancellingMidFlightDropsTheResult() async {
        let gate = Gate()
        let cli = FakeCreateCLI()
        cli.beforeAnswering = { await gate.block() }
        let state = state(cli)
        state.present(.bot)
        state.bot.name = "Helper"
        let running = Task { await state.create() }
        await gate.waitUntilBlocked()
        state.dismiss()
        await gate.release()
        await running.value
        #expect(state.created == nil, "a dismissed sheet does not reopen or steal the selection")
        #expect(!state.isPresented)
    }

    @Test func withoutHerdbotCreateStaysOff() async {
        let state = CreateState(menu: MenuState(), cli: nil)
        state.present(.bot)
        state.bot.name = "Helper"
        #expect(!state.canCreate)
        #expect(state.problem?.contains("herdbot was not found") == true)
        await state.create()
        #expect(state.status == .idle)
    }

    @Test func creatingARoomSendsItsMembersModeAndModerator() async {
        let cli = FakeCreateCLI()
        let state = state(cli, bots: [
            BotView(slug: "beta", title: "Beta", up: true),
            BotView(slug: "alpha", title: "Alpha", up: true),
        ])
        #expect(state.bots.map(\.slug) == ["alpha", "beta"], "the member list is by title")
        state.present(.room)
        state.room.name = "Crew"
        state.setMember("beta", isOn: true)
        state.setMember("alpha", isOn: true)
        state.setMember("alpha", isOn: true) // picking twice keeps one entry
        #expect(state.room.members == ["beta", "alpha"], "members keep the order they were picked in")
        #expect(state.canCreate)
        state.setMode(.moderated)
        #expect(state.room.moderator == "beta", "the first member moderates until changed")
        await state.create()
        #expect(cli.roomCalls == [FakeCreateCLI.RoomCall(name: "Crew", members: ["beta", "alpha"], mode: .moderated, moderator: "beta")])
        #expect(state.created == .room("crew"))
        #expect(!state.isPresented)
    }

    @Test func aChannelRoomSendsNoModerator() async {
        let cli = FakeCreateCLI()
        let state = state(cli)
        state.present(.room)
        state.room.name = "Crew"
        state.setMember("alpha", isOn: true)
        state.setMember("beta", isOn: true)
        state.room.moderator = "alpha" // left over from a moderated draft
        await state.create()
        #expect(cli.roomCalls == [FakeCreateCLI.RoomCall(name: "Crew", members: ["alpha", "beta"], mode: .channel, moderator: nil)])
    }

    @Test func theModeratorFollowsTheMembers() async {
        let state = state(FakeCreateCLI(), bots: [
            BotView(slug: "alpha", title: "Alpha", up: true),
            BotView(slug: "beta", title: "Beta", up: true),
            BotView(slug: "cy", title: "Cy", up: true),
        ])
        state.present(.room)
        state.setMember("alpha", isOn: true)
        state.setMember("beta", isOn: true)
        state.setMode(.moderated)
        #expect(state.room.moderator == "alpha")
        state.room.name = "Crew"
        // Unpicking the moderator moves the role to a bot still in the room.
        state.setMember("alpha", isOn: false)
        #expect(state.room.members == ["beta"])
        #expect(state.room.moderator == "beta")
        // An empty room has no moderator to name.
        state.setMember("beta", isOn: false)
        #expect(state.room.moderator == nil)
        #expect(state.problem == "Pick at least two bots for the room's members.")
    }

    @Test func openingTheSheetAgainStartsClean() async {
        let cli = FakeCreateCLI()
        cli.failure = "no bot \"zed\"; known: alpha"
        let state = state(cli)
        state.present(.bot)
        state.bot.name = "Helper"
        await state.create()
        #expect(state.failure != nil)
        state.dismiss()
        state.present(.room)
        #expect(state.kind == .room)
        #expect(state.bot.name.isEmpty)
        #expect(state.failure == nil)
        #expect(state.created == nil)
    }
}
