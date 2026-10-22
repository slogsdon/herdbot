import AppKit
import HerdbotKit
import SwiftUI

struct SetupView: View {
    let setup: SetupState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Set up herdbot").font(.title2.bold())
            if let problem = setup.problem {
                Text(problem)
                Text("Install or update herdbot, then reopen this window.").foregroundStyle(.secondary)
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(setup.checks) { check in row(check) }
                }
            }
            if let error = setup.fixError {
                Text(error).foregroundStyle(.red).textSelection(.enabled)
            }
            HStack {
                Button("Set up everything") { Task { await setup.fixAll() } }
                    .buttonStyle(.borderedProminent)
                    .disabled(!setup.needsAttention || setup.problem != nil)
                Spacer()
                Button("Done") { dismiss() }
            }
        }
        .padding(20)
        .frame(width: 460)
        .onAppear {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
            Task { await setup.refresh() }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willCloseNotification)) { note in
            guard (note.object as? NSWindow)?.title == "Set up herdbot" else { return }
            UserDefaults.standard.set(setup.report?.version, forKey: "setupDismissedVersion")
            if !NSApp.windows.contains(where: { $0.title == "herdbot" && $0.isVisible }) {
                NSApp.setActivationPolicy(.accessory)
            }
        }
    }

    private func row(_ check: DoctorCheck) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            symbol(check.status)
            VStack(alignment: .leading, spacing: 2) {
                Text(check.id).font(.headline)
                Text(check.message).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            if let fix = check.fix, check.status != .ok {
                if setup.fixing.contains(fix) {
                    ProgressView().controlSize(.small)
                } else {
                    Button(fixLabel(fix)) { Task { await setup.fix(fix) } }
                }
            }
        }
    }

    @ViewBuilder
    private func symbol(_ status: DoctorStatus) -> some View {
        switch status {
        case .ok: Image(systemName: "checkmark.circle.fill").foregroundStyle(.green).accessibilityLabel("ok")
        case .warn: Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange).accessibilityLabel("warning")
        case .fail: Image(systemName: "xmark.octagon.fill").foregroundStyle(.red).accessibilityLabel("failed")
        }
    }
}
