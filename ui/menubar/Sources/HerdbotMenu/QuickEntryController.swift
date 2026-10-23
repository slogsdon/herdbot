import AppKit
import HerdbotKit
import SwiftUI

/// Borderless panels refuse key status by default; this one must take it so dictation lands in its
/// field. Escape closes it.
private final class QuickEntryPanel: NSPanel {
    var onCancel: () -> Void = {}
    override var canBecomeKey: Bool { true }
    override func cancelOperation(_ sender: Any?) { onCancel() }
}

@MainActor
final class QuickEntryController {
    private let state: QuickEntryState
    private let cli: ProcessCLI?
    private let hotkeyLabel: String
    private let panel: QuickEntryPanel
    /// Set while a fade-out runs, so a quick re-show isn't ordered out by the old fade.
    private var hiding = false
    /// When focus left the panel, and how long it may stay away before the panel closes.
    private var policy = QuickEntryFocusPolicy()
    /// Live only while the panel is on screen; `hide()` calls `stopWatchingFocus`.
    private var focusObservers: [NSObjectProtocol] = []
    /// The pending grace period. Cancelled when focus comes back.
    private var dismissal: Task<Void, Never>?

    private var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    init(state: QuickEntryState, cli: ProcessCLI?, hotkeyLabel: String) {
        self.state = state
        self.cli = cli
        self.hotkeyLabel = hotkeyLabel
        panel = QuickEntryPanel(
            contentRect: NSRect(x: 0, y: 0, width: QuickEntryView.width, height: 80),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        // Don't close the moment a dictation HUD takes focus; leaving the panel is handled by the
        // grace period in `panelLostFocus`, which a returning focus cancels.
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.onCancel = { [weak self] in self?.hide() }
    }

    func toggle() {
        panel.isVisible && !hiding ? hide() : show()
    }

    func show() {
        hiding = false
        // A re-show (including one during a pending dismissal) starts from a clean focus slate.
        stopWatchingFocus()
        state.reset()
        // A fresh view each time, so the text field takes focus on every open.
        let host = NSHostingView(rootView: QuickEntryView(
            state: state, cli: cli, hotkeyLabel: hotkeyLabel,
            close: { [weak self] in self?.hide() },
            heightChanged: { [weak self] in self?.resize(toHeight: $0) }))
        panel.contentView = material(around: host)
        let height = host.fittingSize.height
        panel.setFrame(NSRect(origin: origin(forHeight: height), size: NSSize(width: QuickEntryView.width, height: height)), display: true)
        panel.alphaValue = reduceMotion ? 1 : 0
        // The app must be active for the panel to become key: dictation tools type into the active
        // app. Activation brings the app's main and key windows forward, so order the panel in and
        // make it key first, then take key back in case activation picked another window.
        panel.orderFrontRegardless()
        panel.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
        watchFocus()
        if !reduceMotion {
            NSAnimationContext.runAnimationGroup { $0.duration = 0.12; panel.animator().alphaValue = 1 }
        }
    }

    func hide() {
        guard panel.isVisible, !hiding else { return }
        stopWatchingFocus()
        if reduceMotion {
            panel.orderOut(nil)
            return
        }
        hiding = true
        Task {
            await NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.1
                panel.animator().alphaValue = 0
            }
            // A re-show during the fade clears `hiding`; leave the panel up then.
            guard hiding else { return }
            hiding = false
            panel.orderOut(nil)
        }
    }

    /// Focus leaving the panel or the app starts a grace period; a returning focus cancels it. Two
    /// notifications can report the same departure (app deactivation also resigns key), and the
    /// policy keeps the first deadline rather than pushing it back.
    private func watchFocus() {
        guard focusObservers.isEmpty else { return }
        let center = NotificationCenter.default
        focusObservers = [
            center.addObserver(forName: NSWindow.didResignKeyNotification, object: panel, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.panelLostFocus() }
            },
            center.addObserver(forName: NSApplication.didResignActiveNotification, object: NSApp, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.panelLostFocus() }
            },
            center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: panel, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.panelRegainedFocus() }
            },
        ]
    }

    /// Called on every hide, so nothing fires for a panel that is no longer on screen.
    private func stopWatchingFocus() {
        for observer in focusObservers { NotificationCenter.default.removeObserver(observer) }
        focusObservers = []
        dismissal?.cancel()
        dismissal = nil
        policy.focusRegained()
    }

    private func panelLostFocus() {
        guard panel.isVisible, !hiding else { return }
        policy.focusLost(at: .now)
        guard dismissal == nil, let deadline = policy.dismissalDeadline else { return }
        dismissal = Task { [weak self] in
            try? await Task.sleep(until: deadline, clock: .continuous)
            guard !Task.isCancelled else { return }
            self?.dismissIfFocusStayedAway()
        }
    }

    private func panelRegainedFocus() {
        guard panel.isVisible else { return }
        policy.focusRegained()
        dismissal?.cancel()
        dismissal = nil
    }

    private func dismissIfFocusStayedAway() {
        dismissal = nil
        guard panel.isVisible, policy.isDue(at: .now) else { return }
        policy.focusRegained()
        hide()
    }

    /// The system's own material, which follows Reduce Transparency and Increase Contrast by itself:
    /// Liquid Glass where it exists (macOS 26+), the HUD blur before that.
    private func material(around host: NSView) -> NSView {
        if #available(macOS 26, *) {
            let glass = NSGlassEffectView()
            glass.cornerRadius = QuickEntryView.cornerRadius
            glass.contentView = host
            return glass
        }
        let blur = NSVisualEffectView()
        blur.material = .hudWindow
        blur.blendingMode = .behindWindow
        blur.state = .active
        blur.wantsLayer = true
        blur.layer?.cornerRadius = QuickEntryView.cornerRadius
        blur.layer?.cornerCurve = .continuous
        blur.layer?.masksToBounds = true
        host.translatesAutoresizingMaskIntoConstraints = false
        blur.addSubview(host)
        NSLayoutConstraint.activate([
            host.leadingAnchor.constraint(equalTo: blur.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: blur.trailingAnchor),
            host.topAnchor.constraint(equalTo: blur.topAnchor),
            host.bottomAnchor.constraint(equalTo: blur.bottomAnchor),
        ])
        return blur
    }

    /// Upper third of the screen the pointer is on, centred, like Spotlight.
    private func origin(forHeight height: CGFloat) -> NSPoint {
        let screen = NSScreen.screens.first { NSMouseInRect(NSEvent.mouseLocation, $0.frame, false) } ?? NSScreen.main
        let area = screen?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        return NSPoint(x: area.midX - QuickEntryView.width / 2, y: area.maxY - area.height * 0.22 - height)
    }

    /// Keep the top edge still as the field grows or shrinks.
    private func resize(toHeight height: CGFloat) {
        guard panel.isVisible, abs(panel.frame.height - height) > 0.5 else { return }
        var frame = panel.frame
        frame.origin.y += frame.height - height
        frame.size.height = height
        panel.setFrame(frame, display: true)
    }
}
