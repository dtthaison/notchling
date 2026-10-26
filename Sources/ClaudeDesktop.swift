import AppKit

/// Switches the Claude desktop app between accounts, separate from Claude Code but off the same
/// accounts list (matched by account UUID). Unlike Claude Code's seamless token swap, the desktop app
/// reads its login at launch, so a switch quits and reopens it.
///
/// The desktop app keeps its login in `~/Library/Application Support/Claude/config.json`: the encrypted
/// `oauth:tokenCache`/`oauth:tokenCacheV2`, plus `lastKnownAccountUuid` and `bootSideSessions`. The
/// blobs are encrypted with a constant machine key, so Notchling saves and restores them as-is without
/// decrypting. (Session cookies aren't touched yet; if the app can't re-establish the session from the
/// token on its own, cookie swapping would be the next step.)
final class ClaudeDesktopStore: ObservableObject {
    /// The account UUID the desktop app is on now.
    @Published private(set) var activeUuid: String?
    /// Account UUIDs Notchling has a saved desktop session for, so it can switch to them.
    @Published private(set) var saved: Set<String> = []
    @Published private(set) var busy = false
    var changed: (() -> Void)?

    private let queue = DispatchQueue(label: "local.notchling.claudedesktop", qos: .utility)
    private var lastCaptured: String?

    private static let service = "Notchling Claude Desktop"
    private static let bundleID = "com.anthropic.claudefordesktop"
    private static let configURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Claude/config.json")
    private static let backupURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Claude/config.json.notchling-backup")
    /// The login fields a switch moves; everything else in config.json is left as is.
    private static let loginKeys = ["oauth:tokenCache", "oauth:tokenCacheV2", "lastKnownAccountUuid", "bootSideSessions"]

    init() {
        saved = Set(UserDefaults.standard.stringArray(forKey: "claudeDesktopAccounts") ?? [])
    }

    /// Installed, or a session captured before.
    var isAvailable: Bool {
        !saved.isEmpty || NSWorkspace.shared.urlForApplication(withBundleIdentifier: Self.bundleID) != nil
    }

    static var isRunning: Bool {
        !NSRunningApplication.runningApplications(withBundleIdentifier: bundleID).isEmpty
    }

    /// Reads the current desktop login and saves it under its account UUID.
    func sync() {
        queue.async {
            guard let config = Self.readConfig(), let uuid = config["lastKnownAccountUuid"] as? String else {
                DispatchQueue.main.async { if self.activeUuid != nil { self.activeUuid = nil; self.changed?() } }
                return
            }
            let login = config.filter { Self.loginKeys.contains($0.key) }
            let serialized = (try? JSONSerialization.data(withJSONObject: login)).map { String(decoding: $0, as: UTF8.self) }
            if let serialized, serialized != self.lastCaptured, Keychain.write(service: Self.service, account: uuid, value: serialized) {
                self.lastCaptured = serialized
            }
            DispatchQueue.main.async {
                if !self.saved.contains(uuid) { self.saved.insert(uuid); self.persist() }
                if self.activeUuid != uuid { self.activeUuid = uuid }
                self.changed?()
            }
        }
    }

    /// Points the desktop app at a saved account: quit it, restore that account's login, reopen.
    func switchTo(_ uuid: String, done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            let result = Result(catching: {
                guard let saved = Keychain.read(service: Self.service, account: uuid),
                      let login = try? JSONSerialization.jsonObject(with: Data(saved.utf8)) as? [String: Any] else {
                    throw AccountError("Notchling has no saved desktop login for this account.")
                }
                _ = try Self.quit()
                try Self.writeLogin(login)
                Self.open(true)  // always end with the desktop app open on the switched-to account
            })
            DispatchQueue.main.async {
                self.busy = false
                if result.failure == nil { self.activeUuid = uuid }
                self.changed?()
                done(result.failure)
            }
        }
    }

    func forget(_ uuid: String) {
        Keychain.delete(service: Self.service, account: uuid)
        saved.remove(uuid)
        persist()
        changed?()
    }

    private func persist() {
        UserDefaults.standard.set(Array(saved), forKey: "claudeDesktopAccounts")
    }

    // MARK: config.json

    private static func readConfig() -> [String: Any]? {
        guard let data = try? Data(contentsOf: configURL),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return json
    }

    /// Writes the saved login keys into config.json, keeping every other key and the file's permissions.
    /// Backs the file up once before the first change.
    private static func writeLogin(_ login: [String: Any]) throws {
        guard var config = readConfig() else {
            throw AccountError("Couldn't read Claude's config to switch the desktop account.")
        }
        if !FileManager.default.fileExists(atPath: backupURL.path) {
            try? FileManager.default.copyItem(at: configURL, to: backupURL)
        }
        for key in loginKeys {
            if let value = login[key] { config[key] = value } else { config.removeValue(forKey: key) }
        }
        let data = try JSONSerialization.data(withJSONObject: config, options: [.sortedKeys])
        let perms = (try? FileManager.default.attributesOfItem(atPath: configURL.path)[.posixPermissions]) ?? 0o600
        try data.write(to: configURL, options: [.atomic])
        try? FileManager.default.setAttributes([.posixPermissions: perms], ofItemAtPath: configURL.path)
    }

    // MARK: Restarting the app

    private static func quit() throws -> Bool {
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: bundleID)
        guard !running.isEmpty else { return false }
        running.forEach { $0.terminate() }
        let deadline = Date().addingTimeInterval(15)
        while running.contains(where: { !$0.isTerminated }), Date() < deadline { Thread.sleep(forTimeInterval: 0.2) }
        guard running.allSatisfy(\.isTerminated) else {
            throw AccountError("Claude didn't quit, so the desktop account wasn't changed. Quit it and try again.")
        }
        return true
    }

    private static func open(_ shouldOpen: Bool) {
        guard shouldOpen, let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) else { return }
        DispatchQueue.main.async {
            NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
        }
    }
}

private extension Result where Failure == Error {
    var failure: Error? {
        if case .failure(let error) = self { return error }
        return nil
    }
}
