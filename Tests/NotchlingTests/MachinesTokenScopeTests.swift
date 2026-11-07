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

    private let viewKey = "/user/repos?visibility=private&per_page=50"

    /// What GET /user/repos answers: a page of repos, with an optional Link header.
    private func viewAnswer(_ names: [String], status: Int = 200, link: String? = nil) -> HTTPAnswer {
        let json = "[" + names.map { #"{"id":1,"full_name":"\#($0)","private":true}"# }.joined(separator: ",") + "]"
        return HTTPAnswer(status: status, headers: link.map { ["link": $0] } ?? [:], body: Data(json.utf8))
    }

    /// The token's own view lists only the served repo unless a test says otherwise.
    private func table(issues: Int = 200, commits: Int = 403, other: Int = 404, header: String? = "2027-02-01 12:00:00 +0700",
                       view: HTTPAnswer? = nil) -> [String: HTTPAnswer] {
        var a: [String: HTTPAnswer] = [
            "/repos/\(repo)/issues?per_page=1": answer(issues, header.map { [expiryHeader: $0] } ?? [:]),
            "/repos/\(repo)/commits?per_page=1": answer(commits),
            viewKey: view ?? viewAnswer([repo]),
        ]
        for name in others() { a["/repos/\(name)"] = answer(other) }
        return a
    }

    private func http(issues: Int = 200, commits: Int = 403, other: Int = 404, header: String? = "2027-02-01 12:00:00 +0700",
                      view: HTTPAnswer? = nil) -> FakeHTTP {
        FakeHTTP(answers: table(issues: issues, commits: commits, other: other, header: header, view: view))
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

    /// M9: the check reads; it does not try a write. The row says so, so a ✓ is not read as "can write issues".
    func testTheIssuesRowStatesThatWriteIsNotTriedHere() async {
        let result = await check(ctx(http: http(), runner: runner()))
        let row = result.rows.first { $0.name == "Reaches issues" }
        XCTAssertTrue(row?.detail.hasPrefix("can read issues of ") == true, row?.detail ?? "")
        XCTAssertTrue(row?.detail.contains("a write is not tried here") == true, row?.detail ?? "")
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
            viewKey: viewAnswer([repo]),
        ]
        a["/repos/dtthaison/private-2"] = answer(200)
        let result = await check(ctx(http: FakeHTTP(answers: a), runner: runner()))
        XCTAssertEqual(result.state, .fail("can see 1 other private repo (dtthaison/private-2)"))
    }

    // MARK: all repositories, by choice

    private var allReposMachine: Machine {
        var m = m1
        m.sdWorker?.tokenAllRepos = true
        return m
    }

    private func allReposCtx(http: HTTPFetching) -> StepContext {
        ctx(http: http, runner: runner(), all: [allReposMachine, vps], machine: allReposMachine)
    }

    func testAStrictJobStillFailsWhenTheTokenReachesOtherRepos() async {
        var strict = m1
        strict.sdWorker?.tokenAllRepos = false
        let c = ctx(http: http(other: 200, view: viewAnswer([repo] + others(3))), runner: runner(), all: [strict, vps], machine: strict)
        let result = await check(c)
        guard case let .fail(text) = result.state else { return XCTFail("expected fail, got \(result.state)") }
        XCTAssertTrue(text.contains("other private repo"), text)
    }

    func testAnAllReposJobAcceptsOtherReposByChoice() async {
        let view = viewAnswer([repo] + others(30))
        let result = await check(allReposCtx(http: http(other: 200, view: view)))
        guard case .pass = result.state else { return XCTFail("expected pass, got \(result.state)") }
        let row = result.rows.first { $0.name == "Denied elsewhere" }
        XCTAssertEqual(row?.detail, "reaches all repositories, by choice (30 other private repos)")
        XCTAssertTrue(result.rows.allSatisfy(\.ok), "\(result.rows)")
    }

    func testAnAllReposJobIgnoresDoubtsAboutTheOtherRepos() async {
        // gh fails and the token's list errors: both only concern the other repos
        let r = ScriptedRunner()
        r.on({ $0.contains("repo") && $0.contains("list") }, result: CommandResult(code: 1, out: "", err: "no"))
        let c = ctx(http: http(view: viewAnswer([], status: 500)), runner: r, all: [allReposMachine, vps], machine: allReposMachine)
        let result = await check(c)
        guard case .pass = result.state else { return XCTFail("expected pass, got \(result.state)") }
    }

    func testAnAllReposJobStillFailsOnContentsAccess() async {
        let result = await check(allReposCtx(http: http(commits: 200, other: 200, view: viewAnswer([repo] + others(3)))))
        guard case let .fail(text) = result.state else { return XCTFail("expected fail, got \(result.state)") }
        XCTAssertTrue(text.contains("can read code in"), text)
    }

    func testAnAllReposJobStillFailsWhenAServedRepoLacksIssuesAccess() async {
        let result = await check(allReposCtx(http: http(issues: 404, other: 200, view: viewAnswer([repo] + others(3)))))
        guard case let .fail(text) = result.state else { return XCTFail("expected fail, got \(result.state)") }
        XCTAssertTrue(text.contains("no issues access to"), text)
    }

    func testAnAllReposJobStillFailsOnASharedToken() async {
        let values = ["m1-github-issues": secret, "vps-sing-1-github-issues": secret]
        let c = ctx(http: http(other: 200), runner: runner(), values: values, all: [allReposMachine, vps], machine: allReposMachine)
        let result = await check(c)
        guard case .fail = result.state else { return XCTFail("expected fail, got \(result.state)") }
    }

    func testTheAllReposFixDoesNotAskForSelectedRepositories() {
        let step = TokenScope.step(for: allReposMachine, job: allReposMachine.sdWorker!, open: { _ in }, form: { _ in })
        guard case let .run(plan) = step.fix else { return XCTFail("no fix") }
        XCTAssertTrue(plan.display.contains("any repositories"), plan.display)
        XCTAssertTrue(plan.display.contains("Issues: read and write") && plan.display.contains("no Contents"), plan.display)
        XCTAssertFalse(plan.display.contains(repo), plan.display)
        XCTAssertTrue(TokenCreation.clicks(repos: [repo], allRepos: true).first?.contains("All repositories") == true)
    }

    func testTheAllReposFlagDecodesFromOldFilesAndIsWrittenOnlyWhenTrue() throws {
        let old = #"{"python":"python3","sdPath":"/a/sd.py","repos":[],"kinds":[],"issuesSecret":"x"}"#
        var s = try JSONDecoder().decode(SDWorkerSettings.self, from: Data(old.utf8))
        XCTAssertNil(s.tokenAllRepos)
        XCTAssertFalse(String(decoding: try JSONEncoder().encode(s), as: UTF8.self).contains("tokenAllRepos"))
        s.tokenAllRepos = true
        let back = try JSONDecoder().decode(SDWorkerSettings.self, from: JSONEncoder().encode(s))
        XCTAssertEqual(back.tokenAllRepos, true)
    }

    func testTheJobFormSavesTheAllReposToggleAsTrueOrAbsent() {
        var draft = JobDraft(machine: m1)
        XCTAssertFalse(draft.tokenAllRepos)
        XCTAssertNil(JobForm.settings(draft, base: draft.base).tokenAllRepos)
        draft.tokenAllRepos = true
        XCTAssertEqual(JobForm.settings(draft, base: draft.base).tokenAllRepos, true)
        XCTAssertTrue(JobDraft(machine: Machine(name: "m1", host: "m1", user: "u", jobs: [Job(kind: .sdWorker, sdWorker: JobForm.settings(draft, base: draft.base))])).tokenAllRepos)
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
            viewKey: viewAnswer([repo]),
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
        // Every one of the 999 probes is answered 404 (denied), so only the 1000-name cap can make this cantTell.
        let full = [repo] + (1...999).map { "dtthaison/p-\($0)" }
        let wide = http()
        let cut = await check(ctx(http: wide, runner: runner(listing: full)))
        XCTAssertEqual(wide.requests.filter { $0.0.path.hasPrefix("/repos/dtthaison/p-") }.count, 999)
        guard case let .cantTell(text) = cut.state else { return XCTFail("1000 names gave \(cut.state)") }
        XCTAssertTrue(text.contains("cut"), text)
        // Nine hundred and ninety-nine lines are not cut.
        let notCut = await check(ctx(http: http(), runner: runner(listing: Array(full.dropLast()))))
        guard case .pass = notCut.state else { return XCTFail("999 names gave \(notCut.state)") }
        let odd = await check(ctx(http: http(), runner: runner(listing: [repo, "not a repo name"] + others())))
        guard case .cantTell = odd.state else { return XCTFail("odd line gave \(odd.state)") }
    }

    func testASecondaryRateLimitBodyIsNotADenial() async {
        var a: [String: HTTPAnswer] = [
            "/repos/\(repo)/issues?per_page=1": answer(200), "/repos/\(repo)/commits?per_page=1": answer(403),
            viewKey: viewAnswer([repo]),
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

    // MARK: the token's own view (GET /user/repos?visibility=private)

    func testATokenThatListsAll31PrivateReposFails() async {
        // Every per-repo probe says denied; the token's own list is what shows the mistake.
        let result = await check(ctx(http: http(view: viewAnswer([repo] + others())), runner: runner()))
        guard case let .fail(text) = result.state else { return XCTFail("got \(result.state)") }
        XCTAssertTrue(text.hasPrefix("token reaches 30 other private repos (e.g. dtthaison/private-1, dtthaison/private-10, dtthaison/private-11"), text)
        XCTAssertTrue(text.hasSuffix("set Only select repositories"), text)
        XCTAssertFalse(result.rows.first { $0.name == "Denied elsewhere" }!.ok)
    }

    func testATokenThatListsOnlyTheServedRepoPasses() async {
        let result = await check(ctx(http: http(view: viewAnswer([repo])), runner: runner()))
        guard case .pass = result.state else { return XCTFail("got \(result.state)") }
    }

    /// The issues probe passes, so the token reaches the served repo; a private-repo list that does not show it is
    /// not the whole truth, so the result is can't tell, never a pass.
    func testAViewThatLacksAServedRepoCantTell() async {
        let result = await check(ctx(http: http(view: viewAnswer([])), runner: runner()))
        guard case let .cantTell(text) = result.state else { return XCTFail("got \(result.state)") }
        XCTAssertTrue(text.contains(repo), text)
        XCTAssertFalse(result.rows.first { $0.name == "Denied elsewhere" }!.ok)
        // another repo in the list is a fail, which wins over the missing one
        let other = await check(ctx(http: http(view: viewAnswer(["dtthaison/something-else"])), runner: runner()))
        guard case .fail = other.state else { return XCTFail("got \(other.state)") }
        // the served repo's name is compared without regard to case
        let upper = await check(ctx(http: http(view: viewAnswer([repo.uppercased()])), runner: runner()))
        guard case .pass = upper.state else { return XCTFail("got \(upper.state)") }
    }

    /// A name that is not owner/name is doubt, not something to drop: the repo it names could be a leak.
    func testARepoNameTheCheckCantReadMakesTheViewDoubtful() async {
        let json = #"[{"full_name":"\#(repo)"},{"full_name":"bad name with spaces"}]"#
        let odd = await check(ctx(http: http(view: HTTPAnswer(status: 200, headers: [:], body: Data(json.utf8))), runner: runner()))
        guard case .cantTell = odd.state else { return XCTFail("a bad name gave \(odd.state)") }
        for body in [#"[{"full_name":"\#(repo)"},{"full_name":42}]"#, #"[{"full_name":"\#(repo)"},{"id":2}]"#,
                     #"[{"full_name":"\#(repo)"},"dtthaison/x"]"#, #"[{"full_name":"\#(repo)"},{"full_name":"a/../b"}]"#] {
            let result = await check(ctx(http: http(view: HTTPAnswer(status: 200, headers: [:], body: Data(body.utf8))), runner: runner()))
            guard case .cantTell = result.state else { return XCTFail("\(body) gave \(result.state)") }
        }
        // a leak that can be read still fails, whatever else is odd
        let leak = #"[{"full_name":"\#(repo)"},{"full_name":"dtthaison/leaked"},{"full_name":"bad name"}]"#
        let failed = await check(ctx(http: http(view: HTTPAnswer(status: 200, headers: [:], body: Data(leak.utf8))), runner: runner()))
        guard case .fail = failed.state else { return XCTFail("got \(failed.state)") }
    }

    func testTheViewCallIsPrivateOnlyAndPaged() async {
        let h = http()
        _ = await check(ctx(http: h, runner: runner()))
        XCTAssertEqual(h.requests.filter { $0.0.path == "/user/repos" }.map { $0.0.absoluteString },
                       ["https://api.github.com/user/repos?visibility=private&per_page=50"])
    }

    func testAnErrorOnTheViewCantTell() async {
        for status in [500, 403, 404, 0] {
            let result = await check(ctx(http: http(view: viewAnswer([], status: status)), runner: runner()))
            guard case .cantTell = result.state else { return XCTFail("view \(status) gave \(result.state)") }
        }
        var limited = viewAnswer([], status: 403)
        limited = HTTPAnswer(status: 403, headers: ["x-ratelimit-remaining": "0"], body: limited.body)
        let rate = await check(ctx(http: http(view: limited), runner: runner()))
        guard case .cantTell = rate.state else { return XCTFail("rate limit gave \(rate.state)") }
        let junk = await check(ctx(http: http(view: HTTPAnswer(status: 200, headers: [:], body: Data("not json".utf8))), runner: runner()))
        guard case .cantTell = junk.state else { return XCTFail("junk gave \(junk.state)") }
        let revoked = await check(ctx(http: http(view: viewAnswer([], status: 401)), runner: runner()))
        XCTAssertEqual(revoked.state, .fail("GitHub refused the token: expired or revoked"))
    }

    func testTheViewFollowsLinkPagesOnApiGithubComAndSeesTheLaterOnes() async {
        let next = "https://api.github.com/user/repos?visibility=private&per_page=50&page=2"
        var a = table(view: viewAnswer([repo], link: "<\(next)>; rel=\"next\", <\(next)>; rel=\"last\""))
        a["/user/repos?visibility=private&per_page=50&page=2"] = viewAnswer(["dtthaison/secret-other"])
        let h = FakeHTTP(answers: a)
        let result = await check(ctx(http: h, runner: runner()))
        XCTAssertEqual(result.state, .fail("token reaches 1 other private repo (e.g. dtthaison/secret-other); set Only select repositories"))
        XCTAssertTrue(h.requests.allSatisfy { $0.0.host == "api.github.com" && $0.1 == secret })
    }

    func testALinkHeaderCannotMoveTheTokenToAnotherHost() async {
        for target in ["https://evil.example/user/repos?page=2", "http://api.github.com/user/repos?page=2",
                       "https://api.github.com.evil.example/user/repos?page=2", "https://api.github.com:8443/user/repos?page=2",
                       "https://api.github.com/repos/dtthaison/lingeva-mono/issues"] {
            let h = http(view: viewAnswer([repo], link: "<\(target)>; rel=\"next\""))
            let result = await check(ctx(http: h, runner: runner()))
            guard case .cantTell = result.state else { return XCTFail("\(target) gave \(result.state)") }
            XCTAssertFalse(h.requests.contains { $0.0.absoluteString == target }, target)
            XCTAssertTrue(h.requests.allSatisfy { $0.0.host == "api.github.com" && $0.0.scheme == "https" && $0.0.port == nil })
        }
    }

    func testTheViewStopsAfterTenPagesAndCantTell() async {
        var a = table()
        let base = "https://api.github.com/user/repos?visibility=private&per_page=50"
        a[viewKey] = viewAnswer([repo], link: "<\(base)&page=2>; rel=\"next\"")
        for n in 2...30 { a["/user/repos?visibility=private&per_page=50&page=\(n)"] = viewAnswer([repo], link: "<\(base)&page=\(n + 1)>; rel=\"next\"") }
        let h = FakeHTTP(answers: a)
        let result = await check(ctx(http: h, runner: runner()))
        guard case .cantTell = result.state else { return XCTFail("got \(result.state)") }
        XCTAssertEqual(h.requests.filter { $0.0.path == "/user/repos" }.count, 10)
    }

    func testACutOffViewPageStillFailsOnWhatItShowedButNeverPasses() async {
        // A page the 1 MiB cap cut: the names seen before the cut count, and without one the result is cantTell.
        let pad = String(repeating: "x", count: URLSessionFetcher.bodyCap)
        let cutBody = #"[{"full_name":"dtthaison/leaked"},{"description":"\#(pad)"#
        let leaked = await check(ctx(http: http(view: HTTPAnswer(status: 200, headers: [:], body: Data(cutBody.prefix(URLSessionFetcher.bodyCap).utf8))), runner: runner()))
        guard case .fail = leaked.state else { return XCTFail("got \(leaked.state)") }
        let quiet = #"[{"full_name":"\#(repo)"},{"description":"\#(pad)"#
        let cut = await check(ctx(http: http(view: HTTPAnswer(status: 200, headers: [:], body: Data(quiet.prefix(URLSessionFetcher.bodyCap).utf8))), runner: runner()))
        guard case .cantTell = cut.state else { return XCTFail("got \(cut.state)") }
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
        XCTAssertEqual(http.requests.count, 2 + 30 + 1)  // issues, commits, the others, the token's own list
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
        // it only opens the page and the form: the step is not done until a check passes, so Set up all stops here
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.hasSuffix("Fix the token, save it, then Check again."), outcome.message)
        XCTAssertFalse(outcome.message.contains("SECRET123"))
    }
}
