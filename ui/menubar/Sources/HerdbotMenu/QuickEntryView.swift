import AppKit
import HerdbotKit
import SwiftUI

/// Spotlight-style: a large message field, a quiet row beneath it (settings on the left, the
/// recipient on the right), and no buttons. Return sends; Esc closes. Nothing is sent until you
/// press Return, so dictated text can be read first. Colours are system semantic colours and the
/// material comes from the window (see QuickEntryController), so Reduce Transparency, Increase
/// Contrast, Reduce Motion, light/dark and the accent colour all follow the system.
struct QuickEntryView: View {
    static let width: CGFloat = 680
    static let cornerRadius: CGFloat = 22

    let state: QuickEntryState
    let cli: ProcessCLI?
    let hotkeyLabel: String
    let close: () -> Void
    /// Reports the content height so the panel can follow it as the field grows.
    let heightChanged: (CGFloat) -> Void
    @FocusState private var focused: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var increaseContrast: Bool { NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField("Message, Return to send", text: Binding(get: { state.text }, set: { state.text = $0 }), axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 22))
                .lineLimit(1...6)
                .focused($focused)
                .disabled(state.status == .sending)
                .onSubmit(submit)
                .onChange(of: state.text) { state.dismissFailure() }
            HStack(spacing: 10) {
                gear
                note
                Spacer(minLength: 0)
                recipientMenu
            }
        }
        .padding(8)
        .frame(width: Self.width)
        .overlay { result }
        .clipShape(RoundedRectangle(cornerRadius: Self.cornerRadius, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: Self.cornerRadius, style: .continuous)
                .strokeBorder(Color.primary.opacity(increaseContrast ? 0.6 : 0.18), lineWidth: increaseContrast ? 1 : 0.5)
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { heightChanged($0) }
        .onAppear { focused = true }
        // onAppear runs before the panel is key, and SwiftUI drops a focus request made then. The first
        // show becomes key after activation (the notification); later shows become key at once, before
        // this view subscribes, so ask again once the panel has settled.
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { _ in focused = true }
        .task {
            try? await Task.sleep(for: .milliseconds(80))
            focused = true
        }
    }

    private var gear: some View {
        Menu {
            Text("Hotkey \(hotkeyLabel)")
            Divider()
            Button("Quit herdbot menu") { NSApplication.shared.terminate(nil) }
        } label: {
            Image(systemName: "gearshape")
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .foregroundStyle(.secondary)
    }

    /// Why a send can't work, ahead of time.
    @ViewBuilder private var note: some View {
        if cli == nil {
            Text("herdbot not found").foregroundStyle(.red)
        } else if state.daemonDown {
            Text("daemon not running; messages wait until it is").foregroundStyle(.orange)
        }
    }

    /// A Menu rather than a bare Picker: a Picker draws its own bordered pop-up button.
    private var recipientMenu: some View {
        HStack(spacing: 4) {
            Text("To")
            Menu {
                Picker("To", selection: Binding(get: { state.recipient }, set: { if let r = $0 { state.choose(r) } })) {
                    ForEach(state.options) { option in
                        Text(option.label).tag(Optional(option.recipient))
                    }
                }
                .pickerStyle(.inline)
                .labelsHidden()
            } label: {
                HStack(spacing: 3) {
                    Text(state.recipientLabel ?? "no recipients")
                    Image(systemName: "chevron.up.chevron.down").font(.system(size: 9))
                }
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .font(.system(size: 13))
        .foregroundStyle(.secondary)
    }

    /// Covers the whole panel: queued, then the panel closes; failed, until you click or type.
    @ViewBuilder private var result: some View {
        switch state.status {
        case .queued:
            overlay(symbol: "checkmark.circle.fill", tint: .green, title: "Queued to \(state.recipientLabel ?? "herdbot")", detail: nil)
        case .failed(let message):
            overlay(symbol: "exclamationmark.triangle.fill", tint: .orange, title: message, detail: "Click or start typing to edit")
                .onTapGesture { state.dismissFailure() }
        case .idle, .sending:
            EmptyView()
        }
    }

    private func overlay(symbol: String, tint: Color, title: String, detail: String?) -> some View {
        VStack(spacing: 6) {
            Image(systemName: symbol).font(.system(size: 30)).foregroundStyle(tint)
            Text(title).font(.system(size: 15, weight: .medium)).multilineTextAlignment(.center).lineLimit(3)
            if let detail { Text(detail).font(.system(size: 12)).foregroundStyle(.secondary) }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.regularMaterial)
        .transition(reduceMotion ? .identity : .opacity)
    }

    private func submit() {
        guard let cli else { return }
        Task {
            if await state.send(using: cli) {
                try? await Task.sleep(for: .milliseconds(600))
                close()
            }
        }
    }
}
