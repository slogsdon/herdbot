import HerdbotKit
import UserNotifications

/// Posts a notification per newly blocked agent; clicking one focuses that agent's pane.
/// Needs an app bundle: a bare `swift run` binary has no bundle id, so it stays silent.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    private let cli: ProcessCLI?
    private let enabled = Bundle.main.bundleIdentifier != nil

    init(cli: ProcessCLI?) {
        self.cli = cli
        super.init()
        guard enabled else { return }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func post(_ event: Event) {
        guard enabled else { return }
        let content = UNMutableNotificationContent()
        content.title = "\(event.agent) is waiting"
        content.body = event.reason ?? "needs your input"
        content.sound = .default
        content.userInfo = ["agent": event.agent]
        let request = UNNotificationRequest(identifier: "blocked-\(event.agent)-\(event.at)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
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
        await focus(agent)
    }

    private func focus(_ agent: String) async {
        try? await cli?.focus(agent: agent)
    }
}
