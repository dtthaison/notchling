import AppKit

/// A ChatGPT account the Codex CLI can be switched to. Its login is a copy of `~/.codex/auth.json`
/// kept in the Keychain.
struct CodexAccount: Codable, Equatable, Identifiable {
    /// `<chatgpt_user_id>:<chatgpt_account_id>`: one person can be in a personal and a team workspace.
    var id: String
    var email: String
    /// "Free", "Plus", "Pro", "Team"…
    var plan: String?
}

/// Reading a Codex login (`auth.json`) and the usage endpoint Codex's own /status reads.
enum CodexLogin {
    struct Identity: Equatable {
        var id: String
        var email: String
        var plan: String?
    }

    /// Whose login this is, from the id token's claims. Nil for an API key login, which has no account.
    static func identity(in login: Data) -> Identity? {
        guard let tokens = tokens(in: login), let idToken = tokens["id_token"] as? String,
              let claims = jwtClaims(idToken) else { return nil }
        let auth = claims["https://api.openai.com/auth"] as? [String: Any] ?? [:]
        let workspace = auth["chatgpt_account_id"] as? String ?? tokens["account_id"] as? String
        let user = auth["chatgpt_user_id"] as? String ?? auth["user_id"] as? String ?? claims["sub"] as? String
        guard let workspace, let user else { return nil }
        let email = claims["email"] as? String
            ?? (claims["https://api.openai.com/profile"] as? [String: Any])?["email"] as? String
            ?? "ChatGPT Account"
        return Identity(id: "\(user):\(workspace)", email: email, plan: planName(auth["chatgpt_plan_type"] as? String))
    }

    /// The access token and workspace to call the usage endpoint with, while the token is still good.
    /// Notchling never renews a Codex token: refresh tokens rotate, and a Codex session still holding
    /// the old one would be signed out.
    static func access(in login: Data, now: Date = Date()) -> (token: String, workspace: String)? {
        guard let tokens = tokens(in: login), let token = tokens["access_token"] as? String,
              let workspace = tokens["account_id"] as? String else { return nil }
        if let exp = jwtClaims(token)?["exp"] as? Double, Date(timeIntervalSince1970: exp) < now.addingTimeInterval(60) {
            return nil
        }
        return (token, workspace)
    }

    private static func tokens(in login: Data) -> [String: Any]? {
        guard let root = (try? JSONSerialization.jsonObject(with: login)) as? [String: Any] else { return nil }
        return root["tokens"] as? [String: Any]
    }

    static func jwtClaims(_ token: String) -> [String: Any]? {
        let parts = token.split(separator: ".")
        guard parts.count > 1 else { return nil }
        var payload = parts[1].replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        payload += String(repeating: "=", count: (4 - payload.count % 4) % 4)
        guard let data = Data(base64Encoded: payload) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    static func planName(_ raw: String?) -> String? {
        guard let raw, !raw.isEmpty else { return nil }
        return raw.split(separator: "_").map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ")
    }

    /// `GET /backend-api/wham/usage`: the plan and each limit window, as groups of bars.
    static func usage(from data: Data, now: Date = Date()) -> (plan: String?, usage: GroupedUsage?) {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return (nil, nil) }
        var groups: [GroupedUsage.Group] = []
        func add(_ name: String, _ limit: Any?) {
            guard let limit = limit as? [String: Any] else { return }
            let bars = ["primary_window", "secondary_window"].compactMap { key -> (String, UsageWindow)? in
                guard let w = limit[key] as? [String: Any] else { return nil }
                let percent = min(max((w["used_percent"] as? Double) ?? 0, 0), 100)
                var reset = (w["reset_at"] as? Double).map { Date(timeIntervalSince1970: $0) }
                if reset == nil, let after = w["reset_after_seconds"] as? Double { reset = now.addingTimeInterval(after) }
                return (windowLabel(seconds: w["limit_window_seconds"] as? Double), UsageWindow(percent: percent, resetsAt: reset))
            }
            if !bars.isEmpty { groups.append(GroupedUsage.Group(name: name, bars: bars)) }
        }
        add("Codex", root["rate_limit"])
        add("Code Review", root["code_review_rate_limit"])
        for extra in root["additional_rate_limits"] as? [[String: Any]] ?? [] {
            let name = extra["limit_name"] as? String ?? extra["name"] as? String ?? extra["metered_feature"] as? String ?? "Other"
            add(name, extra["rate_limit"] ?? extra)
        }
        let plan = planName(root["plan_type"] as? String)
        return (plan, groups.isEmpty ? nil : GroupedUsage(groups: groups))
    }

    /// "5h" (the bar views read it as the short window), "Week", "Month", else hours or days.
    static func windowLabel(seconds: Double?) -> String {
        guard let seconds, seconds > 0 else { return "Limit" }
        let hours = Int((seconds / 3600).rounded())
        switch hours {
        case 5: return "5h"
        case 168: return "Week"
        case 672...744: return "Month"
        case ..<48: return "\(hours)h"
        default: return "\(hours / 24) days"
        }
    }
}

/// Keeps a copy of every Codex CLI login it has seen and swaps them in on request.
///
/// The Codex CLI, its background app server and the ChatGPT app's Codex all share one login,
/// `~/.codex/auth.json` (ChatGPT OAuth tokens plus an id token naming the account). A switch saves
/// the live file back to its account's copy first, since Codex renews it in place, then writes the
/// other account's copy and restarts the app server so it reads it. Adding runs `codex login` in a
/// scratch `CODEX_HOME`, so the account in use stays signed in.
final class CodexStore: ObservableObject {
    @Published private(set) var accounts: [CodexAccount] = []
    @Published private(set) var activeID: String?
    @Published private(set) var usage: [String: GroupedUsage] = [:]
    @Published private(set) var busy = false
    /// `codex login` is waiting for the browser sign-in.
    @Published private(set) var adding = false
    /// The sign-in page `codex login` printed, to open again.
    @Published private(set) var signInURL: URL?
    var changed: (() -> Void)?

    private let queue = DispatchQueue(label: "local.notchling.codex", qos: .utility)
    private var lastSaved: Data?
    private var fetched: [String: Date] = [:]
    private var login: Process?
    private var loginHome: URL?

    private static let service = "Notchling Codex"
    private static let appBundleID = "com.openai.codex"
    static var home: URL {
        if let custom = ProcessInfo.processInfo.environment["CODEX_HOME"], !custom.isEmpty {
            return URL(fileURLWithPath: (custom as NSString).expandingTildeInPath)
        }
        return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".codex")
    }
    static var authURL: URL { home.appendingPathComponent("auth.json") }

    init() {
        if let data = UserDefaults.standard.data(forKey: "codexAccounts"),
           let saved = try? JSONDecoder().decode([CodexAccount].self, from: data) {
            accounts = saved
        }
    }

    var active: CodexAccount? { accounts.first { $0.id == activeID } }

    var isAvailable: Bool {
        !accounts.isEmpty || FileManager.default.fileExists(atPath: Self.authURL.path) || Self.binary() != nil
    }

    /// Saves the live login, then reads limits for every account whose token is still good (at most
    /// every two minutes each, unless `force`).
    func sync(force: Bool = false) {
        queue.async {
            let state = self.capture()
            DispatchQueue.main.async { self.apply(state) }
            for account in state.accounts {
                if !force, let last = self.fetched[account.id], Date().timeIntervalSince(last) < 120 { continue }
                let login = account.id == state.activeID ? (try? Data(contentsOf: Self.authURL)) : Self.saved(account.id)
                guard let login, let access = CodexLogin.access(in: login),
                      let data = Self.fetchUsage(token: access.token, workspace: access.workspace) else { continue }
                self.fetched[account.id] = Date()
                let read = CodexLogin.usage(from: data)
                DispatchQueue.main.async {
                    if let u = read.usage { self.usage[account.id] = u }
                    if let plan = read.plan, let i = self.accounts.firstIndex(where: { $0.id == account.id }),
                       self.accounts[i].plan != plan {
                        self.accounts[i].plan = plan
                        self.save()
                    }
                    self.changed?()
                }
            }
        }
    }

    /// What a switch interrupts, for the confirmation: the ChatGPT app quits and reopens, the app
    /// server restarts, CLI sessions keep the old account until they're started again.
    static func runningWork() -> (app: Bool, server: Bool, sessions: Int) {
        let app = !NSRunningApplication.runningApplications(withBundleIdentifier: appBundleID).isEmpty
        var server = false, sessions = 0
        for line in shell("/bin/ps", ["-axo", "command="])?.split(whereSeparator: \.isNewline) ?? [] {
            let command = line.split(separator: " ").first.map { ($0 as NSString).lastPathComponent } ?? ""
            guard command == "codex" else { continue }
            if line.contains("app-server") { if line.contains("--managed-daemon") { server = true } } else { sessions += 1 }
        }
        return (app, server, sessions)
    }

    static func switchWarning(_ work: (app: Bool, server: Bool, sessions: Int)) -> String? {
        var parts: [String] = []
        if work.app { parts.append("The ChatGPT app quits and opens again on the other account.") }
        if work.server { parts.append("Codex's background server restarts, so its running tasks stop.") }
        if work.sessions > 0 {
            parts.append(work.sessions == 1
                ? "A Codex session is open in a terminal. It keeps the old account until you start it again."
                : "\(work.sessions) Codex sessions are open in terminals. They keep the old account until you start them again.")
        }
        return parts.isEmpty ? nil : parts.joined(separator: " ")
    }

    func switchTo(_ id: String, done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            var state = self.capture()
            var reopen = false
            let result = Result(catching: {
                guard let login = Self.saved(id) else {
                    throw AccountError("Notchling has no saved login for this account. Add it again.")
                }
                reopen = try Self.quitApp()
                try Self.writeLive(login)
                self.lastSaved = login
                state.activeID = id
                if Self.runningWork().server, let codex = Self.binary() {
                    Self.runQuietly(codex, ["app-server", "daemon", "restart"], timeout: 30)
                }
            })
            if reopen { Self.openApp() }
            self.fetched[id] = nil
            DispatchQueue.main.async {
                self.busy = false
                self.apply(state)
                done(result.failure)
                if result.failure == nil { self.sync() }
            }
        }
    }

    /// Runs `codex login` in a scratch home: the browser opens, the new login lands there and is saved
    /// as another account. The live login isn't touched.
    func beginAdd(done: @escaping (Error?) -> Void) {
        guard login == nil else { return }
        guard let codex = Self.binary() else {
            done(AccountError("Couldn't find the codex command. Install the Codex CLI first."))
            return
        }
        let scratch = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Notchling/codex-login/\(UUID().uuidString)")
        do {
            try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true,
                                                    attributes: [.posixPermissions: 0o700])
        } catch {
            done(error)
            return
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: codex)
        process.arguments = ["login", "-c", "cli_auth_credentials_store=\"file\""]
        var env = ProcessInfo.processInfo.environment
        env["CODEX_HOME"] = scratch.path
        process.environment = env
        let out = Pipe()
        process.standardOutput = out
        process.standardError = out
        process.standardInput = FileHandle.nullDevice
        out.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else { handle.readabilityHandler = nil; return }
            let text = String(decoding: data, as: UTF8.self)
            guard let match = text.firstMatch(of: #/https://auth\.openai\.com/\S+/#), let url = URL(string: String(match.0)) else { return }
            DispatchQueue.main.async { if self?.signInURL == nil { self?.signInURL = url; self?.changed?() } }
        }
        process.terminationHandler = { [weak self] p in
            out.fileHandleForReading.readabilityHandler = nil
            DispatchQueue.main.async { self?.finishAdd(p, scratch: scratch, done: done) }
        }
        do { try process.run() } catch {
            try? FileManager.default.removeItem(at: scratch)
            done(error)
            return
        }
        login = process
        loginHome = scratch
        adding = true
        signInURL = nil
        changed?()
    }

    func cancelAdd() {
        guard let login else { return }
        self.login = nil
        login.terminate()
    }

    private func finishAdd(_ process: Process, scratch: URL, done: (Error?) -> Void) {
        let cancelled = login == nil
        login = nil
        loginHome = nil
        adding = false
        signInURL = nil
        defer { try? FileManager.default.removeItem(at: scratch); changed?() }
        if cancelled { done(nil); return }
        guard process.terminationStatus == 0,
              let data = try? Data(contentsOf: scratch.appendingPathComponent("auth.json")),
              let who = CodexLogin.identity(in: data) else {
            done(AccountError("Codex didn't finish signing in."))
            return
        }
        guard Self.store(data, for: who.id) else {
            done(AccountError("Couldn't save the new Codex login in the Keychain."))
            return
        }
        let account = CodexAccount(id: who.id, email: who.email, plan: who.plan)
        if let i = accounts.firstIndex(where: { $0.id == who.id }) { accounts[i] = account } else { accounts.append(account) }
        save()
        done(nil)
        sync(force: true)
    }

    /// Forgets a saved login. Codex stays signed in if it's the one in use.
    func remove(_ id: String) {
        Keychain.delete(service: Self.service, account: id)
        accounts.removeAll { $0.id == id }
        usage[id] = nil
        save()
        changed?()
    }

    // MARK: State

    private struct State {
        var accounts: [CodexAccount]
        var activeID: String?
    }

    /// Runs on the queue: saves the live login under its account and says whose it is.
    private func capture() -> State {
        var state = DispatchQueue.main.sync { State(accounts: accounts, activeID: activeID) }
        guard let live = try? Data(contentsOf: Self.authURL), let who = CodexLogin.identity(in: live) else {
            state.activeID = nil
            return state
        }
        if live != lastSaved, Self.store(live, for: who.id) { lastSaved = live }
        if let i = state.accounts.firstIndex(where: { $0.id == who.id }) {
            state.accounts[i].email = who.email
            if state.accounts[i].plan == nil { state.accounts[i].plan = who.plan }
        } else {
            state.accounts.append(CodexAccount(id: who.id, email: who.email, plan: who.plan))
        }
        state.activeID = who.id
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
        if let data = try? JSONEncoder().encode(accounts) { UserDefaults.standard.set(data, forKey: "codexAccounts") }
    }

    // MARK: The login

    /// Base64, since `security -w` prints a value with newlines as hex.
    private static func store(_ login: Data, for id: String) -> Bool {
        if saved(id) == login { return true }
        return Keychain.write(service: service, account: id, value: login.base64EncodedString())
    }

    private static func saved(_ id: String) -> Data? {
        Keychain.read(service: service, account: id).flatMap { Data(base64Encoded: $0) }
    }

    /// Writes auth.json while holding Codex's own `auth.lock`, so it can't land mid-renewal.
    private static func writeLive(_ login: Data) throws {
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        let lockPath = home.appendingPathComponent("auth.lock").path
        let fd = open(lockPath, O_CREAT | O_RDWR, 0o600)
        if fd >= 0 { flock(fd, LOCK_EX) }
        defer { if fd >= 0 { flock(fd, LOCK_UN); close(fd) } }
        try PrivateFile.write(login, to: authURL)
    }

    private static func fetchUsage(token: String, workspace: String) -> Data? {
        guard let url = URL(string: "https://chatgpt.com/backend-api/wham/usage") else { return nil }
        var request = URLRequest(url: url, timeoutInterval: 10)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue(workspace, forHTTPHeaderField: "ChatGPT-Account-Id")
        request.setValue("Notchling", forHTTPHeaderField: "User-Agent")
        var result: Data?
        let done = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: request) { data, response, _ in
            if (response as? HTTPURLResponse)?.statusCode == 200 { result = data }
            done.signal()
        }.resume()
        _ = done.wait(timeout: .now() + 12)
        return result
    }

    // MARK: Processes

    /// The codex command: the usual install places, else the login shell's PATH (looked up once,
    /// since the menu asks every time it opens).
    static func binary() -> String? { lookedUpBinary }

    private static let lookedUpBinary: String? = findBinary()

    private static func findBinary() -> String? {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        for path in ["\(home)/.local/bin/codex", "/opt/homebrew/bin/codex", "/usr/local/bin/codex", "\(home)/.npm-global/bin/codex"]
        where FileManager.default.isExecutableFile(atPath: path) {
            return path
        }
        let found = shell("/bin/zsh", ["-lc", "command -v codex"], timeout: 5)?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let found, found.hasPrefix("/"), FileManager.default.isExecutableFile(atPath: found) else { return nil }
        return found
    }

    private static func quitApp() throws -> Bool {
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: appBundleID)
        guard !running.isEmpty else { return false }
        running.forEach { $0.terminate() }
        let deadline = Date().addingTimeInterval(15)
        while running.contains(where: { !$0.isTerminated }), Date() < deadline { Thread.sleep(forTimeInterval: 0.2) }
        guard running.allSatisfy(\.isTerminated) else {
            throw AccountError("The ChatGPT app didn't quit, so the Codex account wasn't changed. Quit it and try again.")
        }
        return true
    }

    private static func openApp() {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: appBundleID) else { return }
        DispatchQueue.main.async { NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration()) }
    }

    /// For a command that starts a daemon: its output goes nowhere, since a daemon keeping a pipe open
    /// would make reading it wait forever.
    private static func runQuietly(_ path: String, _ args: [String], timeout: TimeInterval) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = args
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        process.standardInput = FileHandle.nullDevice
        do { try process.run() } catch { return }
        let deadline = Date().addingTimeInterval(timeout)
        while process.isRunning, Date() < deadline { Thread.sleep(forTimeInterval: 0.2) }
        if process.isRunning { process.terminate() }
    }

    @discardableResult
    private static func shell(_ path: String, _ args: [String], timeout: TimeInterval = 10) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = args
        let out = Pipe()
        process.standardOutput = out
        process.standardError = Pipe()
        process.standardInput = FileHandle.nullDevice
        do { try process.run() } catch { return nil }
        let timer = DispatchWorkItem { if process.isRunning { process.terminate() } }
        DispatchQueue.global().asyncAfter(deadline: .now() + timeout, execute: timer)
        let data = out.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        timer.cancel()
        return String(decoding: data, as: UTF8.self)
    }
}

private extension Result where Failure == Error {
    var failure: Error? {
        if case .failure(let error) = self { return error }
        return nil
    }
}
