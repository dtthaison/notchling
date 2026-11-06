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
    /// `open` and `form` are the seam for the creation flow the Fix starts: tests pass their own and never open the
    /// real form. The default form is the vault's (`VaultPrompt.open`), the one the issues-token step opens too.
    static func step(for machine: Machine, job: SDWorkerSettings,
                     open: @escaping @Sendable (URL) -> Void = { NSWorkspace.shared.open($0) },
                     form: @escaping @Sendable (VaultRequest) -> Void = SecretSteps.openVaultForm) -> SetupStep {
        let repos = job.repos.map(\.github)
        let display = "Open GitHub's token page for \(repos.joined(separator: ", ")) (Issues: read and write, Metadata: read, nothing else) "
            + "and the vault form for \(job.issuesSecret). The new token is saved in the vault and never shown."
        let fix = FixPlan(display: display, needsAdmin: false) { _ in
            // A new token is the one clean fix for a shared, mis-scoped or expired one.
            await MainActor.run {
                TokenCreation.start(secret: job.issuesSecret, machine: machine.name, repos: repos, open: open, form: form)
            }
            // Only the page and the form open here. The step is not done until a check passes, so this is not ok:
            // Set up all stops at it (and the window shows the line as guidance).
            return FixOutcome(ok: false, message: "Opened GitHub's token page and the vault form for \(job.issuesSecret). Fix the token, save it, then Check again.")
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

        // Every private repo of the served owners that is not served, from the user's own gh.
        let listing = await privateListing(served: served, runner: ctx.runner)

        let issuesReqs = requests(served, suffix: "/issues", query: "per_page=1")
        let commitsReqs = requests(served, suffix: "/commits", query: "per_page=1")
        guard issuesReqs.count == served.count, commitsReqs.count == served.count else {
            return CheckResult(state: .cantTell("couldn't build the requests for the served repos"))
        }
        let otherReqs: [(name: String, url: URL)]
        switch listing {
        case .success(let l): otherReqs = requests(l.others, suffix: "", query: nil)
        case .failure: otherReqs = []
        }
        // The token's own list of private repos, before the other probes so that at most 4 requests run at once.
        let view = await tokenView(token: token, http: ctx.http)
        let answers = await fetchAll((issuesReqs + commitsReqs + otherReqs).map(\.url), token: token, http: ctx.http)

        // A 401 anywhere means nothing else it said can be trusted.
        if view.refused || answers.values.contains(where: { $0.status == 401 }) {
            return CheckResult(state: .fail(refused))
        }

        var outcomes: [(String, Outcome)] = []
        outcomes.append(("Reaches issues", issuesOutcome(issuesReqs, answers)))
        outcomes.append(("Denied elsewhere", deniedOutcome(listing, otherReqs, answers, view: view, served: served)))
        outcomes.append(("No code access", contentsOutcome(commitsReqs, answers)))
        outcomes.append(("Distinct token", distinctOutcome(token: token, secretName: secretName, ctx: ctx)))

        let header = issuesReqs.lazy.compactMap { answers[$0.url] }.filter { $0.status == 200 }
            .compactMap { $0.headers["github-authentication-token-expiration"] }.first
        let day = header.flatMap(expiryDay(header:))
        outcomes.append(("Expiry", expiryOutcome(header: header, day: day, now: ctx.now)))

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

    private static func issuesOutcome(_ reqs: [(name: String, url: URL)], _ answers: [URL: HTTPAnswer]) -> Outcome {
        var failed: [String] = [], unknown = false
        for (repo, url) in reqs {
            guard let a = answers[url] else { unknown = true; continue }
            switch a.status {
            case 200: break
            case _ where isDenial(a): failed.append(repo)
            default: unknown = true
            }
        }
        if !failed.isEmpty { return .fail("no issues access to \(failed.joined(separator: ", "))") }
        if unknown { return .unknown("couldn't read the issues of every served repo") }
        // the check reads; a write would change GitHub. sd's labels row proves the write once the labels exist.
        return .ok("can read issues of \(reqs.count) repo\(reqs.count == 1 ? "" : "s") (a write is not tried here)")
    }

    /// Two independent views of "no other private repo": a probe of each repo gh lists, and the token's own list.
    /// Either one seeing an unserved repo is a fail; a doubt in either (and no fail) is can't tell.
    private static func deniedOutcome(_ listing: Result<Listing, ListingError>, _ reqs: [(name: String, url: URL)],
                                      _ answers: [URL: HTTPAnswer], view: TokenView, served: [String]) -> Outcome {
        var fails: [String] = [], unknowns: [String] = []

        switch listing {
        case .failure(let e): unknowns.append(e.message)
        case .success(let l):
            var visible: [String] = [], unknown = 0
            for (name, url) in reqs {
                guard let a = answers[url] else { unknown += 1; continue }
                switch a.status {
                case 200: visible.append(name)
                case _ where isDenial(a): break
                default: unknown += 1
                }
            }
            if !visible.isEmpty {
                fails.append("can see \(visible.count) other private repo\(visible.count == 1 ? "" : "s") (\(shown(visible)))")
            }
            // gh must list the served repos (they are private): if it doesn't, it is looking at the wrong account or
            // cannot see private repos, and an empty "others" list would prove nothing.
            if !l.servedMissing.isEmpty {
                unknowns.append("gh doesn't list \(l.servedMissing.joined(separator: ", ")) among the private repos it can see")
            } else if l.skippedLines > 0 {
                unknowns.append("gh listed \(l.skippedLines) lines that are not repo names")
            } else if l.maybeCut {
                unknowns.append("gh may have cut its list of private repos at \(listLimit)")
            } else if unknown > 0 {
                unknowns.append("couldn't check \(unknown) of \(reqs.count) other private repos")
            }
        }

        // The token's own list: a fine-grained token lists only the repos it can reach.
        let servedKeys = Set(served.map { $0.lowercased() })
        let extra = view.names.filter { !servedKeys.contains($0.lowercased()) }.sorted()
        let seenKeys = Set(view.names.map { $0.lowercased() })
        let notShown = served.filter { !seenKeys.contains($0.lowercased()) }
        if !extra.isEmpty {
            fails.append("token reaches \(extra.count) other private repo\(extra.count == 1 ? "" : "s") (e.g. \(shown(extra))); set Only select repositories")
        } else if let doubt = view.incomplete {
            unknowns.append(doubt)
        } else if !notShown.isEmpty {
            // The served repos are private and the issues probe reaches them, so a whole list that leaves one out is
            // not the whole truth: it proves nothing about the others either.
            unknowns.append("the token's list of private repos doesn't show \(notShown.joined(separator: ", "))")
        }

        if !fails.isEmpty { return .fail(fails.joined(separator: "; ")) }
        if !unknowns.isEmpty { return .unknown(unknowns.joined(separator: "; ")) }
        return .ok("denied \(reqs.count) other private repo\(reqs.count == 1 ? "" : "s"); the token lists only the served repos")
    }

    /// At most `namedRepos` names, then `…` when there are more.
    private static func shown(_ names: [String]) -> String {
        names.prefix(namedRepos).joined(separator: ", ") + (names.count > namedRepos ? "…" : "")
    }

    // MARK: the token's own view

    /// What `GET /user/repos?visibility=private` showed.
    private struct TokenView {
        var names: [String] = []
        /// Why the list may not be the whole of it: an error, a rate limit, a cut-off body, too many pages.
        var incomplete: String?
        var refused = false
    }

    private static let viewPages = 10
    private static let viewDoubt = "couldn't read the private repos the token reaches"

    /// Follows the `Link` header's `next` page only while it stays on `https://api.github.com/user/repos`, up to
    /// `viewPages` pages. The token goes through `http` only; nothing of it is kept.
    private static func tokenView(token: String, http: HTTPFetching) async -> TokenView {
        var view = TokenView()
        // 50 a page: a page of full repo objects is about 400 KB, which leaves room under the 1 MiB body cap.
        guard var url = apiURL("/user/repos", query: "visibility=private&per_page=50") else {
            view.incomplete = viewDoubt
            return view
        }
        for page in 1...viewPages {
            let a = await http.get(url, bearer: token)
            if a.status == 401 { view.refused = true; return view }
            guard a.status == 200 else { view.incomplete = viewDoubt; return view }
            let cut = a.body.count >= URLSessionFetcher.bodyCap
            if let found = repoNames(inPage: a.body, cut: cut) {
                view.names += found.names
                // a repo whose name can't be read could be the leak: doubt, not something to drop. Later pages are
                // still read, since a leak they show is a fail.
                if found.unreadable > 0, view.incomplete == nil {
                    view.incomplete = "the token's list of private repos has \(found.unreadable) name\(found.unreadable == 1 ? "" : "s") this check can't read"
                }
            } else {
                view.incomplete = viewDoubt
                return view
            }
            if cut { view.incomplete = "the token's list of private repos was cut off"; return view }
            guard let link = a.headers["link"], link.contains(#"rel="next""#) else { return view }
            guard let next = nextLink(link), URLSessionFetcher.allows(next), next.path == "/user/repos" else {
                view.incomplete = viewDoubt
                return view
            }
            if page == viewPages { view.incomplete = "the token lists more than \(viewPages) pages of private repos"; return view }
            url = next
        }
        return view
    }

    /// The `full_name` of each repo on a page, and how many entries had none the check can read (not an object, no
    /// string `full_name`, or not `owner/name`). A whole page is parsed as JSON; a page the body cap cut cannot be,
    /// so its names are read where they stand, which is enough to show a repo but never to show there is none
    /// (the caller treats a cut page as doubtful).
    private static func repoNames(inPage body: Data, cut: Bool) -> (names: [String], unreadable: Int)? {
        if !cut {
            guard let list = try? JSONSerialization.jsonObject(with: body) as? [Any] else { return nil }
            var names: [String] = [], unreadable = 0
            for item in list {
                if let name = (item as? [String: Any])?["full_name"] as? String, isRepoName(name) {
                    names.append(name)
                } else {
                    unreadable += 1
                }
            }
            return (names, unreadable)
        }
        let text = String(decoding: body, as: UTF8.self)
        guard let regex = try? NSRegularExpression(pattern: #""full_name"\s*:\s*"([^"]+)""#) else { return nil }
        let range = NSRange(text.startIndex..., in: text)
        let found = regex.matches(in: text, range: range).compactMap { m in
            Range(m.range(at: 1), in: text).map { String(text[$0]) }
        }
        return (found.filter(isRepoName), found.filter { !isRepoName($0) }.count)
    }

    /// The URL of `rel="next"` in a `Link` header, if any.
    private static func nextLink(_ header: String) -> URL? {
        guard let regex = try? NSRegularExpression(pattern: #"<([^>]*)>\s*;\s*rel="next""#) else { return nil }
        let range = NSRange(header.startIndex..., in: header)
        guard let m = regex.firstMatch(in: header, range: range), let r = Range(m.range(at: 1), in: header) else { return nil }
        return URL(string: String(header[r]))
    }

    private static func contentsOutcome(_ reqs: [(name: String, url: URL)], _ answers: [URL: HTTPAnswer]) -> Outcome {
        var readable: [String] = [], unknown = false
        for (repo, url) in reqs {
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
        let mine = token.trimmingCharacters(in: .whitespacesAndNewlines)
        for other in ctx.all where other.name != ctx.machine.name {
            guard let theirs = other.sdWorker?.issuesSecret, !theirs.isEmpty else { continue }
            if theirs == secretName {
                failures.append("\(other.name) uses the same secret \(theirs)")
            } else if let value = ctx.secrets.value(named: theirs), !mine.isEmpty,
                      value.trimmingCharacters(in: .whitespacesAndNewlines) == mine {
                failures.append("same token as \(other.name) (\(theirs))")
            }
        }
        return failures.isEmpty ? .ok("no other machine uses this token") : .fail(failures.joined(separator: "; "))
    }

    private static func expiryOutcome(header: String?, day: String?, now: Date) -> Outcome {
        guard let day else {
            // A header that is there but unreadable is not "no expiry".
            return header == nil ? .ok("GitHub reported no expiry") : .unknown("couldn't read the expiry header")
        }
        if VaultExpiry.status(of: day, now: now) == .expired { return .fail("expired on \(day)") }
        return .ok("expires \(day)")
    }

    /// GitHub's refusal to show a repo: 403 or 404, and not the rate limit speaking.
    private static func isDenial(_ a: HTTPAnswer) -> Bool {
        (a.status == 403 || a.status == 404) && !rateLimited(a)
    }

    private static func rateLimited(_ a: HTTPAnswer) -> Bool {
        a.status == 429 || a.headers["x-ratelimit-remaining"] == "0" || a.headers["retry-after"] != nil
            // A secondary limit may carry neither header; its message says so.
            || String(decoding: a.body, as: UTF8.self).lowercased().contains("rate limit")
    }

    // MARK: repos and requests

    private struct ListingError: Error { let message: String }

    /// What `gh repo list --visibility private` said for the served owners.
    private struct Listing {
        /// Private repos that are not served, sorted.
        var others: [String]
        /// Served repos gh did not list.
        var servedMissing: [String]
        /// Lines that were not `owner/name`, which are never probed.
        var skippedLines: Int
        /// A list as long as `--limit` may have been cut.
        var maybeCut: Bool
    }

    private static let listLimit = 1000

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

    /// The private repos of each served owner, from the user's own `gh`. gh holds its own login; this probe never
    /// hands it the token.
    private static func privateListing(served: [String], runner: CommandRunning) async -> Result<Listing, ListingError> {
        let servedKeys = Set(served.map { $0.lowercased() })
        var seen = Set<String>(), others = Set<String>(), skipped = 0, cut = false
        for owner in owners(of: served) {
            let argv = [MachineTools.path("gh"), "repo", "list", owner, "--visibility", "private", "--limit", "\(listLimit)",
                        "--json", "nameWithOwner", "--jq", ".[].nameWithOwner"]
            let result = await runner.run(argv, cwd: nil, timeout: 60)
            guard result.code == 0 else {
                return .failure(ListingError(message: "couldn't list the private repos of \(owner) with gh"))
            }
            var lines = 0
            for line in result.out.split(whereSeparator: \.isNewline) {
                let name = line.trimmingCharacters(in: .whitespaces)
                if name.isEmpty { continue }
                lines += 1
                guard isRepoName(name) else { skipped += 1; continue }
                seen.insert(name.lowercased())
                if !servedKeys.contains(name.lowercased()) { others.insert(name) }
            }
            if lines >= listLimit { cut = true }
        }
        return .success(Listing(others: others.sorted(), servedMissing: served.filter { !seen.contains($0.lowercased()) },
                                skippedLines: skipped, maybeCut: cut))
    }

    /// The requests for `names`, each paired with its URL, so a row never pairs a name with another's answer.
    private static func requests(_ names: [String], suffix: String, query: String?) -> [(name: String, url: URL)] {
        names.compactMap { name in apiURL("/repos/\(name)\(suffix)", query: query).map { (name, $0) } }
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
