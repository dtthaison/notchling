import Foundation

/// A Claude subscription Notchling can switch Claude Code to. Its login lives in the Keychain.
struct ClaudeAccount: Codable, Equatable {
    /// accountUuid and organizationUuid: one person can be in several organizations.
    var id: String
    var email: String
    var plan: String
    /// The saved login was refused; the user has to sign in to this account again.
    var needsLogin = false
}

struct UsageWindow: Equatable {
    var percent: Double
    var resetsAt: Date?
}

struct AccountUsage: Equatable {
    var session: UsageWindow?
    var week: UsageWindow?
    /// Weekly limits for one model family ("Opus", "Sonnet"), when the plan has them.
    var models: [(name: String, window: UsageWindow)] = []
    var fetched = Date()

    static func == (a: AccountUsage, b: AccountUsage) -> Bool {
        a.session == b.session && a.week == b.week && a.fetched == b.fetched
            && a.models.map(\.name) == b.models.map(\.name) && a.models.map(\.window) == b.models.map(\.window)
    }
}

/// Keeps a copy of every Claude Code login it has seen and swaps them in on request, so switching
/// accounts needs no new sign-in. Also fetches each account's 5-hour and weekly limits.
///
/// Claude Code keeps its login in the Keychain item "Claude Code-credentials" (as JSON: `claudeAiOauth`
/// next to MCP logins) and the account's profile in `oauthAccount` of ~/.claude.json. A switch replaces
/// only those two pieces. Saved logins go in our own Keychain items, never on disk.
final class AccountStore: ObservableObject {
    @Published private(set) var accounts: [ClaudeAccount] = []
    @Published private(set) var activeID: String?
    @Published private(set) var usage: [String: AccountUsage] = [:]
    private(set) var lastSync: Date?
    /// Switching or adding is under way; buttons wait for it.
    @Published private(set) var busy = false
    /// Called on the main thread after anything above changes.
    var changed: (() -> Void)?

    private let queue = DispatchQueue(label: "local.notchling.accounts", qos: .utility)
    private var syncing = false
    /// Access tokens already matched to an account, so the profile is fetched once per token.
    private var known: [String: String] = [:]

    private static let liveService = "Claude Code-credentials"
    private static let savedService = "Notchling Accounts"
    private static let configURL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude.json")
    static let clientID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
    private static let usageEvery: TimeInterval = 5 * 60

    init() {
        if let data = UserDefaults.standard.data(forKey: "accounts"),
           let saved = try? JSONDecoder().decode([ClaudeAccount].self, from: data) {
            accounts = saved
        }
    }

    var active: ClaudeAccount? { accounts.first { $0.id == activeID } }

    /// Reads the current login, saves it, and refreshes limits that are older than five minutes.
    func sync(force: Bool = false) {
        guard !syncing else { return }
        syncing = true
        queue.async {
            var state = self.snapshot()
            Self.capture(&state, known: &self.known)
            AccountLog.seedIfMissing(state.accounts.map(\.id))
            Self.fetchUsage(&state, force: force)
            DispatchQueue.main.async {
                self.syncing = false
                self.apply(state)
            }
        }
    }

    /// Points Claude Code at a saved account. The current login is saved first, since Claude Code
    /// may have refreshed it since the last sync.
    func switchTo(_ id: String, done: @escaping (Error?) -> Void) {
        busy = true
        queue.async {
            var state = self.snapshot()
            Self.capture(&state, known: &self.known)
            var failure: Error?
            do {
                try Self.activate(id)
                state.activeID = id
                if let i = state.accounts.firstIndex(where: { $0.id == id }) { state.accounts[i].needsLogin = false }
            } catch {
                failure = error
            }
            Self.fetchUsage(&state, force: false)
            DispatchQueue.main.async {
                self.busy = false
                self.apply(state)
                done(failure)
            }
        }
    }

    /// Saves a login made in Notchling's own sign-in. Claude Code keeps the account it's on.
    func add(_ oauth: [String: Any], done: @escaping (Result<ClaudeAccount, Error>) -> Void) {
        busy = true
        queue.async {
            var state = self.snapshot()
            let result = Result<ClaudeAccount, Error> {
                guard let access = oauth["accessToken"] as? String,
                      let profile = Self.get("https://api.anthropic.com/api/oauth/profile", token: access),
                      let id = Self.accountID(profile: profile) else {
                    throw AccountError("Signed in, but couldn't read the account's profile. Try again.")
                }
                var login = oauth
                let org = profile["organization"] as? [String: Any] ?? [:]
                if let type = org["organization_type"] as? String {
                    login["subscriptionType"] = type.replacingOccurrences(of: "claude_", with: "")
                }
                if let tier = org["rate_limit_tier"] as? String { login["rateLimitTier"] = tier }
                let info = Self.oauthAccount(profile: profile)
                Self.writeSaved(id, ["claudeAiOauth": login, "oauthAccount": info])
                let account = ClaudeAccount(id: id, email: info["emailAddress"] as? String ?? "Unknown",
                                            plan: Self.planName(info, oauth: login))
                if let i = state.accounts.firstIndex(where: { $0.id == id }) {
                    state.accounts[i] = account
                } else {
                    state.accounts.append(account)
                }
                // A fresh login for the account in use would be a second session; Claude Code keeps its own.
                state.usage[id] = nil
                return account
            }
            Self.fetchUsage(&state, force: false)
            DispatchQueue.main.async {
                self.busy = false
                self.apply(state)
                done(result)
            }
        }
    }

    /// Forgets a saved account. Claude Code stays signed in if it's the current one.
    func remove(_ id: String) {
        accounts.removeAll { $0.id == id }
        usage[id] = nil
        save()
        queue.async { Keychain.delete(service: Self.savedService, account: id) }
        changed?()
    }

    // MARK: State

    private struct State {
        var accounts: [ClaudeAccount]
        var activeID: String?
        var usage: [String: AccountUsage]
    }

    private func snapshot() -> State {
        DispatchQueue.main.sync { State(accounts: accounts, activeID: activeID, usage: usage) }
    }

    private func apply(_ state: State) {
        let before = (accounts, activeID, usage)
        accounts = state.accounts
        activeID = state.activeID
        usage = state.usage
        lastSync = Date()
        if let id = activeID { AccountLog.record(id) }
        save()
        if before.0 != accounts || before.1 != activeID || before.2 != usage { changed?() }
    }

    private func save() {
        if let data = try? JSONEncoder().encode(accounts) { UserDefaults.standard.set(data, forKey: "accounts") }
    }

    // MARK: Capturing the current login

    /// Works out which account Claude Code is signed in to and stores a copy of its login.
    private static func capture(_ state: inout State, known: inout [String: String]) {
        guard let live = liveCredentials(), let oauth = live["claudeAiOauth"] as? [String: Any],
              let access = oauth["accessToken"] as? String, !access.isEmpty else {
            state.activeID = nil
            return
        }
        let config = oauthAccount()
        var id = known[access]
        var profile: [String: Any]?
        if id == nil, !isExpired(oauth) {
            profile = get("https://api.anthropic.com/api/oauth/profile", token: access)
            id = profile.flatMap(accountID(profile:))
        }
        // An expired token can't be checked; Claude Code keeps ~/.claude.json in step with its login.
        if id == nil, let config { id = accountID(config: config) }
        guard let id else { return }
        known[access] = id
        state.activeID = id

        let account = profile.map(oauthAccount(profile:)) ?? (config.flatMap { accountID(config: $0) == id ? $0 : nil })
        var saved = savedLogin(id) ?? [:]
        saved["claudeAiOauth"] = oauth
        if let account { saved["oauthAccount"] = account }
        guard saved["oauthAccount"] != nil else { return }
        writeSaved(id, saved)

        let info = saved["oauthAccount"] as? [String: Any] ?? [:]
        let entry = ClaudeAccount(id: id, email: info["emailAddress"] as? String ?? "Unknown",
                                  plan: planName(info, oauth: oauth))
        if let i = state.accounts.firstIndex(where: { $0.id == id }) {
            state.accounts[i] = entry
        } else {
            state.accounts.append(entry)
        }
    }

    // MARK: Usage

    private static func fetchUsage(_ state: inout State, force: Bool) {
        for index in state.accounts.indices {
            let id = state.accounts[index].id
            if !force, let last = state.usage[id]?.fetched, Date().timeIntervalSince(last) < usageEvery { continue }
            guard let token = accessToken(for: id, active: id == state.activeID, needsLogin: &state.accounts[index].needsLogin),
                  let json = get("https://api.anthropic.com/api/oauth/usage", token: token) else { continue }
            state.usage[id] = parseUsage(json)
        }
    }

    /// A token for reading limits. The active login belongs to Claude Code: refreshing it here would
    /// rotate its refresh token out from under it, so an expired one is skipped until Claude Code renews it.
    private static func accessToken(for id: String, active: Bool, needsLogin: inout Bool) -> String? {
        if active {
            guard let oauth = liveCredentials()?["claudeAiOauth"] as? [String: Any], !isExpired(oauth) else { return nil }
            return oauth["accessToken"] as? String
        }
        guard var saved = savedLogin(id), var oauth = saved["claudeAiOauth"] as? [String: Any] else { return nil }
        if !isExpired(oauth) { return oauth["accessToken"] as? String }
        guard let refresh = oauth["refreshToken"] as? String else { return nil }
        switch refreshed(refresh, scopes: oauth["scopes"] as? [String]) {
        case .success(let fresh):
            fresh.forEach { oauth[$0.key] = $0.value }
            saved["claudeAiOauth"] = oauth
            writeSaved(id, saved)
            needsLogin = false
            return oauth["accessToken"] as? String
        case .refused:
            needsLogin = true
            return nil
        case .failed:
            return nil
        }
    }

    private enum Refresh { case success([String: Any]), refused, failed }

    private static func refreshed(_ refreshToken: String, scopes: [String]?) -> Refresh {
        var body: [String: Any] = ["grant_type": "refresh_token", "refresh_token": refreshToken, "client_id": clientID]
        if let scopes, !scopes.isEmpty { body["scope"] = scopes.joined(separator: " ") }
        guard let url = URL(string: "https://platform.claude.com/v1/oauth/token") else { return .failed }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        guard let (status, json) = send(request) else { return .failed }
        guard status == 200, let json, let access = json["access_token"] as? String else {
            return status == 400 || status == 401 ? .refused : .failed
        }
        var fresh: [String: Any] = ["accessToken": access]
        if let r = json["refresh_token"] as? String { fresh["refreshToken"] = r }
        if let s = json["expires_in"] as? Double { fresh["expiresAt"] = Int((Date().timeIntervalSince1970 + s) * 1000) }
        if let s = json["refresh_token_expires_in"] as? Double {
            fresh["refreshTokenExpiresAt"] = Int((Date().timeIntervalSince1970 + s) * 1000)
        }
        return .success(fresh)
    }

    private static func parseUsage(_ json: [String: Any]) -> AccountUsage {
        func window(_ key: String) -> UsageWindow? {
            guard let w = json[key] as? [String: Any], let p = w["utilization"] as? Double else { return nil }
            return UsageWindow(percent: p, resetsAt: (w["resets_at"] as? String).flatMap(parseDate))
        }
        var result = AccountUsage(session: window("five_hour"), week: window("seven_day"))
        for (key, name) in [("seven_day_opus", "Opus"), ("seven_day_sonnet", "Sonnet")] {
            if let w = window(key) { result.models.append((name, w)) }
        }
        return result
    }

    // MARK: Switching

    private static func activate(_ id: String) throws {
        guard let saved = savedLogin(id), let oauth = saved["claudeAiOauth"], let account = saved["oauthAccount"] else {
            throw AccountError("No saved login for this account. Sign in to it once with claude auth login.")
        }
        // Keep the MCP logins and anything else in the item; replace only the Claude login.
        var live = liveCredentials() ?? [:]
        live["claudeAiOauth"] = oauth
        let data = try JSONSerialization.data(withJSONObject: live, options: [.withoutEscapingSlashes])
        guard Keychain.write(service: liveService, account: NSUserName(), value: String(decoding: data, as: UTF8.self)) else {
            throw AccountError("Couldn't update Claude Code's login in the Keychain.")
        }
        try setConfigAccount(account)
    }

    /// Rewrites `oauthAccount` in ~/.claude.json, keeping every other key and the file's permissions.
    private static func setConfigAccount(_ account: Any) throws {
        let url = configURL
        let data = try Data(contentsOf: url)
        guard var config = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AccountError("~/.claude.json isn't a JSON object.")
        }
        config["oauthAccount"] = account
        let out = try JSONSerialization.data(withJSONObject: config, options: [.prettyPrinted, .withoutEscapingSlashes])
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        try? data.write(to: url.appendingPathExtension("notchling-backup"), options: .atomic)
        try out.write(to: url, options: .atomic)
        if let perms = attributes?[.posixPermissions] {
            try? FileManager.default.setAttributes([.posixPermissions: perms], ofItemAtPath: url.path)
            try? FileManager.default.setAttributes([.posixPermissions: perms],
                                                  ofItemAtPath: url.appendingPathExtension("notchling-backup").path)
        }
    }

    // MARK: Keychain and config

    private static func liveCredentials() -> [String: Any]? {
        Keychain.read(service: liveService, account: NSUserName()).flatMap(parseObject)
    }

    private static func savedLogin(_ id: String) -> [String: Any]? {
        Keychain.read(service: savedService, account: id).flatMap(parseObject)
    }

    private static func writeSaved(_ id: String, _ value: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.withoutEscapingSlashes]) else { return }
        Keychain.write(service: savedService, account: id, value: String(decoding: data, as: UTF8.self))
    }

    private static func oauthAccount() -> [String: Any]? {
        guard let data = try? Data(contentsOf: configURL),
              let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return config["oauthAccount"] as? [String: Any]
    }

    private static func accountID(config: [String: Any]) -> String? {
        guard let a = config["accountUuid"] as? String, let o = config["organizationUuid"] as? String else { return nil }
        return "\(a):\(o)"
    }

    private static func accountID(profile: [String: Any]) -> String? {
        guard let a = (profile["account"] as? [String: Any])?["uuid"] as? String,
              let o = (profile["organization"] as? [String: Any])?["uuid"] as? String else { return nil }
        return "\(a):\(o)"
    }

    /// The `oauthAccount` block Claude Code would write for this profile.
    private static func oauthAccount(profile: [String: Any]) -> [String: Any] {
        let a = profile["account"] as? [String: Any] ?? [:]
        let o = profile["organization"] as? [String: Any] ?? [:]
        var out: [String: Any] = [
            "accountUuid": a["uuid"] ?? "", "emailAddress": a["email"] ?? "",
            "organizationUuid": o["uuid"] ?? "", "organizationName": o["name"] ?? "",
            "organizationType": o["organization_type"] ?? NSNull(), "billingType": o["billing_type"] ?? NSNull(),
            "organizationRateLimitTier": o["rate_limit_tier"] ?? NSNull(),
            "hasExtraUsageEnabled": o["has_extra_usage_enabled"] ?? false,
        ]
        if let name = a["display_name"] { out["displayName"] = name }
        if let name = a["full_name"] { out["fullName"] = name }
        return out
    }

    /// "Max 5x", "Pro", "Team"… from the organization's type and rate limit tier.
    private static func planName(_ account: [String: Any], oauth: [String: Any]) -> String {
        let type = (account["organizationType"] as? String ?? oauth["subscriptionType"] as? String ?? "")
            .replacingOccurrences(of: "claude_", with: "")
        let tier = account["organizationRateLimitTier"] as? String ?? oauth["rateLimitTier"] as? String ?? ""
        let name = type.isEmpty ? "Claude" : type.prefix(1).uppercased() + type.dropFirst()
        if let range = tier.range(of: #"\d+x$"#, options: .regularExpression) { return "\(name) \(tier[range])" }
        return name
    }

    private static func isExpired(_ oauth: [String: Any]) -> Bool {
        guard let ms = oauth["expiresAt"] as? Double else { return false }
        return ms / 1000 < Date().timeIntervalSince1970 + 60
    }

    private static func parseObject(_ text: String) -> [String: Any]? {
        (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any]
    }

    private static let isoFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static func parseDate(_ s: String) -> Date? {
        // Python-style timestamps carry six fractional digits, which the formatter refuses.
        let trimmed = s.replacingOccurrences(of: #"(\.\d{3})\d+"#, with: "$1", options: .regularExpression)
        return isoFormatter.date(from: trimmed) ?? ISO8601DateFormatter().date(from: s)
    }

    // MARK: HTTP

    static func get(_ url: String, token: String) -> [String: Any]? {
        guard let url = URL(string: url) else { return nil }
        var request = URLRequest(url: url)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("oauth-2025-04-20", forHTTPHeaderField: "anthropic-beta")
        guard let (status, json) = send(request), status == 200 else { return nil }
        return json
    }

    /// Runs one request on the calling (background) queue.
    static func send(_ request: URLRequest) -> (Int, [String: Any]?)? {
        var request = request
        request.timeoutInterval = 15
        request.setValue("Notchling", forHTTPHeaderField: "User-Agent")
        let done = DispatchSemaphore(value: 0)
        var result: (Int, [String: Any]?)?
        URLSession.shared.dataTask(with: request) { data, response, _ in
            if let http = response as? HTTPURLResponse {
                result = (http.statusCode, data.flatMap { (try? JSONSerialization.jsonObject(with: $0)) as? [String: Any] })
            }
            done.signal()
        }.resume()
        done.wait()
        return result
    }
}

/// When each account became the one Claude Code uses. Transcripts don't say which account a message
/// went to, so cost is split by the account that was active at the message's time.
enum AccountLog {
    private static let url = HookBridge.dir.appendingPathComponent("account-log.json")
    private static let lock = NSLock()

    struct Entry: Codable {
        var at: Date
        var id: String
    }

    static func load() -> [Entry] {
        lock.lock()
        defer { lock.unlock() }
        guard let data = try? Data(contentsOf: url) else { return [] }
        return (try? JSONDecoder().decode([Entry].self, from: data)) ?? []
    }

    /// Before the log existed, each account became active when Notchling first saved it, which is when
    /// its Keychain item was created. Starts the log from those times.
    static func seedIfMissing(_ ids: [String]) {
        guard !FileManager.default.fileExists(atPath: url.path) else { return }
        let entries = ids.compactMap { id in
            Keychain.created(service: "Notchling Accounts", account: id).map { Entry(at: $0, id: id) }
        }.sorted { $0.at < $1.at }
        write(entries)
    }

    private static func write(_ entries: [Entry]) {
        lock.lock()
        defer { lock.unlock() }
        try? FileManager.default.createDirectory(at: HookBridge.dir, withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(entries) { try? data.write(to: url, options: .atomic) }
    }

    /// Adds an entry when the active account differs from the last one logged.
    static func record(_ id: String) {
        var entries = load()
        guard entries.last?.id != id else { return }
        entries.append(Entry(at: Date(), id: id))
        write(entries)
    }

    /// The account active at `date`; empty before the log starts.
    static func account(at date: Date, in entries: [Entry]) -> String {
        entries.last { $0.at <= date }?.id ?? ""
    }
}

struct AccountError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

/// Generic passwords through /usr/bin/security, the way Claude Code does it. Items it creates trust that
/// tool, so reading them causes no prompt, and an ad hoc signed app that changes on every build wouldn't keep
/// its own Keychain access anyway.
enum Keychain {
    static func read(service: String, account: String) -> String? {
        let (status, out) = run(["find-generic-password", "-s", service, "-a", account, "-w"], input: nil)
        guard status == 0 else { return nil }
        let text = out.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }

    /// Passes the value as hex on stdin through `security -i`, so it never shows in the process list.
    @discardableResult
    static func write(service: String, account: String, value: String) -> Bool {
        let hex = Data(value.utf8).map { String(format: "%02x", $0) }.joined()
        let command = "add-generic-password -U -a \"\(account)\" -s \"\(service)\" -X \"\(hex)\"\n"
        // `security -i` reads lines into a 4 KB buffer; longer ones would be cut, so they go on argv.
        if command.utf8.count < 4000 {
            return run(["-i"], input: command).0 == 0
        }
        return run(["add-generic-password", "-U", "-a", account, "-s", service, "-X", hex], input: nil).0 == 0
    }

    /// When the item was first added, from the "cdat" attribute.
    static func created(service: String, account: String) -> Date? {
        let (status, out) = run(["find-generic-password", "-s", service, "-a", account], input: nil)
        guard status == 0, let range = out.range(of: #""(\d{14})Z"#, options: .regularExpression) else { return nil }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyyMMddHHmmss"
        return f.date(from: String(out[range].dropFirst().prefix(14)))
    }

    static func delete(service: String, account: String) {
        _ = run(["delete-generic-password", "-s", service, "-a", account], input: nil)
    }

    private static func run(_ args: [String], input: String?) -> (Int32, String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = args
        let out = Pipe(), err = Pipe(), inPipe = Pipe()
        process.standardOutput = out
        process.standardError = err
        process.standardInput = inPipe
        do { try process.run() } catch { return (-1, "") }
        if let input { inPipe.fileHandleForWriting.write(Data(input.utf8)) }
        try? inPipe.fileHandleForWriting.close()
        let data = out.fileHandleForReading.readDataToEndOfFile()
        _ = err.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, String(decoding: data, as: UTF8.self))
    }
}

#if DEMO
/// Made-up accounts for Demo/accounts.swift. Only the demo renderer is built with -D DEMO.
extension AccountStore {
    func demo(accounts: [ClaudeAccount], activeID: String?, usage: [String: AccountUsage], busy: Bool = false) {
        self.accounts = accounts
        self.activeID = activeID
        self.usage = usage
        self.busy = busy
    }
}
#endif
