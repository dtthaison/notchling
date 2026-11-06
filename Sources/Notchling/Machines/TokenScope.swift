import AppKit
import Foundation

/// Step 14: which repositories the issues token reaches and when it expires. The probe runs on this Mac and the
/// token never leaves the app: it is read from the vault, compared in memory with the other machines' tokens and
/// sent only in the Authorization header of requests to `https://api.github.com`. No result, row or message
/// holds it.
enum TokenScope {
    /// Read-only issues access on each served repo, nothing on any other private repo, no Contents access, a
    /// value no other machine uses, and an expiry that has not passed.
    ///
    /// `open` and `form` are the seam for the creation flow the Fix starts: tests pass their own.
    static func step(for machine: Machine, job: SDWorkerSettings,
                     open: @escaping @Sendable (URL) -> Void = { NSWorkspace.shared.open($0) },
                     form: @escaping @Sendable (VaultRequest) -> Void = { _ in }) -> SetupStep {
        let repos = job.repos.map(\.github)
        let display = "Open GitHub's token page for \(repos.joined(separator: ", ")) (Issues: read-only, nothing else) "
            + "and the vault form for \(job.issuesSecret). The new token is saved in the vault and never shown."
        let fix = FixPlan(display: display, needsAdmin: false) { _ in
            // A new token is the one clean fix for a shared, mis-scoped or expired one.
            await MainActor.run {
                TokenCreation.start(secret: job.issuesSecret, machine: machine.name, repos: repos, open: open, form: form)
            }
            return FixOutcome(ok: true, message: "Opened GitHub's token page and the vault form for \(job.issuesSecret). Save the new token, then check again.")
        }
        return SetupStep(id: .tokenScope, title: "Token scope", needs: [], cadence: .slow,
                         probe: .local { ctx in await check(ctx, job: job) }, fix: .run(fix))
    }

    /// The day (`YYYY-MM-DD`) in a `github-authentication-token-expiration` header such as
    /// `2027-02-01 12:00:00 +0700`: its first 10 characters when they are a real day.
    static func expiryDay(header: String) -> String? {
        VaultExpiry.parse(String(header.prefix(10)))
    }

    // MARK: the check

    private enum Outcome {
        case ok(String), fail(String), unknown(String)

        var row: (ok: Bool, detail: String) {
            switch self {
            case .ok(let d): return (true, d)
            case .fail(let d), .unknown(let d): return (false, d)
            }
        }
    }

    private static let apiHost = "https://api.github.com"
    private static let refused = "GitHub refused the token: expired or revoked"
    private static let maxInFlight = 4
    private static let namedRepos = 3

    private static func check(_ ctx: StepContext, job: SDWorkerSettings) async -> CheckResult {
        let secretName = job.issuesSecret
        guard !secretName.isEmpty, let token = ctx.secrets.value(named: secretName), !token.isEmpty else {
            return CheckResult(state: .cantTell("\(secretName.isEmpty ? "the issues token" : secretName) is not in the vault"))
        }
        let served = job.repos.map(\.github)
        guard !served.isEmpty, served.allSatisfy(isRepoName) else {
            return CheckResult(state: .cantTell("the served repos are not all owner/name"))
        }
        let servedKeys = Set(served.map { $0.lowercased() })

        // Every private repo of the served owners that is not served, from the user's own gh.
        let listing = await otherPrivateRepos(owners: owners(of: served), excluding: servedKeys, runner: ctx.runner)

        let issuesURLs = served.compactMap { apiURL("/repos/\($0)/issues", query: "per_page=1") }
        let commitsURLs = served.compactMap { apiURL("/repos/\($0)/commits", query: "per_page=1") }
        let otherURLs: [URL]
        switch listing {
        case .success(let names): otherURLs = names.compactMap { apiURL("/repos/\($0)", query: nil) }
        case .failure: otherURLs = []
        }
        let answers = await fetchAll(issuesURLs + commitsURLs + otherURLs, token: token, http: ctx.http)

        // A 401 anywhere means nothing else it said can be trusted.
        if answers.values.contains(where: { $0.status == 401 }) {
            return CheckResult(state: .fail(refused))
        }

        var outcomes: [(String, Outcome)] = []
        outcomes.append(("Reaches issues", issuesOutcome(served, issuesURLs, answers)))
        outcomes.append(("Denied elsewhere", deniedOutcome(listing, otherURLs, answers)))
        outcomes.append(("No code access", contentsOutcome(served, commitsURLs, answers)))
        outcomes.append(("Distinct token", distinctOutcome(token: token, secretName: secretName, ctx: ctx)))

        let header = issuesURLs.lazy.compactMap { answers[$0] }.filter { $0.status == 200 }
            .compactMap { $0.headers["github-authentication-token-expiration"] }.first
        let day = header.flatMap(expiryDay(header:))
        outcomes.append(("Expiry", expiryOutcome(day: day, now: ctx.now)))

        var result = CheckResult(state: summary(outcomes, served: served, day: day))
        result.rows = outcomes.map { SubRow(name: $0.0, ok: $0.1.row.ok, detail: $0.1.row.detail) }
        result.detected.tokenExpires = day
        return result
    }

    private static func summary(_ outcomes: [(String, Outcome)], served: [String], day: String?) -> CheckState {
        var fails: [String] = [], unknowns: [String] = []
        for (_, outcome) in outcomes {
            switch outcome {
            case .fail(let text): fails.append(text)
            case .unknown(let text): unknowns.append(text)
            case .ok: break
            }
        }
        if !fails.isEmpty { return .fail(fails.joined(separator: "; ")) }
        if !unknowns.isEmpty { return .cantTell(unknowns.joined(separator: "; ")) }
        let reach = served.count == 1 ? "issues on \(served[0]) only" : "issues on \(served.count) repos only"
        return .pass(reach + ", no code access" + (day.map { ", expires \($0)" } ?? ""))
    }

    private static func issuesOutcome(_ served: [String], _ urls: [URL], _ answers: [URL: HTTPAnswer]) -> Outcome {
        var failed: [String] = [], unknown = false
        for (repo, url) in zip(served, urls) {
            guard let a = answers[url] else { unknown = true; continue }
            switch a.status {
            case 200: break
            case _ where isDenial(a): failed.append(repo)
            default: unknown = true
            }
        }
        if !failed.isEmpty { return .fail("no issues access to \(failed.joined(separator: ", "))") }
        if unknown { return .unknown("couldn't read the issues of every served repo") }
        return .ok("can read issues of \(served.count) repo\(served.count == 1 ? "" : "s")")
    }

    private static func deniedOutcome(_ listing: Result<[String], ListingError>, _ urls: [URL],
                                      _ answers: [URL: HTTPAnswer]) -> Outcome {
        let names: [String]
        switch listing {
        case .failure(let e): return .unknown(e.message)
        case .success(let n): names = n
        }
        var visible: [String] = [], unknown = 0
        for (name, url) in zip(names, urls) {
            guard let a = answers[url] else { unknown += 1; continue }
            switch a.status {
            case 200: visible.append(name)
            case _ where isDenial(a): break
            default: unknown += 1
            }
        }
        if !visible.isEmpty {
            let shown = visible.prefix(namedRepos).joined(separator: ", ") + (visible.count > namedRepos ? "…" : "")
            return .fail("can see \(visible.count) other private repo\(visible.count == 1 ? "" : "s") (\(shown))")
        }
        if unknown > 0 { return .unknown("couldn't check \(unknown) of \(names.count) other private repos") }
        return .ok("denied \(names.count) other private repo\(names.count == 1 ? "" : "s")")
    }

    private static func contentsOutcome(_ served: [String], _ urls: [URL], _ answers: [URL: HTTPAnswer]) -> Outcome {
        var readable: [String] = [], unknown = false
        for (repo, url) in zip(served, urls) {
            guard let a = answers[url] else { unknown = true; continue }
            switch a.status {
            // 409 is GitHub's answer for an empty repo the token can read.
            case 200, 409: readable.append(repo)
            case _ where isDenial(a): break
            default: unknown = true
            }
        }
        if !readable.isEmpty { return .fail("can read code in \(readable.joined(separator: ", "))") }
        if unknown { return .unknown("couldn't check code access of every served repo") }
        return .ok("no Contents access")
    }

    /// Compares in memory with every other machine's token. Neither value is printed, stored or hashed.
    private static func distinctOutcome(token: String, secretName: String, ctx: StepContext) -> Outcome {
        var failures: [String] = []
        for other in ctx.all where other.name != ctx.machine.name {
            guard let theirs = other.sdWorker?.issuesSecret, !theirs.isEmpty else { continue }
            if theirs == secretName {
                failures.append("\(other.name) uses the same secret \(theirs)")
            } else if let value = ctx.secrets.value(named: theirs), value == token {
                failures.append("same token as \(other.name) (\(theirs))")
            }
        }
        return failures.isEmpty ? .ok("no other machine uses this token") : .fail(failures.joined(separator: "; "))
    }

    private static func expiryOutcome(day: String?, now: Date) -> Outcome {
        guard let day else { return .ok("GitHub reported no expiry") }
        if VaultExpiry.status(of: day, now: now) == .expired { return .fail("expired on \(day)") }
        return .ok("expires \(day)")
    }

    /// GitHub's refusal to show a repo: 403 or 404, and not the rate limit speaking.
    private static func isDenial(_ a: HTTPAnswer) -> Bool {
        (a.status == 403 || a.status == 404) && !rateLimited(a)
    }

    private static func rateLimited(_ a: HTTPAnswer) -> Bool {
        a.status == 429 || a.headers["x-ratelimit-remaining"] == "0" || a.headers["retry-after"] != nil
    }

    // MARK: repos and requests

    private struct ListingError: Error { let message: String }

    /// `owner/name`, as GitHub allows, and never a dot-only name that would climb out of the path.
    private static func isRepoName(_ text: String) -> Bool {
        guard text.range(of: #"^[A-Za-z0-9][A-Za-z0-9-]{0,38}/[A-Za-z0-9._-]{1,100}$"#, options: .regularExpression) != nil
        else { return false }
        let name = text.split(separator: "/")[1]
        return name != "." && name != ".."
    }

    private static func owners(of repos: [String]) -> [String] {
        var seen = Set<String>(), out: [String] = []
        for repo in repos {
            let owner = String(repo.split(separator: "/")[0])
            if seen.insert(owner.lowercased()).inserted { out.append(owner) }
        }
        return out
    }

    /// The private repos of each owner that are not served, sorted, from the user's own `gh`. gh holds its own
    /// login; this probe never hands it the token.
    private static func otherPrivateRepos(owners: [String], excluding served: Set<String>,
                                          runner: CommandRunning) async -> Result<[String], ListingError> {
        var names = Set<String>()
        for owner in owners {
            let argv = [MachineTools.path("gh"), "repo", "list", owner, "--visibility", "private", "--limit", "1000",
                        "--json", "nameWithOwner", "--jq", ".[].nameWithOwner"]
            let result = await runner.run(argv, cwd: nil, timeout: 60)
            guard result.code == 0 else {
                return .failure(ListingError(message: "couldn't list the private repos of \(owner) with gh"))
            }
            for line in result.out.split(whereSeparator: \.isNewline) {
                let name = line.trimmingCharacters(in: .whitespaces)
                if isRepoName(name), !served.contains(name.lowercased()) { names.insert(name) }
            }
        }
        return .success(names.sorted())
    }

    /// Only ever `https://api.github.com`: `path` comes from validated repo names.
    private static func apiURL(_ path: String, query: String?) -> URL? {
        URL(string: apiHost + path + (query.map { "?" + $0 } ?? ""))
    }

    /// All the answers, at most `maxInFlight` requests at a time.
    private static func fetchAll(_ urls: [URL], token: String, http: HTTPFetching) async -> [URL: HTTPAnswer] {
        await withTaskGroup(of: (URL, HTTPAnswer).self) { group in
            var answers: [URL: HTTPAnswer] = [:]
            var next = 0
            func startNext() {
                guard next < urls.count else { return }
                let url = urls[next]
                next += 1
                group.addTask { (url, await http.get(url, bearer: token)) }
            }
            for _ in 0..<min(maxInFlight, urls.count) { startNext() }
            while let (url, answer) = await group.next() {
                answers[url] = answer
                startNext()
            }
            return answers
        }
    }
}

/// Lets a redirect through only when it stays on `https://api.github.com`, so the Authorization header can never
/// follow one to another host. A refused redirect ends the request with the 3xx answer itself.
final class GitHubRedirectGuard: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        guard let url = request.url, URLSessionFetcher.allows(url) else { return completionHandler(nil) }
        var next = request
        if next.value(forHTTPHeaderField: "Authorization") == nil,
           let auth = task.originalRequest?.value(forHTTPHeaderField: "Authorization") {
            next.setValue(auth, forHTTPHeaderField: "Authorization")
        }
        completionHandler(next)
    }
}

/// GitHub's API over `URLSession`. The token stays in the Authorization header of requests to api.github.com;
/// any other host is refused (status 0), and so is a redirect off it. Ephemeral session: no cache, no cookies.
struct URLSessionFetcher: HTTPFetching {
    static let bodyCap = 1 << 20
    private let session: URLSession

    init() {
        let config = URLSessionConfiguration.ephemeral
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.timeoutIntervalForRequest = 20
        config.timeoutIntervalForResource = 20
        session = URLSession(configuration: config)
    }

    /// `https://api.github.com/…` and nothing else: no other host, scheme, port or user info.
    static func allows(_ url: URL) -> Bool {
        url.scheme?.lowercased() == "https" && url.host?.lowercased() == "api.github.com"
            && url.port == nil && url.user == nil && url.password == nil
    }

    func get(_ url: URL, bearer: String) async -> HTTPAnswer {
        let none = HTTPAnswer(status: 0, headers: [:], body: Data())
        guard Self.allows(url) else { return none }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 20)
        request.httpShouldHandleCookies = false
        request.setValue("Bearer \(bearer)", forHTTPHeaderField: "Authorization")
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")
        do {
            let (bytes, response) = try await session.bytes(for: request, delegate: GitHubRedirectGuard())
            guard let http = response as? HTTPURLResponse else { return none }
            var headers: [String: String] = [:]
            for (key, value) in http.allHeaderFields { headers[String(describing: key).lowercased()] = String(describing: value) }
            var body = Data()
            for try await byte in bytes {
                body.append(byte)
                if body.count >= Self.bodyCap { break }
            }
            return HTTPAnswer(status: http.statusCode, headers: headers, body: body)
        } catch {
            return none
        }
    }
}
