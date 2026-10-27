import AppKit

/// Switches the Claude desktop app between accounts, separate from Claude Code but off the same
/// accounts list (matched by account UUID). The desktop app reads its login at launch, so a switch
/// quits and reopens it.
///
/// What actually decides the desktop app's account is its claude.ai **web session cookies**, not the
/// OAuth token (the token cache already holds every account at once). So, like Antigravity, Notchling
/// captures the whole cookie store while an account is active and restores it to switch — the cookies
/// are encrypted with a constant machine key, so a saved copy stays valid. `lastKnownAccountUuid` is
/// set to match. An account can only be switched to after it has been signed in to in the desktop app
/// at least once, since that is the only way its cookies come to exist.
final class ClaudeDesktopStore: ObservableObject {
    /// The account UUID the desktop app is on now.
    @Published private(set) var activeUuid: String?
    /// Account UUIDs Notchling has saved a desktop session (cookies) for.
    @Published private(set) var saved: Set<String> = []
    @Published private(set) var busy = false
    /// Desktop was reopened signed out; the next account signed in there gets captured.
    @Published private(set) var adding = false
    var changed: (() -> Void)?

    private let queue = DispatchQueue(label: "local.notchling.claudedesktop", qos: .utility)
    private var addTimer: Timer?
    private var beforeAdd: String?

    private static let bundleID = "com.anthropic.claudefordesktop"
    private static let claudeDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Claude")
    private static var configURL: URL { claudeDir.appendingPathComponent("config.json") }
    private static var cookiesURL: URL { claudeDir.appendingPathComponent("Cookies") }
    private static var backupURL: URL { claudeDir.appendingPathComponent("Cookies.notchling-backup") }
    /// Our own copies of each account's cookie store.
    private static let storeDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Notchling/desktop")

    init() {
        saved = Set(UserDefaults.standard.stringArray(forKey: "claudeDesktopAccounts") ?? [])
    }

    var isAvailable: Bool {
        !saved.isEmpty || NSWorkspace.shared.urlForApplication(withBundleIdentifier: Self.bundleID) != nil
    }

    static var isRunning: Bool {
        !NSRunningApplication.runningApplications(withBundleIdentifier: bundleID).isEmpty
    }

    /// Reads the current desktop account and saves its cookie store under that UUID.
    func sync() {
        queue.async {
            guard let uuid = Self.activeAccount() else {
                DispatchQueue.main.async { if self.activeUuid != nil { self.activeUuid = nil; self.changed?() } }
                return
            }
            let captured = Self.capture(uuid: uuid)
            DispatchQueue.main.async {
                if captured, !self.saved.contains(uuid) { self.saved.insert(uuid); self.persist() }
                if self.activeUuid != uuid { self.activeUuid = uuid }
                self.changed?()
            }
        }
    }

    /// Points the desktop app at a saved account: quit it, restore that account's cookies, reopen.
    func switchTo(_ uuid: String, done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            let result = Result(catching: {
                let saved = Self.storeDir.appendingPathComponent("\(uuid).cookies")
                guard FileManager.default.fileExists(atPath: saved.path) else {
                    throw AccountError("Notchling has no saved desktop session for this account. Sign in to it in Claude once.")
                }
                _ = try Self.quit()
                try Self.restore(from: saved, uuid: uuid)
                Self.open()
            })
            DispatchQueue.main.async {
                self.busy = false
                if result.failure == nil { self.activeUuid = uuid }
                self.changed?()
                done(result.failure)
            }
        }
    }

    /// Saves the current account, then reopens Claude Desktop signed out so you can sign in to another
    /// account. That account's session is captured as soon as it appears.
    func beginAdd(done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            let current = Self.activeAccount()
            let result = Result(catching: {
                if let current { _ = Self.capture(uuid: current) }
                _ = try Self.quit()
                try Self.signOut()
                Self.open()
            })
            DispatchQueue.main.async {
                self.busy = false
                self.beforeAdd = current
                if result.failure == nil {
                    if let current { self.saved.insert(current); self.persist() }
                    self.activeUuid = nil
                    self.watchForLogin()
                }
                self.changed?()
                done(result.failure)
            }
        }
    }

    /// Stops waiting and, if nobody signed in, puts the previous account back.
    func cancelAdd() {
        stopWatching()
        guard let previous = beforeAdd, Self.activeAccount() == nil else { return }
        switchTo(previous) { _ in }
    }

    private func watchForLogin() {
        adding = true
        addTimer?.invalidate()
        addTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            guard let self, let uuid = ClaudeDesktopStore.activeAccount(),
                  (try? Data(contentsOf: ClaudeDesktopStore.cookiesURL))?.isEmpty == false else { return }
            self.stopWatching()
            _ = uuid
            self.sync()
        }
    }

    private func stopWatching() {
        addTimer?.invalidate()
        addTimer = nil
        adding = false
        changed?()
    }

    func forget(_ uuid: String) {
        try? FileManager.default.removeItem(at: Self.storeDir.appendingPathComponent("\(uuid).cookies"))
        saved.remove(uuid)
        persist()
        changed?()
    }

    private func persist() {
        UserDefaults.standard.set(Array(saved), forKey: "claudeDesktopAccounts")
    }

    // MARK: Capture and restore

    private static func activeAccount() -> String? {
        guard let data = try? Data(contentsOf: configURL),
              let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return config["lastKnownAccountUuid"] as? String
    }

    /// Copies the live cookie store to our per-account copy, when it isn't already an exact copy.
    private static func capture(uuid: String) -> Bool {
        guard let live = try? Data(contentsOf: cookiesURL), !live.isEmpty else { return false }
        try? FileManager.default.createDirectory(at: storeDir, withIntermediateDirectories: true,
                                                 attributes: [.posixPermissions: 0o700])
        let mine = storeDir.appendingPathComponent("\(uuid).cookies")
        if let existing = try? Data(contentsOf: mine), existing == live { return true }
        guard (try? live.write(to: mine, options: [.atomic])) != nil else { return false }
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: mine.path)
        return true
    }

    /// Restores a saved cookie store and marks the account active in config.json. Backs the live
    /// cookie store up once, and clears any stale write-ahead files so the restore isn't overwritten.
    private static func restore(from saved: URL, uuid: String) throws {
        if !FileManager.default.fileExists(atPath: backupURL.path) {
            try? FileManager.default.copyItem(at: cookiesURL, to: backupURL)
        }
        let cookies = try Data(contentsOf: saved)
        try cookies.write(to: cookiesURL, options: [.atomic])
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: cookiesURL.path)
        clearWriteAhead()
        setActive(uuid)
    }

    /// Removes the live cookie session (backing it up once) so Claude Desktop opens at its sign-in
    /// screen, and clears the active-account markers.
    private static func signOut() throws {
        if FileManager.default.fileExists(atPath: cookiesURL.path), !FileManager.default.fileExists(atPath: backupURL.path) {
            try? FileManager.default.copyItem(at: cookiesURL, to: backupURL)
        }
        try? FileManager.default.removeItem(at: cookiesURL)
        clearWriteAhead()
        guard let data = try? Data(contentsOf: configURL),
              var config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        config.removeValue(forKey: "lastKnownAccountUuid")
        config.removeValue(forKey: "bootSideSessions")
        guard let out = try? JSONSerialization.data(withJSONObject: config, options: [.sortedKeys]) else { return }
        let perms = (try? FileManager.default.attributesOfItem(atPath: configURL.path)[.posixPermissions]) ?? 0o600
        try? out.write(to: configURL, options: [.atomic])
        try? FileManager.default.setAttributes([.posixPermissions: perms], ofItemAtPath: configURL.path)
    }

    /// Chromium's write-ahead files; stale ones would override a freshly restored Cookies file.
    private static func clearWriteAhead() {
        for name in ["Cookies-wal", "Cookies-journal", "Cookies-shm"] {
            try? FileManager.default.removeItem(at: claudeDir.appendingPathComponent(name))
        }
    }

    /// Points config.json at the account; keeps every other key and the file's permissions.
    private static func setActive(_ uuid: String) {
        guard let data = try? Data(contentsOf: configURL),
              var config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        config["lastKnownAccountUuid"] = uuid
        config["bootSideSessions"] = ["accountUuid": uuid, "supported": true]
        guard let out = try? JSONSerialization.data(withJSONObject: config, options: [.sortedKeys]) else { return }
        let perms = (try? FileManager.default.attributesOfItem(atPath: configURL.path)[.posixPermissions]) ?? 0o600
        try? out.write(to: configURL, options: [.atomic])
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

    private static func open() {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) else { return }
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

#if DEMO
/// Made-up Desktop sessions for Demo/accounts.swift. Only the demo renderer is built with -D DEMO.
extension ClaudeDesktopStore {
    func demo(activeUuid: String?, saved: Set<String>, busy: Bool = false) {
        self.activeUuid = activeUuid
        self.saved = saved
        self.busy = busy
    }
}
#endif
