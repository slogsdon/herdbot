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
                Composer(chat: chat, title: info(for: selection).title, hasHerdbot: cli != nil)
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
                }
                .padding(16)
            }
            .defaultScrollAnchor(.bottom)
            // Content no taller than the container leaves offset 0, so the inequality holds: at the bottom.
            .onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.contentOffset.y + geometry.containerSize.height >= geometry.contentSize.height - 24
            } action: { _, nearBottom in
                chat.atBottom = nearBottom
            }
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

/// Where you type. Return sends; the system's own newline key adds a line. Nothing is sent without
/// a keypress, so dictated text can be read first.
struct Composer: View {
    let chat: ChatState
    let title: String
    let hasHerdbot: Bool
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            note
            TextField("Message \(title)", text: Binding(get: { chat.draft }, set: { chat.draft = $0 }), axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 15))
                .lineLimit(1...6)
                .focused($focused)
                // Not disabled while sending: that drops focus. canSend already ignores a second Return.
                .onSubmit {
                    Task {
                        await chat.send()
                        focused = true
                    }
                }
                .onChange(of: chat.draft) { chat.dismissSendFailure() }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        // Take focus when a conversation opens; the short wait lets the window become key first.
        .task(id: chat.selection) {
            try? await Task.sleep(for: .milliseconds(80))
            focused = true
        }
    }

    @ViewBuilder private var note: some View {
        if !hasHerdbot {
            Text("herdbot not found").font(.caption).foregroundStyle(.red)
        } else if case .failed(let message) = chat.sendStatus {
            Text(message).font(.caption).foregroundStyle(.red).lineLimit(2)
        } else if chat.daemonDown {
            Text("daemon not running; messages wait until it is").font(.caption).foregroundStyle(.orange)
        }
    }
}

/// The window: sidebar and chat. The Dock icon shows only while it is open.
struct ChatWindow: View {
    let menu: MenuState
    let chat: ChatState
    let unread: UnreadTracker
    let cli: ProcessCLI?
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        NavigationSplitView {
            SidebarView(menu: menu, chat: chat, unread: unread)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 340)
        } detail: {
            ChatView(menu: menu, chat: chat, cli: cli)
        }
        .frame(minWidth: 720, minHeight: 480)
        .task(id: menu.snapshot) {
            await chat.observe()
            unread.sync()
            markSelectionSeen()
        }
        .onChange(of: chat.selection) { markSelectionSeen() }
        .onChange(of: scenePhase) { markSelectionSeen() }
        .onAppear {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willCloseNotification)) { note in
            if (note.object as? NSWindow)?.title == "herdbot" { NSApp.setActivationPolicy(.accessory) }
        }
    }

    /// What is on screen counts as read: only while this window is the active one.
    private func markSelectionSeen() {
        if scenePhase == .active, let selection = chat.selection { unread.markSeen(selection) }
    }
}
