import XCTest
@testable import Notchling

/// Counts requests in flight, answers every one 404 after a short wait.
private final class SlowHTTP: HTTPFetching, @unchecked Sendable {
    private let lock = NSLock()
    private var inFlight = 0
    private var peak = 0
    func get(_ url: URL, bearer: String) async -> HTTPAnswer {
        lock.withLock { inFlight += 1; peak = max(peak, inFlight) }
        try? await Task.sleep(nanoseconds: 5_000_000)
        lock.withLock { inFlight -= 1 }
        return HTTPAnswer(status: 404, headers: [:], body: Data())
    }
    var maxInFlight: Int { lock.withLock { peak } }
}

final class MachinesTokenScopeTests: XCTestCase {
    private let repo = TestMachines.repo
    private let secret = "github_pat_SECRET123"
    private let expiryHeader = "github-authentication-token-expiration"

    private var m1: Machine { TestMachines.mac("m1") }
    private var vps: Machine { TestMachines.linux("vps-sing-1") }
    private var job: SDWorkerSettings { m1.sdWorker! }

    private func answer(_ status: Int, _ headers: [String: String] = [:]) -> HTTPAnswer {
        HTTPAnswer(status: status, headers: headers, body: Data())
    }

    private func others(_ n: Int = 30) -> [String] { (1...n).map { "dtthaison/private-\($0)" } }

    /// gh lists the served repo too, as the real command would.
    private func runner(listing: [String]? = nil) -> ScriptedRunner {
        let r = ScriptedRunner()
        let names = listing ?? ([repo] + others())
        r.on({ $0.contains("repo") && $0.contains("list") },
             result: CommandResult(code: 0, out: names.joined(separator: "\n") + "\n", err: ""))
        return r
    }

    private func http(issues: Int = 200, commits: Int = 403, other: Int = 404, header: String? = "2027-02-01 12:00:00 +0700") -> FakeHTTP {
        var a: [String: HTTPAnswer] = [
            "/repos/\(repo)/issues?per_page=1": answer(issues, header.map { [expiryHeader: $0] } ?? [:]),
            "/repos/\(repo)/commits?per_page=1": answer(commits),
        ]
        for name in others() { a["/repos/\(name)"] = answer(other) }
        return FakeHTTP(answers: a)
    }

    private func ctx(http: HTTPFetching, runner: CommandRunning, values: [String: String]? = nil,
                     all: [Machine]? = nil, machine: Machine? = nil) -> StepContext {
        let values = values ?? ["m1-github-issues": secret, "vps-sing-1-github-issues": "github_pat_OTHER"]
        return .test(machine: machine ?? m1, all: all ?? [m1, vps], runner: runner,
                     secrets: FakeSecrets(values: values), http: http)
    }

    private func check(_ c: StepContext) async -> CheckResult {
        let step = TokenScope.step(for: c.machine, job: c.machine.sdWorker!)
        guard case let .local(probe) = step.probe else {
            XCTFail("token scope is not a local probe")
            return CheckResult(state: .cantTell(""))
        }
        return await probe(c)
    }

    func testAGoodTokenPasses() async {
        let result = await check(ctx(http: http(), runner: runner()))
        guard case .pass = result.state else { return XCTFail("expected pass, got \(result.state)") }
        XCTAssertEqual(result.rows.count, 5)
        XCTAssertTrue(result.rows.allSatisfy(\.ok), "\(result.rows)")
        XCTAssertEqual(result.detected.tokenExpires, "2027-02-01")
    }

    func testTheProbeIsLocalAndHasAFix() {
        let step = TokenScope.step(for: m1, job: job)
        XCTAssertEqual(step.id, .tokenScope)
        if case .local = step.probe {} else { XCTFail("not local") }
        if case .run = step.fix {} else { XCTFail("no fix") }
    }

    func testATokenThatSeesAllPrivateReposFails() async {
        let result = await check(ctx(http: http(other: 200), runner: runner()))
        guard case let .fail(text) = result.state else { return XCTFail("expected fail, got \(result.state)") }
        XCTAssertTrue(text.hasPrefix("can see 30 other private repos"), text)
        XCTAssertTrue(text.hasSuffix("(dtthaison/private-1, dtthaison/private-10, dtthaison/private-11…)"), text)
        XCTAssertEqual(result.rows.filter { !$0.ok }.count, 1)
    }

    func testAFewVisibleReposAreAllNamed() async {
        var a: [String: HTTPAnswer] = [
            "/repos/\(repo)/issues?per_page=1": answer(200), "/repos/\(repo)/commits?per_page=1": answer(404),
        ]
        a["/repos/dtthaison/private-2"] = answer(200)
        let result = await check(ctx(http: FakeHTTP(answers: a), runner: runner()))
        XCTAssertEqual(result.state, .fail("can see 1 other private repo (dtthaison/private-2)"))
    }

    func testTheSameTokenOnTwoMachinesFails() async {
        let values = ["m1-github-issues": secret, "vps-sing-1-github-issues": secret]
        let result = await check(ctx(http: http(), runner: runner(), values: values))
        XCTAssertEqual(result.state, .fail("same token as vps-sing-1 (vps-sing-1-github-issues)"))
        XCTAssertFalse(result.rows.first { $0.name == "Distinct token" }!.ok)
        XCTAssertEqual(result.rows.filter { !$0.ok }.count, 1)
    }

    func testTwoMachinesNamingOneSecretFails() async {
        var other = vps
        other.sdWorker?.issuesSecret = "m1-github-issues"
        let result = await check(ctx(http: http(), runner: runner(), all: [m1, other]))
        XCTAssertEqual(result.state, .fail("vps-sing-1 uses the same secret m1-github-issues"))
    }

    func testContentsAccessFails() async {
        for code in [200, 409] {
            let result = await check(ctx(http: http(commits: code), runner: runner()))
            XCTAssertEqual(result.state, .fail("can read code in \(repo)"), "commits \(code)")
        }
    }

    func testAnExpiredOrRevokedTokenFails() async {
        let revoked = await check(ctx(http: http(issues: 401), runner: runner()))
        XCTAssertEqual(revoked.state, .fail("GitHub refused the token: expired or revoked"))
        // A 401 on any request counts, not only on the first.
        let late = await check(ctx(http: http(other: 401), runner: runner()))
        XCTAssertEqual(late.state, .fail("GitHub refused the token: expired or revoked"))
        let past = await check(ctx(http: http(header: "2026-11-01 00:00:00 UTC"), runner: runner()))
        XCTAssertEqual(past.state, .fail("expired on 2026-11-01"))
        XCTAssertEqual(past.detected.tokenExpires, "2026-11-01")
    }

    func testMissingVaultEntryCantTell() async {
        let http = http()
        let result = await check(ctx(http: http, runner: runner(), values: [:]))
        XCTAssertEqual(result.state, .cantTell("m1-github-issues is not in the vault"))
        XCTAssertTrue(http.requests.isEmpty)
    }

    func testAMissingExpiryHeaderIsReportedNotInvented() async {
        let result = await check(ctx(http: http(header: nil), runner: runner()))
        guard case .pass = result.state else { return XCTFail("expected pass, got \(result.state)") }
        XCTAssertNil(result.detected.tokenExpires)
    }

    func testAServerErrorOrRateLimitCantTellAndNeverCountsAsDenied() async {
        let down = await check(ctx(http: http(issues: 503), runner: runner()))
        guard case .cantTell = down.state else { return XCTFail("503 gave \(down.state)") }
        let unreachable = await check(ctx(http: http(other: 0), runner: runner()))
        guard case .cantTell = unreachable.state else { return XCTFail("network error gave \(unreachable.state)") }

        // A rate-limited 403 is not a denial.
        var a: [String: HTTPAnswer] = [
            "/repos/\(repo)/issues?per_page=1": answer(200), "/repos/\(repo)/commits?per_page=1": answer(403),
        ]
        for name in others() { a["/repos/\(name)"] = answer(403, ["x-ratelimit-remaining": "0"]) }
        let limited = await check(ctx(http: FakeHTTP(answers: a), runner: runner()))
        guard case .cantTell = limited.state else { return XCTFail("rate limit gave \(limited.state)") }
    }

    // MARK: gh's listing must be trustworthy

    /// Today's second mistake must never pass because gh saw too little.
    func testAnEmptyGhListingCantTellEvenWhenTheOthersAnswer200() async {
        for listing in [[], ["dtthaison/some-other"]] as [[String]] {
            let result = await check(ctx(http: http(other: 200), runner: runner(listing: listing)))
            guard case let .cantTell(text) = result.state else { return XCTFail("\(listing) gave \(result.state)") }
            XCTAssertTrue(text.contains(repo), text)
            XCTAssertFalse(result.rows.first { $0.name == "Denied elsewhere" }!.ok)
        }
    }

    func testAServedRepoIsFoundInTheListingWhateverItsCase() async {
        let result = await check(ctx(http: http(), runner: runner(listing: [repo.uppercased()] + others())))
        guard case .pass = result.state else { return XCTFail("got \(result.state)") }
    }

    func testAListingWithOnlyTheServedReposPasses() async {
        let result = await check(ctx(http: http(), runner: runner(listing: [repo])))
        guard case .pass = result.state else { return XCTFail("got \(result.state)") }
    }

    func testAListingThatMayBeCutOrHasOddLinesCantTell() async {
        let full = [repo] + (1...999).map { "dtthaison/p-\($0)" }
        let cut = await check(ctx(http: http(), runner: runner(listing: full)))
        guard case .cantTell = cut.state else { return XCTFail("1000 names gave \(cut.state)") }
        let odd = await check(ctx(http: http(), runner: runner(listing: [repo, "not a repo name"] + others())))
        guard case .cantTell = odd.state else { return XCTFail("odd line gave \(odd.state)") }
    }

    func testASecondaryRateLimitBodyIsNotADenial() async {
        var a: [String: HTTPAnswer] = [
            "/repos/\(repo)/issues?per_page=1": answer(200), "/repos/\(repo)/commits?per_page=1": answer(403),
        ]
        let body = Data(#"{"message":"You have exceeded a secondary rate limit."}"#.utf8)
        for name in others() { a["/repos/\(name)"] = HTTPAnswer(status: 403, headers: [:], body: body) }
        let result = await check(ctx(http: FakeHTTP(answers: a), runner: runner()))
        guard case .cantTell = result.state else { return XCTFail("got \(result.state)") }
    }

    func testAnUnreadableExpiryHeaderCantTell() async {
        let result = await check(ctx(http: http(header: "soon"), runner: runner()))
        guard case let .cantTell(text) = result.state else { return XCTFail("got \(result.state)") }
        XCTAssertTrue(text.contains("expiry"), text)
        XCTAssertNil(result.detected.tokenExpires)
    }

    func testTheSameTokenWithStrayWhitespaceIsStillTheSame() async {
        let values = ["m1-github-issues": secret, "vps-sing-1-github-issues": secret + " \n"]
        let result = await check(ctx(http: http(), runner: runner(), values: values))
        XCTAssertEqual(result.state, .fail("same token as vps-sing-1 (vps-sing-1-github-issues)"))
    }

    func testAFailedGhListCantTell() async {
        let r = ScriptedRunner()
        r.on({ $0.contains("list") }, result: CommandResult(code: 1, out: "", err: "gh: not logged in"))
        let result = await check(ctx(http: http(), runner: r))
        guard case let .cantTell(text) = result.state else { return XCTFail("expected cantTell, got \(result.state)") }
        XCTAssertFalse(text.isEmpty)
    }

    func testGhListsEachOwnerOnceWithTheDocumentedArguments() async {
        var m = m1
        m.sdWorker?.repos.append(MachineRepo(github: "dtthaison/second", localPath: "/x", checkCommand: "true"))
        let r = runner()
        _ = await check(ctx(http: http(), runner: r, all: [m, vps], machine: m))
        let lists = r.calls.filter { $0.argv.contains("list") }
        XCTAssertEqual(lists.count, 1)
        XCTAssertEqual(Array(lists[0].argv.dropFirst()),
                       ["repo", "list", "dtthaison", "--visibility", "private", "--limit", "1000",
                        "--json", "nameWithOwner", "--jq", ".[].nameWithOwner"])
        XCTAssertTrue(lists[0].argv[0].hasSuffix("/gh"))
    }

    func testTokenScopeNeverPutsTheTokenInAnyResult() async {
        // A good run, a shared-token run, a 401 run and a wide-open run: none shows the value anywhere.
        let two = ["m1-github-issues": secret, "vps-sing-1-github-issues": "github_pat_OTHER"]
        let runs: [(FakeHTTP, [String: String])] = [
            (http(), two),
            (http(), ["m1-github-issues": secret, "vps-sing-1-github-issues": secret]),
            (http(issues: 401), two),
            (http(other: 200), two),
        ]
        for (http, values) in runs {
            let r = runner()
            let result = await check(ctx(http: http, runner: r, values: values))
            XCTAssertFalse(String(describing: result).contains("SECRET123"))
            XCTAssertFalse(String(reflecting: result).contains("SECRET123"))
            XCTAssertFalse(http.requests.isEmpty)
            for (url, bearer) in http.requests {
                XCTAssertFalse(url.absoluteString.contains("SECRET123"))
                XCTAssertEqual(bearer, secret)
            }
            for call in r.calls {
                XCTAssertFalse(call.argv.joined(separator: " ").contains("SECRET123"))
                XCTAssertFalse(call.env.values.contains { $0.contains("SECRET123") })
                XCTAssertNil(call.stdin)
            }
        }
        // The other machine's value stays out as well, and is never sent as a bearer.
        let http = http()
        let result = await check(ctx(http: http, runner: runner()))
        XCTAssertFalse(String(describing: result).contains("github_pat_OTHER"))
        XCTAssertFalse(http.requests.contains { $0.1 == "github_pat_OTHER" })
    }

    func testTheTokenGoesOnlyToApiGithubCom() async {
        let http = http()
        _ = await check(ctx(http: http, runner: runner()))
        XCTAssertEqual(http.requests.count, 2 + 30)
        for (url, _) in http.requests {
            XCTAssertEqual(url.scheme, "https")
            XCTAssertEqual(url.host, "api.github.com")
            XCTAssertNil(url.user)
            XCTAssertNil(url.port)
        }
    }

    func testARepoNameThatIsNotOwnerSlashNameNeverBecomesARequest() async {
        var m = m1
        m.sdWorker?.repos = [MachineRepo(github: "../../user", localPath: "/x", checkCommand: "true")]
        let http = http()
        let result = await check(ctx(http: http, runner: runner(), all: [m, vps], machine: m))
        guard case .cantTell = result.state else { return XCTFail("expected cantTell, got \(result.state)") }
        XCTAssertTrue(http.requests.isEmpty)
    }

    func testRequestsRunAtMostFourAtATime() async {
        let slow = SlowHTTP()
        _ = await check(ctx(http: slow, runner: runner()))
        XCTAssertGreaterThan(slow.maxInFlight, 1)
        XCTAssertLessThanOrEqual(slow.maxInFlight, 4)
    }

    func testExpiryHeaderParsing() {
        XCTAssertEqual(TokenScope.expiryDay(header: "2027-02-01 12:00:00 UTC"), "2027-02-01")
        XCTAssertEqual(TokenScope.expiryDay(header: "2027-02-01 12:00:00 +0700"), "2027-02-01")
        XCTAssertNil(TokenScope.expiryDay(header: "soon"))
        XCTAssertNil(TokenScope.expiryDay(header: ""))
        XCTAssertNil(TokenScope.expiryDay(header: "2027-02-30 00:00:00 UTC"))
    }

    // MARK: the fetcher

    func testTheFetcherRefusesAnyOtherHostWithoutSendingAnything() async {
        let fetcher = URLSessionFetcher()
        for text in ["https://evil.example/repos/a/b", "http://api.github.com/repos/a/b", "https://api.github.com.evil.example/x",
                     "https://user@api.github.com/x", "https://api.github.com:8443/x"] {
            let a = await fetcher.get(URL(string: text)!, bearer: secret)
            XCTAssertEqual(a.status, 0, text)
            XCTAssertTrue(a.headers.isEmpty, text)
        }
    }

    func testARedirectIsFollowedOnlyWithinApiGithubCom() {
        let session = URLSession(configuration: .ephemeral)
        defer { session.invalidateAndCancel() }
        let task = session.dataTask(with: URL(string: "https://api.github.com/repos/a/b")!)
        let guardian = GitHubRedirectGuard()
        let response = HTTPURLResponse(url: URL(string: "https://api.github.com/repos/a/b")!, statusCode: 301,
                                       httpVersion: nil, headerFields: nil)!

        /// Nil: the redirect is refused. Otherwise the request that would be sent.
        func decide(_ to: String) -> URLRequest? {
            var answered = false
            var out: URLRequest?
            var request = URLRequest(url: URL(string: to)!)
            request.setValue("Bearer \(secret)", forHTTPHeaderField: "Authorization")
            guardian.urlSession(session, task: task, willPerformHTTPRedirection: response, newRequest: request) {
                answered = true
                out = $0
            }
            XCTAssertTrue(answered, to)
            return out
        }
        XCTAssertNil(decide("https://evil.example/steal"))
        XCTAssertNil(decide("http://api.github.com/repositories/1"))
        XCTAssertNil(decide("https://uploads.github.com/x"))
        XCTAssertEqual(decide("https://api.github.com/repositories/1")?.url?.host, "api.github.com")
    }

    // MARK: the Fix

    func testTheFixOpensTheTokenFlowForTheMachinesSecret() async {
        let step = TokenScope.step(for: m1, job: job, open: { _ in }, form: { _ in })
        guard case let .run(plan) = step.fix else { return XCTFail("no fix") }
        XCTAssertFalse(plan.needsAdmin)
        XCTAssertTrue(plan.display.contains("m1-github-issues"))
        // The worker takes jobs, changes labels and comments, so the token needs Issues read and write.
        XCTAssertTrue(plan.display.contains("Issues: read and write"), plan.display)
        XCTAssertFalse(plan.display.contains("read-only"), plan.display)
        XCTAssertTrue(plan.display.contains("Metadata: read"), plan.display)
        let outcome = await plan.run(.test(ctx(http: http(), runner: runner())))
        XCTAssertTrue(outcome.ok)
        XCTAssertFalse(outcome.message.contains("SECRET123"))
    }
}
