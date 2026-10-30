import HerdbotKit
import SwiftUI

/// Settings for the global config or one bot, rendered from the CLI's metadata. Every read and
/// write goes through `herdbot config`, so this view never parses or rewrites TOML itself.
struct SettingsView: View {
    let state: SettingsState
    let menu: MenuState

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if let problem = state.problem {
                HStack(alignment: .firstTextBaseline) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                    Text(problem).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    Spacer()
                    Button("Try again") { Task { await state.reload() } }
                }
                .padding(12)
                Divider()
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    ForEach(state.settings) { setting in
                        SettingRow(state: state, setting: setting)
                    }
                    if state.loading && state.settings.isEmpty {
                        ProgressView().controlSize(.small)
                    } else if state.settings.isEmpty && state.problem == nil {
                        Text("herdbot reports no settings for this scope.").foregroundStyle(.secondary)
                    }
                }
                .padding(16)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private var header: some View {
        HStack(spacing: 10) {
            Text("Settings").font(.headline)
            Picker("Scope", selection: Binding(get: { state.scope }, set: { scope in Task { await state.select(scope) } })) {
                Text("Global").tag(SettingsState.Scope.global)
                ForEach(botSlugs, id: \.self) { slug in
                    Text(slug).tag(SettingsState.Scope.bot(slug))
                }
            }
            .labelsHidden()
            .frame(maxWidth: 220)
            if state.loading { ProgressView().controlSize(.small) }
            Spacer()
            Button("Reload") { Task { await state.reload() } }
                .controlSize(.small)
                .disabled(state.loading)
            Button("Done") { state.dismiss() }
                .controlSize(.small)
                .keyboardShortcut(.defaultAction)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    /// Every bot the snapshot knows, plus the one on screen if it is missing from the snapshot.
    private var botSlugs: [String] {
        var slugs = (menu.snapshot?.bots ?? []).map(\.slug)
        if let current = state.scope.bot, !slugs.contains(current) { slugs.append(current) }
        return slugs
    }
}

/// One setting: its metadata, a control that matches its type, and save/use-default actions.
private struct SettingRow: View {
    let state: SettingsState
    let setting: SettingDescriptor

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(setting.key).font(.system(.body, design: .monospaced).weight(.semibold))
            Text(setting.description).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            control
            if setting.type == .map { entries }
            HStack(spacing: 6) {
                Text("Default: \(defaultLabel)").font(.caption).foregroundStyle(.secondary)
                Text("·").font(.caption).foregroundStyle(.secondary)
                Text(setting.apply.label).font(.caption).foregroundStyle(.secondary)
                Spacer()
                status
            }
            HStack(spacing: 8) {
                if setting.type == .map {
                    Button("Add or replace entry") { Task { await state.setMapEntry(setting.key, state.draft(for: setting.key)) } }
                        .disabled(state.saveStatus(setting.key) == .saving)
                    Button("Reset all entries") { Task { await state.resetMap(setting.key) } }
                        .disabled(state.saveStatus(setting.key) == .saving)
                } else {
                    Button("Save") { Task { await state.save(setting.key) } }
                        .disabled(!state.canSave(setting.key))
                    Button("Use default") { Task { await state.unset(setting.key) } }
                        .disabled(state.saveStatus(setting.key) == .saving)
                }
            }
            .controlSize(.small)
        }
    }

    @ViewBuilder private var control: some View {
        switch setting.type {
        case .enumeration:
            Picker("", selection: draft) {
                ForEach(setting.choices, id: \.self) { choice in Text(choice).tag(choice) }
                // A value the CLI no longer lists still shows, instead of silently picking another.
                if !setting.choices.contains(state.draft(for: setting.key)) {
                    Text(state.draft(for: setting.key)).tag(state.draft(for: setting.key))
                }
            }
            .labelsHidden()
            .frame(maxWidth: 240)
        case .boolean:
            Toggle("", isOn: Binding(get: { state.draft(for: setting.key) == "true" },
                                    set: { state.edit(setting.key, to: $0 ? "true" : "false") }))
                .labelsHidden()
                .toggleStyle(.switch)
        case .integer:
            TextField("", text: draft).frame(width: 140).monospacedDigit()
        case .string, .map:
            TextField(setting.type == .map ? "FROM=TO" : "", text: draft)
                .textFieldStyle(.roundedBorder)
                .font(.system(.body, design: .monospaced))
        }
    }

    /// A map's current entries. `herdbot config` replaces one entry at a time and can only unset
    /// the whole key, so there is no per-entry delete here.
    @ViewBuilder private var entries: some View {
        let entries = state.mapEntries(setting.key)
        if entries.isEmpty {
            Text("No entries").font(.callout).foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 3) {
                ForEach(entries) { entry in
                    HStack(spacing: 6) {
                        Text(entry.from)
                        Image(systemName: "arrow.right").font(.caption2).foregroundStyle(.secondary)
                        Text(entry.to)
                    }
                    .font(.system(.callout, design: .monospaced))
                    .textSelection(.enabled)
                }
            }
        }
    }

    private var draft: Binding<String> {
        Binding(get: { state.draft(for: setting.key) }, set: { state.edit(setting.key, to: $0) })
    }

    private var defaultLabel: String {
        switch setting.defaultValue {
        case .unset: "not set"
        case .table(let entries) where entries.isEmpty: "none"
        case .text(let text) where text.isEmpty: "empty"
        default: setting.defaultValue.text
        }
    }

    @ViewBuilder private var status: some View {
        switch state.saveStatus(setting.key) {
        case .saving:
            ProgressView().controlSize(.mini)
        case .saved:
            Label("Saved; \(setting.apply.label.lowercased())", systemImage: "checkmark.circle.fill")
                .font(.caption).foregroundStyle(.green)
        case .failed(let message):
            Text(message).font(.caption).foregroundStyle(.red).lineLimit(3).textSelection(.enabled)
        case .idle:
            if state.isDirty(setting.key) {
                Text("Unsaved changes").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}
