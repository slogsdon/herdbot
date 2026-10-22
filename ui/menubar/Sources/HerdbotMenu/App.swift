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
    private let quickEntry: QuickEntryController
    private let hotkeyRegistered: Bool

    init() {
        let state = MenuState()
        let cli = ProcessCLI.locate().map { ProcessCLI(binary: $0) }
        let notifier = Notifier(cli: cli)
        state.onBlocked = { notifier.post($0) }
        if let cli {
            Task { await state.follow(cli) }
        } else {
            state.fail(CLIError.notFound.localizedDescription)
        }
        let quickEntry = QuickEntryController(state: QuickEntryState(menu: state), cli: cli)
        // ⌥Space. Not a modifier alone, so it can't clash with MacWhisper's Right ⌘ dictation key.
        hotkeyRegistered = GlobalHotkey().register(keyCode: kVK_Space, modifiers: optionKey) { quickEntry.toggle() }
        self.state = state
        self.cli = cli
        self.notifier = notifier
        self.quickEntry = quickEntry
    }

    var body: some Scene {
        MenuBarExtra {
            MenuContent(state: state, cli: cli, hotkeyRegistered: hotkeyRegistered, openQuickEntry: { quickEntry.show() })
        } label: {
            MenuIcon(icon: state.icon)
        }
    }
}
