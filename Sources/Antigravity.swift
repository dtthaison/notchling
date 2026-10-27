import AppKit

/// A Google account Antigravity can be switched to. Its login lives in the Keychain.
struct AntigravityAccount: Codable, Equatable, Identifiable {
    /// The Google account's `sub`, which never changes.
    var id: String
    var email: String
    /// Antigravity's plan name ("Pro", "Free"…), read from its local server while it runs.
    var plan: String?
}

/// Antigravity's usage limits, as its own View Usage panel shows them: a few model groups, each with
/// a weekly and a five-hour bar. Percent is how much is used, so it reads like the Claude bars.
struct AntigravityUsage: Equatable {
    struct Group: Equatable {
        var name: String
        var bars: [(label: String, window: UsageWindow)]
        static func == (a: Group, b: Group) -> Bool {
            a.name == b.name && a.bars.map(\.label) == b.bars.map(\.label) && a.bars.map(\.window) == b.bars.map(\.window)
        }
    }
    var groups: [Group]
    var fetched = Date()

    static func == (a: AntigravityUsage, b: AntigravityUsage) -> Bool { a.groups == b.groups }
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
    /// Limits for the account in use, read from Antigravity's local server while it runs. Kept per
    /// account so closing Antigravity doesn't blank the last numbers.
    @Published private(set) var usage: [String: AntigravityUsage] = [:]
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
    /// Antigravity's own login, where it restores from at launch (go-keyring in the login Keychain).
    /// The token file is only a mirror the standalone language server reads, so a switch must set both.
    private static let liveService = "gemini"
    private static let liveAccount = "antigravity"
    private static let keyringPrefix = "go-keyring-base64:"

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

    /// Reads the current login and saves it, then reads the active account's limits if Antigravity
    /// is running.
    func sync() {
        queue.async {
            // Show the account list at once; the limits need the local server and can be slow.
            let state = self.capture()
            DispatchQueue.main.async { self.apply(state) }
            let email = state.accounts.first { $0.id == state.activeID }?.email
            guard let id = state.activeID, let snapshot = AntigravityQuota.fetch(expectedEmail: email) else { return }
            DispatchQueue.main.async {
                if let usage = snapshot.usage { self.usage[id] = usage }
                if let plan = snapshot.plan, let i = self.accounts.firstIndex(where: { $0.id == id }) {
                    self.accounts[i].plan = plan
                    self.save()
                }
                self.changed?()
            }
        }
    }

    /// Quits Antigravity, puts the saved login in place and opens it again.
    func switchTo(_ id: String, done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            var state = self.capture()
            var reopen: [String] = []
            let result = Result(catching: {
                guard let login = Keychain.read(service: Self.service, account: id) else {
                    throw AccountError("Notchling has no saved login for this account. Add it again.")
                }
                reopen = try Self.quitApps()
                try Self.writeLogin(login)
                self.lastSaved = login
                state.activeID = id
            })
            // Always end with Antigravity open on the switched-to account, even if it wasn't running.
            Self.open(reopen.isEmpty ? [Self.bundleIDs[0]] : reopen)
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
                Self.signOut()
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
        guard let previous = beforeAdd, Self.readLogin() == nil else { return }
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
        var usage: [String: AntigravityUsage]
    }

    /// Runs on the queue: saves the login in the file, if any, and says whose it is.
    private func capture() -> State {
        var state = DispatchQueue.main.sync { State(accounts: accounts, activeID: activeID, usage: usage) }
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
        if usage != state.usage { usage = state.usage }
        changed?()
    }

    private func save() {
        if let data = try? JSONEncoder().encode(accounts) {
            UserDefaults.standard.set(data, forKey: "antigravityAccounts")
        }
    }

    // MARK: The login

    /// The login Antigravity would restore from: its Keychain item, else the file mirror.
    private static func readLogin() -> String? {
        if let stored = Keychain.read(service: liveService, account: liveAccount), let json = decodeKeyring(stored) {
            return json
        }
        guard let data = try? Data(contentsOf: tokenURL), !data.isEmpty else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// Sets both places Antigravity reads: its go-keyring Keychain item and the file the language
    /// server reads. The Keychain item already exists (Antigravity made it), so an in-place update
    /// keeps its access, which is why Antigravity can still read it.
    private static func writeLogin(_ login: String) throws {
        let encoded = keyringPrefix + Data(login.utf8).base64EncodedString()
        guard Keychain.write(service: liveService, account: liveAccount, value: encoded) else {
            throw AccountError("Couldn't update Antigravity's saved login.")
        }
        let dir = tokenURL.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try Data(login.utf8).write(to: tokenURL, options: [.atomic])
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: tokenURL.path)
    }

    /// Clears Antigravity's login so it starts at the sign-in screen.
    private static func signOut() {
        Keychain.delete(service: liveService, account: liveAccount)
        try? FileManager.default.removeItem(at: tokenURL)
    }

    /// go-keyring stores the JSON as "go-keyring-base64:<base64>".
    private static func decodeKeyring(_ value: String) -> String? {
        guard value.hasPrefix(keyringPrefix),
              let data = Data(base64Encoded: String(value.dropFirst(keyringPrefix.count))) else { return nil }
        return String(data: data, encoding: .utf8)
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

/// Reads the signed-in account's limits from Antigravity's local language server, the same source as
/// its own View Usage panel. The server runs only while Antigravity is open, listens on a random
/// local port behind a CSRF token, and speaks Connect (JSON over HTTPS with a self-signed cert), so
/// this finds the process, its token and ports, then calls each until one answers.
enum AntigravityQuota {
    private static let service = "/exa.language_server_pb.LanguageServerService/"

    /// The account's plan and limits, read together from the local server.
    struct Snapshot {
        var plan: String?
        var usage: AntigravityUsage?
    }

    /// `expectedEmail` guards against reading while Antigravity is still loading another account:
    /// if the server reports a different email, it hasn't caught up, so skip this round.
    static func fetch(expectedEmail: String?) -> Snapshot? {
        guard let pid = pid(), let csrf = csrfToken(pid: pid) else { return nil }
        for port in ports(pid: pid) {
            guard let statusData = post("GetUserStatus", port: port, csrf: csrf) else { continue }
            let status = parseStatus(statusData)
            if let expectedEmail, let email = status.email, email != expectedEmail { return nil }
            let usage = post("RetrieveUserQuotaSummary", port: port, csrf: csrf).flatMap(parse)
            return Snapshot(plan: status.plan, usage: usage)
        }
        return nil
    }

    // MARK: Finding the server

    private static func pid() -> String? {
        shell("/usr/bin/pgrep", ["-f", "bin/language_server --standalone"])?
            .split(whereSeparator: \.isNewline).first.map(String.init)
    }

    /// From the server's own command line (`--csrf_token <uuid>`).
    private static func csrfToken(pid: String) -> String? {
        guard let args = shell("/bin/ps", ["-o", "command=", "-p", pid]),
              let range = args.range(of: #"--csrf_token\s+(\S+)"#, options: .regularExpression) else { return nil }
        return args[range].split(whereSeparator: \.isWhitespace).last.map(String.init)
    }

    /// The server picks a random port, so read the ones it's listening on. Highest first: its port is
    /// an ephemeral (high) one, while the low ports are other services that can hang the TLS probe.
    private static func ports(pid: String) -> [Int] {
        guard let out = shell("/usr/sbin/lsof", ["-nP", "-p", pid, "-iTCP", "-sTCP:LISTEN"]) else { return [] }
        var seen: [Int] = []
        for match in out.matches(of: #/:(\d+) \(LISTEN\)/#) {
            if let port = Int(match.1), !seen.contains(port) { seen.append(port) }
        }
        return seen.sorted(by: >)
    }

    // MARK: The call

    /// One Connect unary call; returns the body only on a 200.
    private static func post(_ method: String, port: Int, csrf: String) -> Data? {
        guard let url = URL(string: "https://127.0.0.1:\(port)\(service)\(method)") else { return nil }
        var request = URLRequest(url: url, timeoutInterval: 2)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("1", forHTTPHeaderField: "Connect-Protocol-Version")
        request.setValue(csrf, forHTTPHeaderField: "x-codeium-csrf-token")
        request.httpBody = Data("{}".utf8)

        let delegate = LocalTrust()
        let session = URLSession(configuration: .ephemeral, delegate: delegate, delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        var result: Data?
        var status = 0
        let done = DispatchSemaphore(value: 0)
        session.dataTask(with: request) { data, response, _ in
            status = (response as? HTTPURLResponse)?.statusCode ?? 0
            result = data
            done.signal()
        }.resume()
        guard done.wait(timeout: .now() + 3) == .success, status == 200 else { return nil }
        return result
    }

    /// The signed-in account's email and plan from GetUserStatus. The plan is `userTier.name` (the
    /// real Antigravity tier, e.g. "Antigravity Starter Quota"), not `planInfo.planName` (which is the
    /// underlying Codeium plan and always reads "Pro"). Shortened to just the tier word.
    private static func parseStatus(_ data: Data) -> (email: String?, plan: String?) {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let status = root["userStatus"] as? [String: Any] else { return (nil, nil) }
        let email = status["email"] as? String
        let name = (status["userTier"] as? [String: Any]).flatMap { $0["name"] as? String }
        let plan = name.map {
            $0.replacingOccurrences(of: "Antigravity ", with: "").replacingOccurrences(of: " Quota", with: "")
        }.flatMap { $0.isEmpty ? nil : $0 }
        return (email, plan)
    }

    private static func parse(_ data: Data) -> AntigravityUsage? {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let response = root["response"] as? [String: Any],
              let groups = response["groups"] as? [[String: Any]] else { return nil }
        let out = groups.compactMap { group -> AntigravityUsage.Group? in
            guard let name = group["displayName"] as? String,
                  let buckets = group["buckets"] as? [[String: Any]] else { return nil }
            let bars: [(String, UsageWindow)] = buckets.map { bucket in
                let remaining = bucket["remainingFraction"] as? Double ?? 1
                let percent = min(max((1 - remaining) * 100, 0), 100)
                let label = (bucket["window"] as? String) == "weekly" ? "Week" : "5h"
                return (label, UsageWindow(percent: percent, resetsAt: date(bucket["resetTime"] as? String)))
            }
            return AntigravityUsage.Group(name: name, bars: bars)
        }
        return out.isEmpty ? nil : AntigravityUsage(groups: out)
    }

    private static func date(_ string: String?) -> Date? {
        guard let string else { return nil }
        return ISO8601DateFormatter().date(from: string)
    }

    private static func shell(_ path: String, _ args: [String]) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = args
        let out = Pipe()
        process.standardOutput = out
        process.standardError = Pipe()
        do { try process.run() } catch { return nil }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return String(decoding: data, as: UTF8.self)
    }

    /// Trusts the server's self-signed certificate, but only on the loopback address.
    private final class LocalTrust: NSObject, URLSessionDelegate {
        func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge,
                        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
            guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
                  challenge.protectionSpace.host == "127.0.0.1", let trust = challenge.protectionSpace.serverTrust else {
                completionHandler(.performDefaultHandling, nil)
                return
            }
            completionHandler(.useCredential, URLCredential(trust: trust))
        }
    }
}
