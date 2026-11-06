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
    /// The page to get a new value on: GitHub's fine-grained token settings for a GitHub secret, else the
    /// secret's URL. Only http(s) with a host: the URL comes from an agent, and `file:`, `smb:` or an app's
    /// scheme must never be handed to the system to open.
    static func url(for entry: VaultEntry) -> URL? {
        guard let text = entry.url?.trimmingCharacters(in: .whitespaces), !text.isEmpty else { return nil }
        if let host = VaultEntry.host(of: text), host == "github.com" || host.hasSuffix(".github.com") {
            return URL(string: "https://github.com/settings/personal-access-tokens")
        }
        guard let url = URL(string: text.contains("://") ? text : "https://" + text),
              let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
              let host = url.host, !host.isEmpty else { return nil }
        return url
    }

    /// The secret's URL when it won't be opened (not http or https), to show as plain text; else nil.
    static func refusal(for entry: VaultEntry) -> String? {
        guard let text = entry.url?.trimmingCharacters(in: .whitespaces), !text.isEmpty, url(for: entry) == nil else { return nil }
        return text
    }

    /// Opens the page in the browser, if the secret has an http(s) URL.
    static func openPage(for entry: VaultEntry) {
        if let url = url(for: entry) { NSWorkspace.shared.open(url) }
    }

    /// A reminder's Renew… button: the page, then the secret's form with the value empty (its own panel).
    static func start(name: String) {
        guard let entry = SecretVault.entry(name) else { return }
        openPage(for: entry)
        VaultPrompt.open(entry, renew: true)
    }

    /// Clicking a reminder: the secret's form, without opening the browser.
    static func show(name: String) {
        guard let entry = SecretVault.entry(name) else { return }
        VaultPrompt.open(entry, renew: false)
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

    /// Reminders being posted right now, so two checks don't post one twice.
    private var inflight = Set<String>()

    /// A reminder counts as sent only once it was seen: the system notification was delivered, or the user
    /// clicked Notchling's banner (which times out and is replaced by the next one). Until then the next
    /// check shows it again.
    func check(now: Date = Date()) {
        let entries = SecretVault.all()
        let sent = VaultReminder.pruned(Self.sent, to: entries)
        Self.sent = sent
        for due in VaultReminder.due(entries: entries, sent: sent, now: now, calendar: .current) {
            let key = "\(VaultReminder.key(id: due.id, day: due.day))|\(due.threshold.rawValue)"
            guard inflight.insert(key).inserted else { continue }
            post(due) { [weak self] delivered in
                DispatchQueue.main.async {
                    self?.inflight.remove(key)
                    if delivered { self?.markSent(due) }
                }
            }
        }
    }

    private func markSent(_ due: VaultReminder.Due) {
        Self.sent = VaultReminder.markSent(due, in: Self.sent)
    }

    /// `done(true)` when the system notification was delivered; the banner paths answer false.
    private func post(_ due: VaultReminder.Due, done: @escaping (Bool) -> Void) {
        let title = due.threshold == .expired ? "Secret expired" : "Secret expiring"
        guard Bundle.main.bundleIdentifier != nil else { banner(title, due); return done(false) }
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
                center.add(request) { error in
                    if error != nil { self?.banner(title, due) }
                    done(error == nil)
                }
            case .notDetermined:
                // The answer can take as long as the user does; the reminder can't wait for it.
                self?.banner(title, due)
                center.requestAuthorization(options: [.alert]) { _, _ in }
                done(false)
            default:
                self?.banner(title, due)
                done(false)
            }
        }
    }

    private func banner(_ title: String, _ due: VaultReminder.Due) {
        DispatchQueue.main.async { [weak self] in
            NoticeBanner.show(title, due.message,
                              action: ("Renew…", { self?.markSent(due); VaultRenew.start(name: due.name) }),
                              onClick: { self?.markSent(due) })
        }
    }

    // MARK: UNUserNotificationCenterDelegate

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        if let name = response.notification.request.content.userInfo["secret"] as? String {
            switch response.actionIdentifier {
            // Only the Renew… button opens the browser; a click on the body shows the secret.
            case Self.renewAction: DispatchQueue.main.async { VaultRenew.start(name: name) }
            case UNNotificationDefaultActionIdentifier: DispatchQueue.main.async { VaultRenew.show(name: name) }
            default: break
            }
        }
        completionHandler()
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list])
    }
}
