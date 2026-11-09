import HerdbotKit
import SwiftUI

/// Setup rerun and the daemon's lifecycle, shown on the global Settings scope. Every action goes
/// through `SetupState`, which shows only what the CLI confirmed.
struct LifecycleSection: View {
    let setup: SetupState
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Setup and daemon").font(.headline)
            Text(statusLine).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                Button("Run setup again") { openWindow(id: "setup") }
                switch setup.daemon {
                case .enabled:
                    Button("Disable daemon") { Task { await setup.setDaemon(enabled: false) } }
                case .disabled:
                    Button("Enable daemon") { Task { await setup.setDaemon(enabled: true) } }
                case .unknown, .notInstalled:
                    EmptyView()
                }
                Button("Uninstall…") { setup.prompt = .uninstall }
                    .accessibilityHint("Asks first; bots and memory are kept")
                Button("Delete bot data…", role: .destructive) {
                    setup.purgeDraft = ""
                    setup.prompt = .purge
                }
                .disabled(setup.dataRoot == nil)
                .accessibilityHint("Asks you to type the herdbot home path first")
                if setup.pending != nil {
                    ProgressView().controlSize(.small).accessibilityLabel("Working")
                }
            }
            .controlSize(.small)
            .disabled(setup.pending != nil)
            if let result = setup.lifecycleResult {
                Text(result).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            if let error = setup.lifecycleError {
                Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled)
            }
        }
        .task {
            await setup.refresh()
            await setup.refreshDaemon()
        }
        .confirmationDialog(
            "Uninstall herdbot's service?",
            isPresented: promptBinding(.uninstall),
            titleVisibility: .visible
        ) {
            Button("Uninstall", role: .destructive) { Task { await setup.uninstall(purge: false) } }
        } message: {
            Text("This removes the launchd service and the herdbot command link. Bots and memory are kept.")
        }
        .sheet(isPresented: promptBinding(.purge)) { purgeSheet }
    }

    /// True while `prompt` is this confirmation; dismissing the sheet clears it.
    private func promptBinding(_ prompt: LifecyclePrompt) -> Binding<Bool> {
        Binding(
            get: { setup.prompt == prompt },
            set: { if !$0, setup.prompt == prompt { setup.prompt = nil } }
        )
    }

    private var statusLine: String {
        switch setup.daemon {
        case .unknown: "Checking the daemon…"
        case .enabled: "The daemon is loaded in launchd."
        case .disabled: "The daemon is disabled. Its service stays installed; enable loads it again."
        case .notInstalled: "\(serviceMessage) Run setup to install it."
        }
    }

    /// The doctor's own words for the service check, so a service that runs another binary is not
    /// described as missing.
    private var serviceMessage: String {
        guard let check = setup.report?.checks.first(where: { $0.id == "service" }) else {
            return "No service is installed."
        }
        return check.message.hasSuffix(".") ? check.message : check.message + "."
    }

    private var purgeSheet: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Delete all bot data?").font(.title3.bold())
            Text("This permanently deletes every bot, memory and room under \(setup.dataRoot ?? "the herdbot home"), and removes the service. It cannot be undone.")
                .fixedSize(horizontal: false, vertical: true)
            Text("Type the path to confirm:").font(.callout)
            TextField(setup.dataRoot ?? "", text: Binding(get: { setup.purgeDraft }, set: { setup.purgeDraft = $0 }))
                .textFieldStyle(.roundedBorder)
                .font(.system(.body, design: .monospaced))
            HStack {
                Spacer()
                Button("Cancel") { setup.prompt = nil }
                    .keyboardShortcut(.cancelAction)
                Button("Delete data", role: .destructive) {
                    setup.prompt = nil
                    Task { await setup.uninstall(purge: true, confirmation: setup.purgeDraft) }
                }
                .disabled(!setup.purgeMatches)
            }
        }
        .padding(20)
        .frame(width: 460)
    }
}
