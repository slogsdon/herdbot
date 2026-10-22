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
        // Stay open if a dictation HUD takes focus for a moment: only Escape or a send closes it.
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.onCancel = { [weak self] in self?.hide() }
    }

    func toggle() {
        panel.isVisible && !hiding ? hide() : show()
    }

    func show() {
        hiding = false
        state.reset()
        // A fresh view each time, so the text field takes focus on every open.
        let host = NSHostingView(rootView: QuickEntryView(
            state: state, cli: cli, hotkeyLabel: hotkeyLabel,
            close: { [weak self] in self?.hide() },
            heightChanged: { [weak self] in self?.resize(toHeight: $0) }))
        panel.contentView = material(around: host)
        let height = host.fittingSize.height
        panel.setFrame(NSRect(origin: origin(forHeight: height), size: NSSize(width: QuickEntryView.width, height: height)), display: true)
        // The app must be active for the panel to become key: dictation tools type into the active app.
        NSApp.activate(ignoringOtherApps: true)
        panel.alphaValue = reduceMotion ? 1 : 0
        panel.makeKeyAndOrderFront(nil)
        if !reduceMotion {
            NSAnimationContext.runAnimationGroup { $0.duration = 0.12; panel.animator().alphaValue = 1 }
        }
    }

    func hide() {
        guard panel.isVisible, !hiding else { return }
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
