import AppKit
import Carbon.HIToolbox
import HerdbotKit
import SwiftUI

@main
struct HerdbotMenuApp: App {
    // A plain `let`: the App struct is built once, and @State is a macro whose plugin needs full Xcode.
    private let state: MenuState
    private let cli: ProcessCLI?
    private let notifier: Notifier
    private let notificationStatus: NotificationStatus
    private let quickEntry: QuickEntryController
    private let chat: ChatState
    private let unread: UnreadTracker
    private let opener = WindowOpener()
    private let hotkeyRegistered: Bool

    init() {
        let state = MenuState()
        let cli = ProcessCLI.locate().map { ProcessCLI(binary: $0) }
        let notificationStatus = NotificationStatus()
        let notifier = Notifier(cli: cli, status: notificationStatus)
        state.onBlocked = { notifier.post($0) }
        if let cli {
            Task { await state.follow(cli) }
        } else {
            state.fail(CLIError.notFound.localizedDescription)
        }
        let quickEntry = QuickEntryController(state: QuickEntryState(menu: state), cli: cli, hotkeyLabel: "⌥Space")
        // ⌥Space. Not a modifier alone, so it can't clash with MacWhisper's Right ⌘ dictation key.
        hotkeyRegistered = GlobalHotkey().register(keyCode: kVK_Space, modifiers: optionKey) { quickEntry.toggle() }
        self.state = state
        self.cli = cli
        self.notifier = notifier
        self.notificationStatus = notificationStatus
        self.quickEntry = quickEntry
        let chat = ChatState(menu: state, cli: cli)
        self.chat = chat
        unread = UnreadTracker(menu: state)
        let opener = self.opener
        // A blocked notification opens that bot's chat; an agent we can't place still focuses its pane.
        notifier.openChat = { agent in
            guard let bot = chat.bot(forAgent: agent) else { return false }
            Task { await chat.select(bot) }
            NSApp.activate(ignoringOtherApps: true)
            opener.open()
            return true
        }
    }

    var body: some Scene {
        // Opened from the menu, not at launch.
        Window("herdbot", id: "chat") {
            ChatWindow(menu: state, chat: chat, unread: unread, cli: cli)
        }
        .defaultLaunchBehavior(.suppressed)
        MenuBarExtra {
            MenuContent(state: state, notifications: notificationStatus, cli: cli, hotkeyRegistered: hotkeyRegistered, openQuickEntry: { quickEntry.show() })
        } label: {
            MenuIcon(icon: state.icon).modifier(OpenerCapture(opener: opener))
        }
    }
}
