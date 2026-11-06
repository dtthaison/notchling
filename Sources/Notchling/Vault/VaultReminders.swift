import AppKit
import UserNotifications

/// Which expiry reminders are due. A secret with an `expires` day gets one reminder at 7 days before, one at
/// 1 day before, one on the day and one once it has passed; each fires once. Only the nearest threshold that
/// applies is considered, so a Mac that was off for days gets one reminder (with the days left), not three.
enum VaultReminder {
    enum Threshold: String, Equatable {
        case week = "7d", day = "1d", today = "0d", expired = "expired"
    }

    struct Due: Equatable {
        let name: String
        let id: String
        let day: String
        let threshold: Threshold
        let message: String
    }

    /// What was already sent is keyed by secret and expiry day, so a new date starts again.
    static func key(id: String, day: String) -> String { "\(id)|\(day)" }

    static func due(entries: [VaultEntry], sent: [String: [String]], now: Date, calendar: Calendar) -> [Due] {
        entries.compactMap { entry in
            guard let day = entry.expires, let status = VaultExpiry.status(of: day, now: now, calendar: calendar) else { return nil }
            let threshold: Threshold, when: String
            switch status {
            case .expired: threshold = .expired; when = "expired on \(day)"
            case .today: threshold = .today; when = "expires today (\(day))"
            case .inDays(let n):
                guard n <= VaultExpiry.renewSoonDays else { return nil }
                threshold = n <= 1 ? .day : .week
                when = "expires in \(n) \(n == 1 ? "day" : "days") (\(day))"
            }
            if sent[key(id: entry.id, day: day)]?.contains(threshold.rawValue) == true { return nil }
            return Due(name: entry.name, id: entry.id, day: day, threshold: threshold, message: "\(entry.name) \(when)")
        }
    }

    static func markSent(_ due: Due, in sent: [String: [String]]) -> [String: [String]] {
        var sent = sent
        sent[key(id: due.id, day: due.day), default: []].append(due.threshold.rawValue)
        return sent
    }

    /// Forgets what was sent for one secret (a new value was saved).
    static func clearing(_ id: String, in sent: [String: [String]]) -> [String: [String]] {
        sent.filter { !$0.key.hasPrefix(id + "|") }
    }

    /// Drops the memory of secrets that are gone or whose day changed.
    static func pruned(_ sent: [String: [String]], to entries: [VaultEntry]) -> [String: [String]] {
        let live = Set(entries.compactMap { e in e.expires.map { key(id: e.id, day: $0) } })
        return sent.filter { live.contains($0.key) }
    }
}

/// Where Renew… sends the user.
enum VaultRenew {
    /// The page to get a new value on: GitHub's token settings for a GitHub secret, else the secret's URL.
    static func url(for entry: VaultEntry) -> URL? {
        guard let text = entry.url?.trimmingCharacters(in: .whitespaces), !text.isEmpty else { return nil }
        if let host = VaultEntry.host(of: text), host == "github.com" || host.hasSuffix(".github.com") {
            return URL(string: "https://github.com/settings/tokens")
        }
        return URL(string: text.contains("://") ? text : "https://" + text)
    }

    /// Opens the page in the browser, if the secret has a URL.
    static func openPage(for entry: VaultEntry) {
        if let url = url(for: entry) { NSWorkspace.shared.open(url) }
    }

    /// A reminder's Renew…: the page, then the secret's form with the value empty (its own panel).
    static func start(name: String) {
        guard let entry = SecretVault.entry(name) else { return }
        openPage(for: entry)
        VaultPrompt.renew(entry)
    }
}

/// Checks the saved expiry days at launch, once a day and on wake, and posts a reminder for each that is due.
/// The system notification carries a Renew… button; when macOS refuses notifications it falls back on
/// Notchling's own banner, which has the same button.
final class VaultReminders: NSObject, UNUserNotificationCenterDelegate {
    static let shared = VaultReminders()
    static let sentKey = "vaultReminderSent"
    private static let category = "vault.expiry"
    private static let renewAction = "vault.renew"

    private var timer: Timer?
    private var wake: NSObjectProtocol?

    private static var sent: [String: [String]] {
        get { UserDefaults.standard.dictionary(forKey: sentKey) as? [String: [String]] ?? [:] }
        set { UserDefaults.standard.set(newValue, forKey: sentKey) }
    }

    /// Call on the main thread at launch.
    func start() {
        guard timer == nil else { return }
        if Bundle.main.bundleIdentifier != nil {
            let center = UNUserNotificationCenter.current()
            center.delegate = self
            let renew = UNNotificationAction(identifier: Self.renewAction, title: "Renew…", options: [.foreground])
            center.setNotificationCategories([UNNotificationCategory(identifier: Self.category, actions: [renew],
                                                                     intentIdentifiers: [], options: [])])
        }
        check()
        timer = Timer.scheduledTimer(withTimeInterval: 24 * 3600, repeats: true) { [weak self] _ in self?.check() }
        wake = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in self?.check() }
    }

    /// Forgets the reminders sent for a secret, after a new value was saved.
    static func clearSent(ids: [String]) {
        var sent = Self.sent
        for id in ids { sent = VaultReminder.clearing(id, in: sent) }
        Self.sent = sent
    }

    func check(now: Date = Date()) {
        let entries = SecretVault.all()
        var sent = VaultReminder.pruned(Self.sent, to: entries)
        for due in VaultReminder.due(entries: entries, sent: sent, now: now, calendar: .current) {
            post(due)
            sent = VaultReminder.markSent(due, in: sent)
        }
        Self.sent = sent
    }

    private func post(_ due: VaultReminder.Due) {
        let title = due.threshold == .expired ? "Secret expired" : "Secret expiring"
        guard Bundle.main.bundleIdentifier != nil else { return banner(title, due) }
        let center = UNUserNotificationCenter.current()
        center.getNotificationSettings { [weak self] settings in
            switch settings.authorizationStatus {
            case .authorized, .provisional:
                let content = UNMutableNotificationContent()
                content.title = title
                content.body = due.message
                content.categoryIdentifier = Self.category
                content.userInfo = ["secret": due.name]
                let request = UNNotificationRequest(identifier: "vault-expiry-\(due.id)-\(due.threshold.rawValue)", content: content, trigger: nil)
                center.add(request) { error in if error != nil { self?.banner(title, due) } }
            case .notDetermined:
                // The answer can take as long as the user does; the reminder can't wait for it.
                self?.banner(title, due)
                center.requestAuthorization(options: [.alert]) { _, _ in }
            default:
                self?.banner(title, due)
            }
        }
    }

    private func banner(_ title: String, _ due: VaultReminder.Due) {
        DispatchQueue.main.async {
            NoticeBanner.show(title, due.message, action: ("Renew…", { VaultRenew.start(name: due.name) }))
        }
    }

    // MARK: UNUserNotificationCenterDelegate

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        if let name = response.notification.request.content.userInfo["secret"] as? String,
           response.actionIdentifier == Self.renewAction || response.actionIdentifier == UNNotificationDefaultActionIdentifier {
            DispatchQueue.main.async { VaultRenew.start(name: name) }
        }
        completionHandler()
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list])
    }
}
