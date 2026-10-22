import AppKit
import HerdbotKit
import SwiftUI

/// Escape closes it; a plain NSPanel refuses key status without this subclass.
private final class QuickEntryPanel: NSPanel {
    var onCancel: () -> Void = {}
    override var canBecomeKey: Bool { true }
    override func cancelOperation(_ sender: Any?) { onCancel() }
}

@MainActor
final class QuickEntryController {
    private let state: QuickEntryState
    private let cli: ProcessCLI?
    private let panel: QuickEntryPanel

    init(state: QuickEntryState, cli: ProcessCLI?) {
        self.state = state
        self.cli = cli
        panel = QuickEntryPanel(
            contentRect: NSRect(x: 0, y: 0, width: 480, height: 120),
            styleMask: [.titled, .fullSizeContentView],
            backing: .buffered,
            defer: false)
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        // Stay open if a dictation HUD takes focus for a moment: only Escape, Cancel or a send closes it.
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.onCancel = { [weak self] in self?.hide() }
    }

    func toggle() {
        panel.isVisible ? hide() : show()
    }

    func show() {
        state.reset()
        // A fresh view each time, so the text field takes focus on every open.
        let view = NSHostingView(rootView: QuickEntryView(state: state, cli: cli, close: { [weak self] in self?.hide() }))
        panel.contentView = view
        panel.setContentSize(view.fittingSize)
        panel.center()
        // The app must be active for the panel to become key: dictation tools type into the active app.
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
    }

    func hide() {
        panel.orderOut(nil)
    }
}
