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
/// agent's pane when herdbot doesn't know the bot. The body names the bot and what it is stuck on.
/// Needs an app bundle: a bare `swift run` binary has no bundle id, so it stays silent.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    private let cli: ProcessCLI?
    private let status: NotificationStatus
    private let enabled = Bundle.main.bundleIdentifier != nil
    private var authorized = false
    /// Opens the chat for a bot slug and says whether it could; set once the chat exists.
    var openChat: (String) -> Bool = { _ in false }
    /// The bot an agent belongs to, resolved when a notification is posted: the run is still in the
    /// snapshot then, so a click still lands after it settles. nil for an agent herdbot doesn't know.
    var botForAgent: (String) -> String? = { _ in nil }
    /// A bot's display name, for the body.
    var botName: (String) -> String = { $0 }

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
        let bot = botForAgent(event.agent)
        let name = bot.map(botName)
        Task {
            await refreshStatus()
            guard authorized else { return }
            await add(event, bot: bot, name: name)
        }
    }

    func post(_ card: Card) {
        guard enabled else { return }
        let name = botName(card.bot)
        Task {
            await refreshStatus()
            guard authorized else { return }
            let content = UNMutableNotificationContent()
            content.title = NotificationCopy.approvalTitle
            content.body = NotificationCopy.body(bot: name, card: card)
            content.sound = .default
            content.userInfo = ["agent": card.agent, "bot": card.bot]
            let request = UNNotificationRequest(identifier: "card-\(card.id)", content: content, trigger: nil)
            do {
                try await UNUserNotificationCenter.current().add(request)
            } catch {
                fputs("herdbot menu: notification failed: \(error)\n", stderr)
            }
        }
    }

    private func add(_ event: Event, bot: String?, name: String?) async {
        let content = UNMutableNotificationContent()
        content.title = NotificationCopy.blockedTitle
        content.body = NotificationCopy.body(bot: name, event: event)
        content.sound = .default
        var info = ["agent": event.agent]
        if let bot { info["bot"] = bot }
        content.userInfo = info
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
        let info = response.notification.request.content.userInfo
        let agent = info["agent"] as? String
        let bot = info["bot"] as? String
        await MainActor.run { openOrFocus(bot: bot, agent: agent) }
    }

    /// Opens the bot's chat; only an agent whose bot can't be found still focuses its pane.
    private func openOrFocus(bot: String?, agent: String?) {
        if let slug = bot ?? agent.flatMap({ botForAgent($0) }), openChat(slug) { return }
        guard let agent else { return }
        Task { try? await cli?.focus(agent: agent) }
    }
}
