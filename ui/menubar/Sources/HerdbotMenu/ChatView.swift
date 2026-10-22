import HerdbotKit
import SwiftUI

/// The selected conversation: a header, a blocked banner, and the scrollable messages.
struct ChatView: View {
    let menu: MenuState
    let chat: ChatState
    let cli: ProcessCLI?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        if let selection = chat.selection {
            VStack(spacing: 0) {
                header(selection)
                Divider()
                banner(selection)
                messages(selection)
            }
        } else {
            Text("Choose a bot or room").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    /// The header's title and status line, and the agent a bot's "Jump to pane" focuses.
    private func info(for selection: Recipient) -> (title: String, status: String, agent: String?) {
        switch selection {
        case .bot(let slug):
            let bot = menu.snapshot?.bots.first { $0.slug == slug }
            let status = bot?.working == true ? "working…" : (bot?.up == true ? "idle" : "not running")
            return (bot.map { $0.title.isEmpty ? slug : $0.title } ?? slug, status, slug)
        case .room(let slug):
            let room = menu.snapshot?.rooms.first { $0.slug == slug }
            let status = [room?.mode.replacingOccurrences(of: "_", with: " "), room?.paused == true ? "paused" : nil]
                .compactMap { $0 }.joined(separator: " · ")
            return (room.map { $0.name.isEmpty ? slug : $0.name } ?? slug, status, nil)
        }
    }

    private func header(_ selection: Recipient) -> some View {
        let info = info(for: selection)
        let runs = menu.snapshot?.runs.filter { selection == .bot($0.bot) } ?? []
        return HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 1) {
                Text(info.title).font(.headline)
                Text(info.status).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            ForEach(runs, id: \.id) { run in
                Button { Task { try? await cli?.focus(agent: run.agent) } } label: {
                    Label(run.description, systemImage: "terminal").lineLimit(1)
                }
                .buttonStyle(.bordered).controlSize(.small)
            }
            if let agent = info.agent {
                Button("Jump to pane") { Task { try? await cli?.focus(agent: agent) } }.controlSize(.small)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    @ViewBuilder private func banner(_ selection: Recipient) -> some View {
        if case .bot(let slug) = selection, let blocked = menu.snapshot?.blocked.first(where: { $0.agent == slug }) {
            HStack {
                Image(systemName: "hand.raised.fill").foregroundStyle(.orange)
                Text("Waiting on you: \(blocked.reason ?? "needs your input")").lineLimit(2)
                Spacer()
                Button("Jump to pane") { Task { try? await cli?.focus(agent: slug) } }.controlSize(.small)
            }
            .padding(10)
            .background(.orange.opacity(0.15))
        }
    }

    private func messages(_ selection: Recipient) -> some View {
        let isRoom: Bool = { if case .room = selection { true } else { false } }()
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 8) {
                    if chat.canLoadOlder {
                        ProgressView().controlSize(.small).frame(maxWidth: .infinity)
                            .onAppear { Task { if let kept = await chat.loadOlder() { proxy.scrollTo(kept, anchor: .top) } } }
                    }
                    ForEach(Timeline.entries(chat.items)) { entry in
                        switch entry {
                        case .day(let label): DaySeparator(label: label)
                        case .item(let item): MessageRow(item: item, showSpeaker: isRoom).id(item.id)
                        }
                    }
                    Color.clear.frame(height: 1).id("bottom")
                        .onAppear { chat.atBottom = true }
                        .onDisappear { chat.atBottom = false }
                }
                .padding(16)
            }
            .defaultScrollAnchor(.bottom)
            .id(selection)
            .onChange(of: chat.items.last?.id) {
                if chat.atBottom { proxy.scrollTo("bottom", anchor: .bottom) }
            }
            .overlay(alignment: .bottomTrailing) {
                if !chat.atBottom {
                    Button {
                        withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) { proxy.scrollTo("bottom", anchor: .bottom) }
                    } label: { Image(systemName: "arrow.down.circle.fill").font(.title) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).padding(14)
                    .help("Jump to latest")
                }
            }
            .overlay {
                if let problem = chat.problem {
                    Text(problem).foregroundStyle(.red).padding()
                } else if chat.items.isEmpty && !chat.loading {
                    Text("No messages yet").foregroundStyle(.secondary)
                }
            }
        }
    }
}

/// The window: sidebar and chat. The Dock icon shows only while it is open.
struct ChatWindow: View {
    let menu: MenuState
    let chat: ChatState
    let cli: ProcessCLI?

    var body: some View {
        NavigationSplitView {
            SidebarView(menu: menu, chat: chat)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 340)
        } detail: {
            ChatView(menu: menu, chat: chat, cli: cli)
        }
        .frame(minWidth: 720, minHeight: 480)
        .task(id: menu.snapshot) { await chat.observe() }
        .onAppear {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willCloseNotification)) { note in
            if (note.object as? NSWindow)?.title == "herdbot" { NSApp.setActivationPolicy(.accessory) }
        }
    }
}
