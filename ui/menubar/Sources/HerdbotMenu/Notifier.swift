import Foundation
import HerdbotKit
import Observation
import UserNotifications

/// Why notifications can't appear, shown in the menu; nil when they are allowed (or not applicable).
@MainActor @Observable
final class NotificationStatus {
    var problem: String?
}

/// Posts a notification per newly blocked agent; clicking one opens that bot's chat, or focuses the
/// agent's pane when it isn't a bot herdbot knows.
/// Needs an app bundle: a bare `swift run` binary has no bundle id, so it stays silent.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    private let cli: ProcessCLI?
    private let status: NotificationStatus
    private let enabled = Bundle.main.bundleIdentifier != nil
    private var authorized = false
    /// Opens the chat for an agent and says whether it could; set once the chat exists.
    var openChat: (String) -> Bool = { _ in false }

    init(cli: ProcessCLI?, status: NotificationStatus) {
        self.cli = cli
        self.status = status
        super.init()
        guard enabled else { return }
        UNUserNotificationCenter.current().delegate = self
        Task { await refreshStatus() }
    }

    /// Reads the system's notification setting (asking once if it was never decided) and records any problem.
    func refreshStatus() async {
        guard enabled else { return }
        let center = UNUserNotificationCenter.current()
        var current = await center.notificationSettings().authorizationStatus
        if current == .notDetermined {
            _ = try? await center.requestAuthorization(options: [.alert, .sound])
            current = await center.notificationSettings().authorizationStatus
        }
        switch current {
        case .authorized, .provisional, .ephemeral:
            authorized = true
            status.problem = nil
        case .denied:
            authorized = false
            status.problem = "Notifications are off for herdbot menu. Enable them in System Settings."
        default:
            authorized = false
        }
    }

    func post(_ event: Event) {
        guard enabled else { return }
        Task {
            await refreshStatus()
            guard authorized else { return }
            await add(event)
        }
    }

    func post(_ card: Card) {
        guard enabled else { return }
        Task {
            await refreshStatus()
            guard authorized else { return }
            let content = UNMutableNotificationContent()
            content.title = "\(card.bot) needs your approval"
            content.body = card.title
            content.sound = .default
            content.userInfo = ["agent": card.agent]
            let request = UNNotificationRequest(identifier: "card-\(card.id)", content: content, trigger: nil)
            do {
                try await UNUserNotificationCenter.current().add(request)
            } catch {
                fputs("herdbot menu: notification failed: \(error)\n", stderr)
            }
        }
    }

    private func add(_ event: Event) async {
        let content = UNMutableNotificationContent()
        content.title = "\(event.agent) is waiting"
        content.body = event.reason ?? "needs your input"
        content.sound = .default
        content.userInfo = ["agent": event.agent]
        let request = UNNotificationRequest(identifier: "blocked-\(event.agent)-\(event.at)", content: content, trigger: nil)
        do {
            try await UNUserNotificationCenter.current().add(request)
        } catch {
            fputs("herdbot menu: notification failed: \(error)\n", stderr)
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        guard let agent = response.notification.request.content.userInfo["agent"] as? String else { return }
        await MainActor.run { openOrFocus(agent) }
    }

    private func openOrFocus(_ agent: String) {
        if openChat(agent) { return }
        Task { try? await cli?.focus(agent: agent) }
    }
}
