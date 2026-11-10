import AppKit
import HerdbotKit
import SwiftUI

/// Bot, room and template forms. Create asks `CreateState`; the CLI owns every bot write.
///
/// States: the form defaults to the kind the "+" menu asked for; Create is disabled while the draft
/// is incomplete, while a create is running and when herdbot is missing, and the reason is always
/// on screen; a CLI rejection appears verbatim above the buttons and the draft stays. Return
/// creates, Escape cancels, and every field carries its own accessibility label. No custom motion
/// is used, so the sheet's system transition already follows Reduce Motion.
struct CreateSheet: View {
    let state: CreateState

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 2) {
                Text(state.heading).font(.title3.weight(.semibold))
                Text(state.kind == .template ? "Preview a Grok Bot URL, then paste any components the page withholds." : state.kind == .bot
                     ? "Creates a folder under the herdbot home with its instructions in BOT.md."
                     : "Creates a room from bots that already exist.")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            switch state.kind {
            case .bot: BotForm(state: state)
            case .room: RoomForm(state: state)
            case .template: TemplateForm(state: state)
            }
            message
            footer
        }
        .padding(20)
        .frame(width: 520)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(state.heading)
    }

    /// Why Create is off, or what the CLI said. Never both: a rejection is the more urgent message.
    @ViewBuilder private var message: some View {
        if let failure = state.failure {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
                Text(failure)
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .accessibilityElement(children: .combine)
        } else if let problem = state.problem {
            Text(problem).font(.callout).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var footer: some View {
        HStack(spacing: 10) {
            if state.isBusy {
                ProgressView().controlSize(.small).accessibilityLabel(state.status == .previewing ? "Loading preview" : "Creating")
            }
            Spacer()
            Button("Cancel") { state.dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Create") { Task { await state.create() } }
                .keyboardShortcut(.defaultAction)
                .disabled(!state.canCreate)
                .help(state.problem ?? (state.isCreating ? "Creating…" : "Create it"))
                .accessibilityLabel(state.kind == .room ? "Create room" : "Create bot")
        }
    }
}

/// A label above its control, so a long field label never squeezes the field.
private struct Field<Content: View>: View {
    let label: String
    var note: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label).font(.callout.weight(.medium))
            content
            if let note {
                Text(note).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

private struct BotForm: View {
    let state: CreateState
    @FocusState private var nameFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Field(label: "Name", note: state.bot.name.trimmingCharacters(in: .whitespaces).isEmpty
                  ? nil : "Slug: \(CreateForm.slug(for: state.bot.name))") {
                TextField("Helper", text: binding(\.name))
                    .textFieldStyle(.roundedBorder)
                    .focused($nameFocused)
                    .disabled(state.isBusy)
                    .accessibilityLabel("Bot name")
            }
            Field(label: "Working directory", note: "Leave empty for the bot's own folder under the herdbot home.") {
                HStack(spacing: 8) {
                    TextField("Bot folder", text: binding(\.cwd))
                        .textFieldStyle(.roundedBorder)
                        .font(.system(.body, design: .monospaced))
                        .disabled(state.isBusy)
                        .accessibilityLabel("Working directory")
                    Button("Choose…") { chooseFolder() }
                        .disabled(state.isBusy)
                        .accessibilityLabel("Choose working directory")
                }
            }
            Field(label: "Task class", note: "The routing class for this bot's main agent; subagents still pick their own.") {
                Picker("Task class", selection: binding(\.taskClass)) {
                    ForEach(TaskClass.all) { option in
                        Text(option.label).tag(option.slug)
                    }
                }
                .pickerStyle(.menu)
                .labelsHidden()
                .frame(maxWidth: 280, alignment: .leading)
                .disabled(state.isBusy)
                .accessibilityLabel("Task class")
            }
            if state.kind != .template {
                Field(label: "Instructions", note: "Written to BOT.md. Leave empty and edit the file later.") {
                    ZStack(alignment: .topLeading) {
                        TextEditor(text: binding(\.instructions))
                            .font(.body)
                            .scrollContentBackground(.hidden)
                            .padding(6)
                            .frame(height: 110)
                            .disabled(state.isBusy)
                            .accessibilityLabel("Bot instructions")
                        if state.bot.instructions.isEmpty {
                            Text("What this bot is for and how it should answer.")
                                .foregroundStyle(.tertiary)
                                .padding(.top, 12).padding(.leading, 11)
                                .allowsHitTesting(false)
                        }
                    }
                    .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(.separator))
                }
            }
        }
        // A sheet has one obvious starting point; the rest of the fields follow with Tab.
        .task { nameFocused = true }
    }

    private func binding<T>(_ keyPath: WritableKeyPath<NewBotDraft, T>) -> Binding<T> {
        Binding(get: { state.bot[keyPath: keyPath] }, set: { state.bot[keyPath: keyPath] = $0 })
    }

    /// The CLI expands `~` and checks the directory, so the panel only has to fill the field.
    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true
        panel.prompt = "Choose"
        panel.message = "Where this bot's agents start. Leave empty to use the bot's own folder."
        if !state.bot.cwd.isEmpty { panel.directoryURL = URL(fileURLWithPath: state.bot.cwd) }
        if panel.runModal() == .OK, let url = panel.url { state.bot.cwd = url.path }
    }
}

private struct RoomForm: View {
    let state: CreateState
    @FocusState private var nameFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Field(label: "Name") {
                TextField("Crew", text: Binding(get: { state.room.name }, set: { state.room.name = $0 }))
                    .textFieldStyle(.roundedBorder)
                    .focused($nameFocused)
                    .disabled(state.isBusy)
                    .accessibilityLabel("Room name")
            }
            Field(label: "Members", note: memberNote) {
                members
            }
            Field(label: "Mode", note: state.room.mode.summary) {
                Picker("Mode", selection: Binding(get: { state.room.mode }, set: { state.setMode($0) })) {
                    ForEach(RoomMode.allCases) { mode in Text(mode.label).tag(mode) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .disabled(state.isBusy)
                .accessibilityLabel("Room mode")
            }
            if state.room.mode == .moderated {
                Field(label: "Moderator", note: "The moderator takes every new message and names who speaks next.") {
                    if state.room.members.isEmpty {
                        Text("Pick members first.").font(.callout).foregroundStyle(.secondary)
                    } else {
                        Picker("Moderator", selection: Binding(get: { state.room.moderator }, set: { state.room.moderator = $0 })) {
                            ForEach(state.room.members, id: \.self) { slug in
                                Text(state.title(for: slug)).tag(Optional(slug))
                            }
                        }
                        .pickerStyle(.menu)
                        .labelsHidden()
                        .frame(maxWidth: 300, alignment: .leading)
                        .disabled(state.isBusy)
                        .accessibilityLabel("Moderator")
                    }
                }
            }
        }
        .task { nameFocused = true }
    }

    @ViewBuilder private var members: some View {
        if state.bots.isEmpty {
            Text("There are no bots yet. Create a bot first, then the room can have members.")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(state.bots, id: \.slug) { bot in
                        Toggle(isOn: Binding(
                            get: { state.room.members.contains(bot.slug) },
                            set: { state.setMember(bot.slug, isOn: $0) }
                        )) {
                            Text(bot.title.isEmpty ? bot.slug : "\(bot.title) (\(bot.slug))")
                                .lineLimit(1)
                        }
                        .toggleStyle(.checkbox)
                        .disabled(state.isBusy)
                        .accessibilityLabel("Member \(bot.title.isEmpty ? bot.slug : bot.title)")
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(6)
            }
            .frame(height: 120)
            .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(.separator))
        }
    }

    /// Picked bots stay in the order they were picked: round-robin rooms turn in that order.
    private var memberNote: String {
        let picked = state.room.members.map { state.title(for: $0) }
        guard !picked.isEmpty else { return "Pick at least two bots." }
        return "In turn order: \(picked.joined(separator: " → "))"
    }
}

private struct TemplateForm: View {
    let state: CreateState
    @FocusState private var urlFocused: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Field(label: "Template URL", note: "https://x.ai/bot/<id> or https://x.ai/bot/marketplace/bots/<slug>") {
                    HStack {
                        TextField("https://x.ai/bot/…", text: Binding(get: { state.templateURL }, set: { state.templateURL = $0 }))
                            .textFieldStyle(.roundedBorder)
                            .focused($urlFocused)
                            .disabled(state.isBusy)
                            .accessibilityLabel("Template URL")
                            .onSubmit { Task { await state.previewTemplate() } }
                        Button("Preview") { Task { await state.previewTemplate() } }
                            .disabled(!state.canPreview)
                            .accessibilityLabel("Preview template")
                    }
                }
                if let preview = state.preview {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(preview.name).font(.headline)
                        Text(preview.description)
                        ForEach(Array(preview.memories.enumerated()), id: \.offset) { _, memory in
                            Text(memory)
                        }
                        Text("Components: " + (preview.components.isEmpty ? "none listed" : preview.components.joined(separator: ", ")))
                            .font(.callout).foregroundStyle(.secondary)
                        Text("Not imported: " + (preview.notImported.isEmpty ? "none" : preview.notImported.joined(separator: ", ") + " — open the template in Grok Bot and paste them below."))
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    .textSelection(.enabled)
                    Field(label: "Paste components", note: "Use ## Memories, ## Skill: Name (optional Description:), or ## Routine: Name (optional Cron:). Schedules start disabled; no cron keeps prose only.") {
                        TextEditor(text: Binding(get: { state.paste }, set: { state.paste = $0 }))
                            .font(.body)
                            .frame(height: 110)
                            .disabled(state.isBusy)
                            .accessibilityLabel("Pasted template components")
                    }
                    BotForm(state: state)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxHeight: 540)
        .task { urlFocused = true }
    }
}
