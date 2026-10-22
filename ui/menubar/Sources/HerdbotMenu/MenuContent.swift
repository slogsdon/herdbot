import AppKit
import HerdbotKit
import SwiftUI

struct MenuContent: View {
    let state: MenuState
    let notifications: NotificationStatus
    let cli: ProcessCLI?
    let hotkeyRegistered: Bool
    let openQuickEntry: () -> Void
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        if let problem = notifications.problem {
            Text(problem)
            Button("Open Notification Settings") {
                NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension")!)
            }
            Divider()
        }
        if let problem = state.problem {
            Text(problem)
        } else if let snapshot = state.snapshot {
            if snapshot.daemon == "down" {
                Text("daemon not running; messages won't deliver")
            }
            if !snapshot.blocked.isEmpty {
                Section("Waiting on you") {
                    ForEach(snapshot.blocked, id: \.self) { event in
                        Button("\(event.agent): \(event.reason ?? "waiting")") { focus(event.agent) }
                    }
                }
            }
            Section("Bots") {
                ForEach(snapshot.bots, id: \.slug) { bot in
                    Text("\(bot.up ? "●" : "○") \(bot.title.isEmpty ? bot.slug : bot.title)")
                }
            }
            if !snapshot.runs.isEmpty {
                Section("Running") {
                    ForEach(snapshot.runs, id: \.id) { run in
                        Button("\(run.bot): \(run.description) (\(run.route))") { focus(run.agent) }
                    }
                }
            }
            if !snapshot.cooldowns.isEmpty {
                Section("Cooling down") {
                    ForEach(snapshot.cooldowns, id: \.harness) { cooldown in
                        Text("\(cooldown.harness) until \(until(cooldown.until)): \(cooldown.reason)")
                    }
                }
            }
            if !snapshot.warnings.isEmpty {
                Text("\(snapshot.warnings.count) warning(s): \(snapshot.warnings[0])")
            }
        } else {
            Text("waiting for herdbot…")
        }
        Divider()
        Button("Open herdbot") {
            NSApp.activate(ignoringOtherApps: true)
            openWindow(id: "chat")
        }
        Button(hotkeyRegistered ? "Quick entry (⌥Space)" : "Quick entry (⌥Space is taken by another app)") { openQuickEntry() }
        Button("Quit") { NSApplication.shared.terminate(nil) }
    }

    private func focus(_ agent: String) {
        Task { try? await cli?.focus(agent: agent) }
    }

    private func until(_ seconds: Int) -> String {
        Date(timeIntervalSince1970: TimeInterval(seconds)).formatted(date: .omitted, time: .shortened)
    }
}
