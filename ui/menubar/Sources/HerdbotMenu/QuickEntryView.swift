import HerdbotKit
import SwiftUI

/// Recipient picker, a text field that dictation tools type or paste into, and Send.
/// Return sends; ⌘Return does too. Nothing is sent until you submit.
struct QuickEntryView: View {
    let state: QuickEntryState
    let cli: ProcessCLI?
    let close: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Picker("To", selection: Binding(get: { state.recipient }, set: { if let r = $0 { state.choose(r) } })) {
                ForEach(state.options) { option in
                    Text(option.label).tag(Optional(option.recipient))
                }
            }
            TextField("Message", text: Binding(get: { state.text }, set: { state.text = $0 }), axis: .vertical)
                .lineLimit(1...8)
                .focused($focused)
                .onSubmit(submit)
            HStack {
                status
                Spacer()
                Button("Cancel", action: close).keyboardShortcut(.cancelAction)
                Button("Send", action: submit)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(!state.canSend)
            }
        }
        .padding(16)
        .frame(width: 480)
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

    @ViewBuilder private var status: some View {
        switch state.status {
        case .queued: Text("queued").foregroundStyle(.green)
        case .sending: Text("sending…").foregroundStyle(.secondary)
        case .failed(let message): Text(message).foregroundStyle(.red).lineLimit(2)
        case .idle:
            if cli == nil {
                Text("herdbot not found").foregroundStyle(.red)
            } else if state.daemonDown {
                Text("daemon not running; messages won't deliver until it is").foregroundStyle(.orange)
            }
        }
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
