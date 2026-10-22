import AppKit
import HerdbotKit
import SwiftUI

@main
struct HerdbotMenuApp: App {
    // A plain `let`: the App struct is built once, and @State is a macro whose plugin needs full Xcode.
    private let state: MenuState
    private let cli: ProcessCLI?
    private let notifier: Notifier

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
        self.state = state
        self.cli = cli
        self.notifier = notifier
    }

    var body: some Scene {
        MenuBarExtra {
            MenuContent(state: state, cli: cli)
        } label: {
            MenuIcon(icon: state.icon)
        }
    }
}
