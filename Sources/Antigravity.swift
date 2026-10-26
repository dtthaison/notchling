import AppKit

/// A Google account Antigravity can be switched to. Its login lives in the Keychain.
struct AntigravityAccount: Codable, Equatable, Identifiable {
    /// The Google account's `sub`, which never changes.
    var id: String
    var email: String
}

/// Keeps a copy of every Antigravity login it has seen and swaps them in on request.
///
/// Antigravity and Antigravity IDE share one login: ~/.gemini/jetski-standalone-oauth-token, Google
/// OAuth tokens plus an id token that names the account. They read it at launch, so a switch quits
/// them, replaces the file and opens them again. Google refresh tokens don't rotate, so a saved copy
/// keeps working after Antigravity has used it.
final class AntigravityStore: ObservableObject {
    @Published private(set) var accounts: [AntigravityAccount] = []
    @Published private(set) var activeID: String?
    /// Switching or adding is under way; buttons wait for it.
    @Published private(set) var busy = false
    /// Antigravity was restarted at its sign-in screen; the next login it saves gets added.
    @Published private(set) var adding = false
    /// Called on the main thread after anything above changes.
    var changed: (() -> Void)?

    private let queue = DispatchQueue(label: "local.notchling.antigravity", qos: .utility)
    /// The file as last saved, so an unchanged login isn't written to the Keychain every minute.
    private var lastSaved: String?
    /// The account to put back if adding is cancelled.
    private var beforeAdd: String?
    private var addTimer: Timer?

    private static let service = "Notchling Antigravity"
    static let tokenURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".gemini/jetski-standalone-oauth-token")
    private static let bundleIDs = ["com.google.antigravity", "com.google.antigravity-ide"]

    init() {
        if let data = UserDefaults.standard.data(forKey: "antigravityAccounts"),
           let saved = try? JSONDecoder().decode([AntigravityAccount].self, from: data) {
            accounts = saved
        }
    }

    var active: AntigravityAccount? { accounts.first { $0.id == activeID } }

    /// Installed, or used before: otherwise the menu leaves Antigravity out.
    var isAvailable: Bool {
        !accounts.isEmpty || FileManager.default.fileExists(atPath: Self.tokenURL.path)
            || NSWorkspace.shared.urlForApplication(withBundleIdentifier: Self.bundleIDs[0]) != nil
    }

    /// Reads the current login and saves it.
    func sync() {
        queue.async {
            let state = self.capture()
            DispatchQueue.main.async { self.apply(state) }
        }
    }

    /// Quits Antigravity, puts the saved login in place and opens it again.
    func switchTo(_ id: String, done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            var state = self.capture()
            let result = Result(catching: {
                guard let login = Keychain.read(service: Self.service, account: id) else {
                    throw AccountError("Notchling has no saved login for this account. Add it again.")
                }
                let reopen = try Self.quitApps()
                try Self.writeLogin(login)
                self.lastSaved = login
                state.activeID = id
                Self.open(reopen)
            })
            DispatchQueue.main.async {
                self.busy = false
                self.apply(state)
                done(result.failure)
            }
        }
    }

    /// Saves the current login, then restarts Antigravity signed out so the user can sign in to
    /// another account. That login is saved as soon as it shows up.
    func beginAdd(done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            let state = self.capture()
            let result = Result(catching: {
                if let current = Self.readLogin() {
                    // The file goes away next; make sure the copy really is in the Keychain first.
                    guard let id = state.activeID, Keychain.read(service: Self.service, account: id) == current else {
                        throw AccountError("Couldn't save the current Antigravity login, so it was left in place.")
                    }
                }
                let reopen = try Self.quitApps()
                if FileManager.default.fileExists(atPath: Self.tokenURL.path) {
                    try FileManager.default.removeItem(at: Self.tokenURL)
                }
                self.lastSaved = nil
                Self.open(reopen.isEmpty ? [Self.bundleIDs[0]] : reopen)
            })
            DispatchQueue.main.async {
                self.busy = false
                var state = state
                if result.failure == nil {
                    self.beforeAdd = state.activeID
                    state.activeID = nil
                    self.watchForLogin()
                }
                self.apply(state)
                done(result.failure)
            }
        }
    }

    /// Stops waiting and, if nobody signed in yet, puts the previous login back.
    func cancelAdd() {
        stopWatching()
        guard let previous = beforeAdd, !FileManager.default.fileExists(atPath: Self.tokenURL.path) else { return }
        switchTo(previous) { _ in }
    }

    /// Forgets a saved login. Antigravity stays signed in if it's the one in use.
    func remove(_ id: String) {
        Keychain.delete(service: Self.service, account: id)
        accounts.removeAll { $0.id == id }
        save()
        changed?()
    }

    // MARK: Waiting for a new login

    private func watchForLogin() {
        adding = true
        addTimer?.invalidate()
        addTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            guard let self, FileManager.default.fileExists(atPath: Self.tokenURL.path) else { return }
            self.stopWatching()
            self.sync()
        }
    }

    private func stopWatching() {
        addTimer?.invalidate()
        addTimer = nil
        adding = false
        changed?()
    }

    // MARK: State

    private struct State {
        var accounts: [AntigravityAccount]
        var activeID: String?
    }

    /// Runs on the queue: saves the login in the file, if any, and says whose it is.
    private func capture() -> State {
        var state = DispatchQueue.main.sync { State(accounts: accounts, activeID: activeID) }
        guard let login = Self.readLogin() else {
            state.activeID = nil
            return state
        }
        guard let account = Self.account(in: login) else { return state }
        if login != lastSaved, Keychain.write(service: Self.service, account: account.id, value: login) {
            lastSaved = login
        }
        if let i = state.accounts.firstIndex(where: { $0.id == account.id }) {
            state.accounts[i].email = account.email
        } else {
            state.accounts.append(account)
        }
        state.activeID = account.id
        return state
    }

    private func apply(_ state: State) {
        if accounts != state.accounts {
            accounts = state.accounts
            save()
        }
        if activeID != state.activeID { activeID = state.activeID }
        changed?()
    }

    private func save() {
        if let data = try? JSONEncoder().encode(accounts) {
            UserDefaults.standard.set(data, forKey: "antigravityAccounts")
        }
    }

    // MARK: The login file

    private static func readLogin() -> String? {
        guard let data = try? Data(contentsOf: tokenURL), !data.isEmpty else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// Written whole and owner-only, like Antigravity writes it.
    private static func writeLogin(_ login: String) throws {
        let dir = tokenURL.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try Data(login.utf8).write(to: tokenURL, options: [.atomic])
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: tokenURL.path)
    }

    /// The account named by the id token's `sub` and `email`.
    static func account(in login: String) -> AntigravityAccount? {
        guard let json = try? JSONSerialization.jsonObject(with: Data(login.utf8)) as? [String: Any],
              let token = json["id_token"] as? String else { return nil }
        let parts = token.split(separator: ".")
        guard parts.count > 1 else { return nil }
        var payload = parts[1].replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        payload += String(repeating: "=", count: (4 - payload.count % 4) % 4)
        guard let data = Data(base64Encoded: payload),
              let claims = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let sub = claims["sub"] as? String else { return nil }
        return AntigravityAccount(id: sub, email: claims["email"] as? String ?? "Google Account")
    }

    // MARK: Restarting Antigravity

    /// Quits whichever Antigravity apps are open and returns them, to open again afterwards.
    private static func quitApps() throws -> [String] {
        let running = bundleIDs.flatMap { NSRunningApplication.runningApplications(withBundleIdentifier: $0) }
        guard !running.isEmpty else { return [] }
        running.forEach { $0.terminate() }
        let deadline = Date().addingTimeInterval(15)
        while running.contains(where: { !$0.isTerminated }), Date() < deadline {
            Thread.sleep(forTimeInterval: 0.2)
        }
        guard running.allSatisfy(\.isTerminated) else {
            throw AccountError("Antigravity didn't quit, so the account wasn't changed. Quit it and try again.")
        }
        return Array(Set(running.compactMap(\.bundleIdentifier))).sorted()
    }

    private static func open(_ bundleIDs: [String]) {
        for id in bundleIDs {
            guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: id) else { continue }
            DispatchQueue.main.async {
                NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
            }
        }
    }

    /// Antigravity is open, so a switch would restart it.
    static var isRunning: Bool {
        bundleIDs.contains { !NSRunningApplication.runningApplications(withBundleIdentifier: $0).isEmpty }
    }
}

private extension Result where Failure == Error {
    var failure: Error? {
        if case .failure(let error) = self { return error }
        return nil
    }
}
