import Foundation
import Testing
@testable import HerdbotKit

/// Stands in for `herdbot config …`: answers from fixture descriptors and records every call.
final class FakeSettingsCLI: SettingsCLI, @unchecked Sendable {
    struct Call: Equatable {
        let command: String
        let key: String
        let value: String?
        let bot: String?
    }

    /// `config list` answers keyed by bot slug; `""` is the global scope.
    nonisolated(unsafe) var lists: [String: [SettingDescriptor]] = [:]
    nonisolated(unsafe) var listFailure: String?
    /// A rejection per command: `"set"`, `"unset"` or `"list"`.
    nonisolated(unsafe) var failures: [String: String] = [:]
    nonisolated(unsafe) var calls: [Call] = []
    /// Runs before a set/unset answers, so a test can hold a save open.
    nonisolated(unsafe) var beforeAnswering: (@Sendable () async -> Void)?

    func listSettings(bot: String?) async throws -> [SettingDescriptor] {
        calls.append(Call(command: "list", key: "", value: nil, bot: bot))
        if let listFailure { throw CLIError.failed(listFailure) }
        return lists[bot ?? ""] ?? []
    }

    func getSetting(key: String, bot: String?) async throws -> SettingDescriptor {
        calls.append(Call(command: "get", key: key, value: nil, bot: bot))
        return try descriptor(key, bot: bot, as: "get")
    }

    func setSetting(key: String, value: String, bot: String?) async throws -> SettingDescriptor {
        calls.append(Call(command: "set", key: key, value: value, bot: bot))
        if let beforeAnswering { await beforeAnswering() }
        let base = try descriptor(key, bot: bot, as: "set")
        return with(base, value: stored(base, from: value))
    }

    func unsetSetting(key: String, bot: String?) async throws -> SettingDescriptor {
        calls.append(Call(command: "unset", key: key, value: nil, bot: bot))
        if let beforeAnswering { await beforeAnswering() }
        let base = try descriptor(key, bot: bot, as: "unset")
        return with(base, value: base.defaultValue)
    }

    private func descriptor(_ key: String, bot: String?, as command: String) throws -> SettingDescriptor {
        if let failure = failures[command] { throw CLIError.failed(failure) }
        guard let setting = (lists[bot ?? ""] ?? []).first(where: { $0.key == key }) else {
            throw CLIError.failed("fake: no \(key) setting for \(bot ?? "global")")
        }
        return setting
    }

    /// What the real CLI would store for a raw value, enough to read the result back.
    private func stored(_ base: SettingDescriptor, from raw: String) -> SettingValue {
        switch base.type {
        case .integer: return .integer(Int(raw) ?? 0)
        case .boolean: return .flag(raw == "true")
        case .map:
            var entries: [String: String] = [:]
            if case .table(let existing) = base.value { entries = existing }
            if let separator = raw.firstIndex(of: "=") {
                let from = raw[..<separator].trimmingCharacters(in: .whitespaces)
                let to = raw[raw.index(after: separator)...].trimmingCharacters(in: .whitespaces)
                entries[from] = String(to)
            }
            return .table(entries)
        default: return .text(raw)
        }
    }

    private func with(_ base: SettingDescriptor, value: SettingValue) -> SettingDescriptor {
        SettingDescriptor(key: base.key, type: base.type, description: base.description,
                          defaultValue: base.defaultValue, value: value, choices: base.choices, apply: base.apply)
    }
}

/// Lets a test hold a save mid-flight and then release it.
actor Gate {
    private var arrived = false
    private var arrival: CheckedContinuation<Void, Never>?
    private var departure: CheckedContinuation<Void, Never>?

    func block() async {
        await withCheckedContinuation { continuation in
            departure = continuation
            arrived = true
            arrival?.resume()
            arrival = nil
        }
    }

    func waitUntilBlocked() async {
        if arrived { return }
        await withCheckedContinuation { arrival = $0 }
    }

    func release() {
        departure?.resume()
        departure = nil
    }
}

@MainActor
@Suite struct Settings {
    private func fake() throws -> FakeSettingsCLI {
        let cli = FakeSettingsCLI()
        cli.lists = [
            "": try settings(from: globalSettingsJSON),
            "helper": try settings(from: botSettingsJSON),
            "other": try settings(from: botSettingsJSON),
        ]
        return cli
    }

    @Test func loadsGlobalSettingsWithTheirValuesAsDrafts() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)

        #expect(state.scope == .global)
        #expect(state.problem == nil)
        #expect(state.settings.map(\.key) == ["approvals.mode", "memory.max_compactors", "paths", "cua.host"])
        #expect(state.draft(for: "approvals.mode") == "auto")
        #expect(state.draft(for: "memory.max_compactors") == "4")
        #expect(state.draft(for: "cua.host") == "")
        // A map edits one entry at a time, so its field starts empty.
        #expect(state.draft(for: "paths") == "")
        #expect(cli.calls == [FakeSettingsCLI.Call(command: "list", key: "", value: nil, bot: nil)])
    }

    @Test func withoutTheCLITheFormSaysHerdbotIsMissing() async {
        let state = SettingsState(cli: nil)
        await state.select(.global)
        #expect(state.problem == CLIError.notFound.localizedDescription)
        #expect(state.settings.isEmpty)
    }

    @Test func selectingABotLoadsItsSettings() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)
        await state.select(.bot("helper"))

        #expect(state.scope == .bot("helper"))
        #expect(state.settings.map(\.key) == ["title", "approvals", "memory"])
        #expect(state.draft(for: "title") == "Helper")
        #expect(state.draft(for: "memory") == "true")
    }

    @Test func aLoadErrorKeepsTheValuesOnScreenAndReportsIt() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)
        state.edit("cua.host", to: "builder@example")

        cli.listFailure = "cannot read ~/.herdbot/herdbot.toml: permission denied"
        await state.select(.bot("helper"))

        #expect(state.problem == "cannot read ~/.herdbot/herdbot.toml: permission denied")
        // The failed scope switch leaves the loaded scope, its descriptors and the typed value alone.
        #expect(state.scope == .global)
        #expect(state.settings.map(\.key).contains("cua.host"))
        #expect(state.draft(for: "cua.host") == "builder@example")
    }

    @Test func aSuccessfulLoadClearsAnEarlierLoadError() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        cli.listFailure = "daemon not running"
        await state.select(.global)
        #expect(state.problem == "daemon not running")

        cli.listFailure = nil
        await state.reload()
        #expect(state.problem == nil)
        #expect(state.settings.map(\.key) == ["approvals.mode", "memory.max_compactors", "paths", "cua.host"])
    }

    @Test func savingABotSettingWritesToThatBotOnly() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.bot("helper"))
        state.edit("memory", to: "false")
        await state.save("memory")

        #expect(cli.calls.last == FakeSettingsCLI.Call(command: "set", key: "memory", value: "false", bot: "helper"))
        #expect(state.saveStatus("memory") == .saved)
        #expect(state.descriptor("memory")?.value == .flag(false))
        #expect(state.draft(for: "memory") == "false")
        #expect(state.isDirty("memory") == false)

        await state.select(.bot("other"))
        state.edit("memory", to: "false")
        await state.save("memory")
        #expect(cli.calls.last == FakeSettingsCLI.Call(command: "set", key: "memory", value: "false", bot: "other"))
    }

    @Test func aRejectedSaveKeepsTheTypedValueAndShowsTheCLIError() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)
        state.edit("approvals.mode", to: "always")

        let message = #"invalid approvals.mode = "always" in /Users/me/.herdbot/herdbot.toml: expected one of "ask", "auto""#
        cli.failures["set"] = message
        await state.save("approvals.mode")

        #expect(state.saveStatus("approvals.mode") == .failed(message))
        #expect(state.draft(for: "approvals.mode") == "always")
        #expect(state.descriptor("approvals.mode")?.value == .text("auto"))
        #expect(state.isDirty("approvals.mode"))
    }

    @Test func unsetFallsBackToTheDefaultAndClearsTheDraft() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.bot("helper"))
        state.edit("memory", to: "false")

        await state.unset("memory")

        #expect(cli.calls.last == FakeSettingsCLI.Call(command: "unset", key: "memory", value: nil, bot: "helper"))
        #expect(state.saveStatus("memory") == .saved)
        #expect(state.descriptor("memory")?.value == .flag(false))
        #expect(state.draft(for: "memory") == "false")
        #expect(state.isDirty("memory") == false)
    }

    @Test func aFailedUnsetKeepsTheTypedValue() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.bot("helper"))
        state.edit("title", to: "Draft title")

        cli.failures["unset"] = "cannot write ~/.herdbot/bots/helper/bot.toml: read-only file system"
        await state.unset("title")

        #expect(state.saveStatus("title") == .failed("cannot write ~/.herdbot/bots/helper/bot.toml: read-only file system"))
        #expect(state.draft(for: "title") == "Draft title")
        #expect(state.descriptor("title")?.value == .text("Helper"))
    }

    @Test func aSaveThatFinishesAfterSwitchingScopesDoesNotApply() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.bot("helper"))
        state.edit("title", to: "Renamed")

        let gate = Gate()
        cli.beforeAnswering = { await gate.block() }
        let saving = Task { await state.save("title") }
        await gate.waitUntilBlocked()

        await state.select(.global)
        await gate.release()
        await saving.value

        // The write already went to the bot it was started for; its answer belongs to no visible form.
        #expect(cli.calls.filter { $0.command == "set" } == [FakeSettingsCLI.Call(command: "set", key: "title", value: "Renamed", bot: "helper")])
        #expect(state.scope == .global)
        #expect(state.settings.map(\.key) == ["approvals.mode", "memory.max_compactors", "paths", "cua.host"])
        #expect(state.saveStatus("title") == .idle)
    }

    @Test func anOldSaveDoesNotClearANewerSaveForTheSameKey() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.bot("helper"))
        state.edit("title", to: "Helper renamed")

        let oldGate = Gate()
        cli.beforeAnswering = { await oldGate.block() }
        let oldSave = Task { await state.save("title") }
        await oldGate.waitUntilBlocked()

        await state.select(.bot("other"))
        state.edit("title", to: "Other renamed")
        let newGate = Gate()
        cli.beforeAnswering = { await newGate.block() }
        let newSave = Task { await state.save("title") }
        await newGate.waitUntilBlocked()

        await oldGate.release()
        await oldSave.value
        #expect(state.scope == .bot("other"))
        #expect(state.saveStatus("title") == .saving)

        await newGate.release()
        await newSave.value
        #expect(state.saveStatus("title") == .saved)
        #expect(state.descriptor("title")?.value == .text("Other renamed"))
    }

    @Test func switchingScopesKeepsTypedValuesPerScope() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)
        state.edit("cua.host", to: "builder@example")
        state.edit("approvals.mode", to: "ask")

        await state.select(.bot("helper"))
        state.edit("title", to: "Helper Bot")

        await state.select(.global)
        #expect(state.draft(for: "cua.host") == "builder@example")
        #expect(state.draft(for: "approvals.mode") == "ask")

        await state.select(.bot("helper"))
        #expect(state.draft(for: "title") == "Helper Bot")
    }

    @Test func reloadPicksUpAValueChangedElsewhereButKeepsUnsavedEdits() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)
        state.edit("approvals.mode", to: "ask")

        // Something outside the app (another CLI run, an editor) changed cua.host.
        cli.lists[""] = cli.lists[""]?.map { setting in
            guard setting.key == "cua.host" else { return setting }
            return SettingDescriptor(key: setting.key, type: setting.type, description: setting.description,
                                     defaultValue: setting.defaultValue, value: .text("builder@example"),
                                     choices: setting.choices, apply: setting.apply)
        }
        await state.reload()

        // The untouched field follows the file; the unsaved edit survives the reload.
        #expect(state.draft(for: "cua.host") == "builder@example")
        #expect(state.draft(for: "approvals.mode") == "ask")
        #expect(state.isDirty("approvals.mode"))
        #expect(state.isDirty("cua.host") == false)
    }

    @Test func theMapControlShowsEntriesAndSetsOneAtATime() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)

        #expect(state.mapEntries("paths") == [SettingMapEntry(from: "/workspace/vault", to: "~/Documents/vault")])
        #expect(state.draft(for: "paths") == "")

        state.edit("paths", to: "  /srv/app = /Users/me/app  ")
        let saved = await state.setMapEntry("paths", state.draft(for: "paths"))
        #expect(saved)
        // Only the outer whitespace is trimmed here; the CLI owns `FROM=TO` parsing and its spacing.
        #expect(cli.calls.last == FakeSettingsCLI.Call(command: "set", key: "paths", value: "/srv/app = /Users/me/app", bot: nil))
        // Adding one entry leaves the other ones in place and empties the entry field.
        #expect(state.mapEntries("paths") == [
            SettingMapEntry(from: "/srv/app", to: "/Users/me/app"),
            SettingMapEntry(from: "/workspace/vault", to: "~/Documents/vault"),
        ])
        #expect(state.draft(for: "paths") == "")

        let reset = await state.resetMap("paths")
        #expect(reset)
        #expect(cli.calls.last == FakeSettingsCLI.Call(command: "unset", key: "paths", value: nil, bot: nil))
        #expect(state.mapEntries("paths").isEmpty)
    }

    @Test func anEmptyMapEntryIsRejectedWithoutCallingTheCLI() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)
        let calls = cli.calls.count

        let saved = await state.setMapEntry("paths", "   ")

        #expect(saved == false)
        #expect(cli.calls.count == calls)
        #expect(state.saveStatus("paths") == .failed("Enter FROM=TO, for example /workspace/vault=~/Documents/vault"))
    }

    @Test func presentingSelectsTheScopeAndDismissingKeepsIt() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        #expect(state.isPresented == false)

        await state.present(.bot("helper"))
        #expect(state.isPresented)
        #expect(state.scope == .bot("helper"))
        #expect(state.settings.map(\.key) == ["title", "approvals", "memory"])

        state.dismiss()
        #expect(state.isPresented == false)
        #expect(state.scope == .bot("helper"))
    }

    @Test func editingClearsThatKeysOldStatus() async throws {
        let cli = try fake()
        let state = SettingsState(cli: cli)
        await state.select(.global)
        state.edit("cua.host", to: "remote")
        cli.failures["set"] = "boom"
        await state.save("cua.host")
        #expect(state.saveStatus("cua.host") == .failed("boom"))

        state.edit("cua.host", to: "other")
        #expect(state.saveStatus("cua.host") == .idle)
        #expect(state.draft(for: "cua.host") == "other")
    }
}
