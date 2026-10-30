import Foundation
import Observation

/// One `FROM=TO` row of a `map` setting, as the form lists it.
public struct SettingMapEntry: Identifiable, Hashable, Sendable {
    public let from: String
    public let to: String

    public var id: String { from }

    public init(from: String, to: String) {
        self.from = from
        self.to = to
    }
}

/// The Settings screen: loads the CLI's setting metadata for one scope, holds the form's drafts,
/// and reports a status per key. Every read and write goes through `SettingsCLI`; this type never
/// sees TOML or the filesystem.
@MainActor @Observable
public final class SettingsState {
    /// Which file the form edits: the global `herdbot.toml` or one bot's `bot.toml`.
    public enum Scope: Hashable, Sendable {
        case global
        case bot(String)

        /// The `--bot` argument; nil means the global scope.
        public var bot: String? {
            if case .bot(let slug) = self { slug } else { nil }
        }
    }

    /// What the last action on one key did. A failure never discards the typed value.
    public enum SaveStatus: Equatable, Sendable {
        case idle
        case saving
        case saved
        case failed(String)
    }

    /// The scope on screen. It only changes once its settings have loaded, so a failed switch
    /// leaves the previous form (and its unsaved values) intact.
    public private(set) var scope: Scope = .global
    public private(set) var settings: [SettingDescriptor] = []
    /// Why the settings could not be loaded; saving failures live on their own key instead.
    public private(set) var problem: String?
    public private(set) var loading = false
    public private(set) var isPresented = false
    private var status: [String: SaveStatus] = [:]
    private var writeIDs: [String: UUID] = [:]
    /// Drafts per scope, so switching between global and a bot keeps what was typed but not saved.
    private var drafts: [Scope: [String: String]] = [:]
    /// The value each draft started from, so a reload can refresh untouched fields and keep edited ones.
    private var baselines: [Scope: [String: String]] = [:]
    private var loadID = 0

    @ObservationIgnored private let cli: (any SettingsCLI)?

    public init(cli: (any SettingsCLI)?) {
        self.cli = cli
    }

    // MARK: - Navigation

    /// Show the screen and load `scope`; the status menu and the sidebar open it this way.
    public func present(_ scope: Scope = .global) async {
        isPresented = true
        await select(scope)
    }

    /// Back to the conversation. The loaded settings stay, so reopening is instant.
    public func dismiss() {
        isPresented = false
    }

    // MARK: - Loading

    /// Load another scope. On failure the current scope, descriptors and drafts stay as they are
    /// and `problem` says why.
    public func select(_ requested: Scope) async {
        guard let cli else {
            problem = CLIError.notFound.localizedDescription
            return
        }
        loadID += 1
        let id = loadID
        loading = true
        do {
            let loaded = try await cli.listSettings(bot: requested.bot)
            guard id == loadID else { return }
            scope = requested
            settings = loaded
            problem = nil
            status = [:]
            seedDrafts(for: requested, from: loaded)
        } catch {
            guard id == loadID else { return }
            problem = error.localizedDescription
        }
        if id == loadID { loading = false }
    }

    /// Re-read the scope on screen, e.g. after the CLI or the config file changed.
    public func reload() async {
        await select(scope)
    }

    public func descriptor(_ key: String) -> SettingDescriptor? {
        settings.first { $0.key == key }
    }

    /// Start a field from what the file holds now. A `map` edits one entry at a time, so its field
    /// starts empty; its current entries are listed separately.
    private func seed(_ setting: SettingDescriptor) -> String {
        setting.type == .map ? "" : setting.value.text
    }

    /// Refresh fields the user has not touched and keep the ones they have, so both a reload and a
    /// return to an earlier scope show the right thing.
    private func seedDrafts(for scope: Scope, from loaded: [SettingDescriptor]) {
        var scopeDrafts = drafts[scope] ?? [:]
        var scopeBaselines = baselines[scope] ?? [:]
        for setting in loaded {
            let fresh = seed(setting)
            let baseline = scopeBaselines[setting.key]
            if scopeDrafts[setting.key] == nil || scopeDrafts[setting.key] == baseline {
                scopeDrafts[setting.key] = fresh
            }
            scopeBaselines[setting.key] = fresh
        }
        // Drop drafts for keys the CLI no longer offers.
        let keys = Set(loaded.map(\.key))
        drafts[scope] = scopeDrafts.filter { keys.contains($0.key) }
        baselines[scope] = scopeBaselines.filter { keys.contains($0.key) }
    }

    // MARK: - Editing

    public func draft(for key: String) -> String {
        drafts[scope]?[key] ?? ""
    }

    /// Rewrite one field. Editing also clears an earlier failure for that key.
    public func edit(_ key: String, to text: String) {
        drafts[scope, default: [:]][key] = text
        if status[key] != .saving { status[key] = .idle }
    }

    /// Whether the field holds something a save would change.
    public func isDirty(_ key: String) -> Bool {
        guard let descriptor = descriptor(key) else { return false }
        return draft(for: key) != (baselines[scope]?[key] ?? seed(descriptor))
    }

    public func canSave(_ key: String) -> Bool {
        descriptor(key) != nil && status[key] != .saving && isDirty(key)
    }

    public func saveStatus(_ key: String) -> SaveStatus {
        status[key] ?? .idle
    }

    // MARK: - Writing

    /// `herdbot config set`: the CLI validates the draft, so a rejection is shown verbatim and the
    /// typed value stays in the field.
    public func save(_ key: String) async {
        guard let cli, descriptor(key) != nil else { return }
        let target = scope
        let writeID = UUID()
        writeIDs[key] = writeID
        status[key] = .saving
        do {
            let updated = try await cli.setSetting(key: key, value: draft(for: key), bot: target.bot)
            // An answer for a scope that is no longer on screen must not overwrite this form.
            guard scope == target, writeIDs[key] == writeID else { return clearInFlight(key, writeID: writeID) }
            apply(updated, in: target)
            status[key] = .saved
        } catch {
            guard scope == target, writeIDs[key] == writeID else { return clearInFlight(key, writeID: writeID) }
            status[key] = .failed(error.localizedDescription)
        }
    }

    /// `herdbot config unset`: remove the key so its default (or the global value) applies again.
    public func unset(_ key: String) async {
        guard let cli, descriptor(key) != nil else { return }
        let target = scope
        let writeID = UUID()
        writeIDs[key] = writeID
        status[key] = .saving
        do {
            let updated = try await cli.unsetSetting(key: key, bot: target.bot)
            guard scope == target, writeIDs[key] == writeID else { return clearInFlight(key, writeID: writeID) }
            apply(updated, in: target)
            status[key] = .saved
        } catch {
            guard scope == target, writeIDs[key] == writeID else { return clearInFlight(key, writeID: writeID) }
            status[key] = .failed(error.localizedDescription)
        }
    }

    /// A stale write may clear only its own in-flight status, never a newer write's.
    private func clearInFlight(_ key: String, writeID: UUID) {
        if writeIDs[key] == writeID, status[key] == .saving { status[key] = .idle }
    }

    private func apply(_ updated: SettingDescriptor, in target: Scope) {
        if let index = settings.firstIndex(where: { $0.key == updated.key }) {
            settings[index] = updated
        }
        // A saved map entry has been folded into the table; its one-entry field starts over.
        let fresh = seed(updated)
        drafts[target]?[updated.key] = fresh
        baselines[target]?[updated.key] = fresh
    }

    // MARK: - Map settings

    /// The entries a `map` setting holds now, sorted by their `FROM` path.
    public func mapEntries(_ key: String) -> [SettingMapEntry] {
        guard case .table(let entries)? = descriptor(key)?.value else { return [] }
        return entries
            .map { SettingMapEntry(from: $0.key, to: $0.value) }
            .sorted { $0.from < $1.from }
    }

    /// Add one entry through `config set <key> FROM=TO`, replacing the entry with the same `FROM`.
    /// `config unset` is the only way to remove entries, and it removes all of them.
    @discardableResult
    public func setMapEntry(_ key: String, _ entry: String) async -> Bool {
        let trimmed = entry.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else {
            status[key] = .failed("Enter FROM=TO, for example /workspace/vault=~/Documents/vault")
            return false
        }
        // Keep the typed entry if the CLI rejects it.
        edit(key, to: trimmed)
        await save(key)
        return saveStatus(key) == .saved
    }

    /// `config unset <key>`: drop every entry, so the setting falls back to its default.
    @discardableResult
    public func resetMap(_ key: String) async -> Bool {
        await unset(key)
        return saveStatus(key) == .saved
    }
}
