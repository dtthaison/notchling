import XCTest
@testable import Notchling

/// A thread-safe list for the closures a Fix calls from its own task.
private final class Seen<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [T] = []
    func add(_ item: T) { lock.withLock { items.append(item) } }
    var all: [T] { lock.withLock { items } }
}

final class MachinesSecretsTests: XCTestCase {
    private let claudeValue = "sk-ant-oat01-SECRETVALUE"

    private func claudeUsage(_ machine: String = "m1") -> SecretUsage {
        SecretUsage(name: "claude-worker", machine: machine, remotePath: MachinePaths.claudeEnv,
                    format: .keyValue(MachinePaths.claudeKey))
    }

    private func fixContext(runner: ScriptedRunner, secrets: FakeSecrets, machine: Machine = TestMachines.mac(),
                            log: @escaping @Sendable (String) -> Void = { _ in }) -> FixContext {
        .test(.test(machine: machine, runner: runner, secrets: secrets), log: log)
    }

    private func date(_ text: String) -> Date {
        VaultExpiry.date(from: text) ?? Date(timeIntervalSince1970: 0)
    }

    // MARK: Usages

    func testUsagesOfEveryMachine() {
        let usages = SecretSteps.usages(in: [TestMachines.mac(), TestMachines.linux()])
        XCTAssertEqual(usages.count, 4)
        let claude = usages.filter { $0.name == "claude-worker" }
        XCTAssertEqual(claude.map(\.machine), ["m1", "vps-sing-1"])
        XCTAssertTrue(claude.allSatisfy { $0.remotePath == "~/.config/agents/claude.env"
            && $0.format == .keyValue("CLAUDE_CODE_OAUTH_TOKEN") })
        XCTAssertTrue(usages.contains(SecretUsage(name: "m1-github-issues", machine: "m1",
                                                   remotePath: "~/.config/agents/github-token", format: .raw)))
        XCTAssertTrue(usages.contains(SecretUsage(name: "vps-sing-1-github-issues", machine: "vps-sing-1",
                                                   remotePath: "~/.config/agents/github-token", format: .raw)))
    }

    // MARK: Delivery

    func testDeliveryPutsTheValueOnlyOnStdin() async {
        let runner = ScriptedRunner()
        let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": claudeValue]))
        let outcome = await SecretSteps.deliver(claudeUsage(), ctx: ctx)
        XCTAssertTrue(outcome.ok, outcome.message)
        let calls = runner.calls
        XCTAssertEqual(calls.count, 1)
        XCTAssertEqual(calls[0].stdin, Data("CLAUDE_CODE_OAUTH_TOKEN=\(claudeValue)\n".utf8))
        XCTAssertFalse(calls[0].argv.contains { $0.contains("SECRETVALUE") })
        XCTAssertFalse(calls[0].env.values.contains { $0.contains("SECRETVALUE") })
        XCTAssertEqual(calls[0].argv, SSH.script(TestMachines.mac(), RemoteShell.writeFile("~/.config/agents/claude.env")))
        XCTAssertFalse(outcome.message.contains("SECRETVALUE"))
    }

    func testAMachineThatEchoesTheValueNeverShowsItInTheOutcomeOrTheLog() async {
        let echo = "cat: \(claudeValue) CLAUDE_CODE_OAUTH_TOKEN=\(claudeValue)"
        let runner = ScriptedRunner()
        runner.on({ _ in true }, result: CommandResult(code: 1, out: "", err: echo), chunks: [echo, "again \(claudeValue)\n"])
        let lines = LogBox()
        let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": claudeValue]), log: { lines.add($0) })
        let outcome = await SecretSteps.deliver(claudeUsage(), ctx: ctx)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("[secret:claude-worker]"), outcome.message)
        XCTAssertFalse(outcome.message.contains("SECRETVALUE"), outcome.message)
        XCTAssertFalse(lines.lines.joined().contains("SECRETVALUE"), lines.lines.joined())
        XCTAssertTrue(lines.lines.joined().contains("[secret:claude-worker]"))
    }

    func testATooShortValueIsRefused() async {
        let runner = ScriptedRunner()
        let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": "abc1234"]))
        let outcome = await SecretSteps.deliver(claudeUsage(), ctx: ctx)
        XCTAssertFalse(outcome.ok)
        XCTAssertEqual(outcome.message, "the value of claude-worker is too short; save it again")
        XCTAssertTrue(runner.calls.isEmpty)
    }

    /// A realistic Claude token is 108 characters. The echo starts 250 characters in, so a cut at 300 would
    /// split it; no 8-character piece of it may reach the message.
    func testAnEchoThatStartsLateIsRedactedBeforeTheMessageIsCut() async {
        let alphabet = Array("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789")
        let value = "sk-ant-oat01-" + String((0..<95).map { alphabet[($0 * 7) % alphabet.count] })
        XCTAssertEqual(value.count, 108)
        for onStderr in [true, false] {
            let said = String(repeating: "x", count: 250) + value + " tail"
            let result = onStderr ? CommandResult(code: 1, out: "", err: said) : CommandResult(code: 1, out: said, err: "")
            let runner = ScriptedRunner()
            runner.on({ _ in true }, result: result)
            let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": value]))
            let outcome = await SecretSteps.deliver(claudeUsage(), ctx: ctx)
            XCTAssertFalse(outcome.ok)
            let chars = Array(value)
            for start in 0...(chars.count - 8) {
                XCTAssertFalse(outcome.message.contains(String(chars[start..<start + 8])), "fragment at \(start): \(outcome.message)")
            }
            XCTAssertTrue(outcome.message.contains("[secret:claude-worker]"), outcome.message)
        }
    }

    func testAHugeOrCutEchoIsNotShownAtAll() async {
        for said in ["[earlier output cut]\nrest of \(claudeValue)", String(repeating: "y", count: 5000)] {
            let runner = ScriptedRunner()
            runner.on({ _ in true }, result: CommandResult(code: 1, out: "", err: said))
            let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": claudeValue]))
            let outcome = await SecretSteps.deliver(claudeUsage(), ctx: ctx)
            XCTAssertFalse(outcome.ok)
            XCTAssertFalse(outcome.message.contains("SECRETVALUE"))
            XCTAssertFalse(outcome.message.contains("yyyy"))
            XCTAssertFalse(outcome.message.contains("rest of"))
        }
    }

    func testRawTokenHasNoNewline() async {
        let runner = ScriptedRunner()
        let usage = SecretUsage(name: "m1-github-issues", machine: "m1", remotePath: MachinePaths.githubToken, format: .raw)
        let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["m1-github-issues": "github_pat_ABC"]))
        let outcome = await SecretSteps.deliver(usage, ctx: ctx)
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertEqual(runner.calls.first?.stdin, Data("github_pat_ABC".utf8))
        XCTAssertEqual(runner.calls.first?.argv, SSH.script(TestMachines.mac(), RemoteShell.writeFile(MachinePaths.githubToken)))
    }

    func testAValueWithALineBreakIsRefused() async {
        for bad in ["abc\ndef", "abc\rdef", "abc\0def"] {
            let runner = ScriptedRunner()
            let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": bad]))
            let outcome = await SecretSteps.deliver(claudeUsage(), ctx: ctx)
            XCTAssertFalse(outcome.ok)
            XCTAssertEqual(outcome.message, "the value of claude-worker has a line break; save it again")
            XCTAssertTrue(runner.calls.isEmpty)
        }
    }

    func testAMissingVaultEntryIsNotADelivery() async {
        let runner = ScriptedRunner()
        let outcome = await SecretSteps.deliver(claudeUsage(), ctx: fixContext(runner: runner, secrets: FakeSecrets()))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
        let empty = await SecretSteps.deliver(claudeUsage(), ctx: fixContext(
            runner: runner, secrets: FakeSecrets(values: ["claude-worker": ""])))
        XCTAssertFalse(empty.ok)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    func testAUsageOfAnotherMachineIsNotDelivered() async {
        let runner = ScriptedRunner()
        let ctx = fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": claudeValue]))
        let outcome = await SecretSteps.deliver(claudeUsage("vps-sing-1"), ctx: ctx)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    // MARK: Verification

    private func parse(_ probe: Probe, _ text: String, code: Int32 = 0) async -> CheckState? {
        guard case let .remote(_, parse) = probe else { return nil }
        return await parse(RemoteOutput(text: text, code: code), .test()).state
    }

    func testVerifyLooksOnlyAtModeAndSize() async {
        for linux in [false, true] {
            let probe = SecretSteps.verifyProbe(claudeUsage(), linux: linux)
            guard case let .remote(script, _) = probe else { return XCTFail("not a remote probe") }
            XCTAssertEqual(script, RemoteShell.stat([MachinePaths.claudeEnv], linux: linux))
            XCTAssertFalse(script.contains("cat"))
            XCTAssertFalse(script.contains("head"))
        }
        let probe = SecretSteps.verifyProbe(claudeUsage(), linux: false)
        let pass = await parse(probe, "600 41\n")
        XCTAssertEqual(pass, .pass("mode 600, 41 bytes"))
        let missing = await parse(probe, "missing\n")
        XCTAssertEqual(missing, .fail("not on the machine"))
        let loose = await parse(probe, "644 41\n")
        XCTAssertEqual(loose, .fail("mode 644, should be 600"))
        let empty = await parse(probe, "600 0\n")
        XCTAssertEqual(empty, .fail("empty"))
        let garbage = await parse(probe, "Permission denied\n")
        if case .cantTell? = garbage {} else { XCTFail("expected can't tell, got \(String(describing: garbage))") }
        let failed = await parse(probe, "600 41\n", code: 255)
        if case .cantTell? = failed {} else { XCTFail("expected can't tell, got \(String(describing: failed))") }
    }

    // MARK: Steps

    func testClaudeLoginFixOpensTheFormWhenTheVaultLacksTheToken() async {
        let forms = Seen<VaultRequest>()
        let runner = ScriptedRunner()
        let step = SecretSteps.claudeLogin(for: TestMachines.mac(), form: { forms.add($0) })
        guard case let .run(plan) = step.fix else { return XCTFail("not a Fix") }
        let outcome = await plan.run(fixContext(runner: runner, secrets: FakeSecrets()))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
        let request = forms.all.first
        XCTAssertEqual(forms.all.count, 1)
        XCTAssertEqual(request?.name, "claude-worker")
        XCTAssertEqual(request?.kind, "token")
        XCTAssertEqual(request?.url, "https://claude.ai")
        XCTAssertEqual(request?.note, "Claude Code OAuth token of the separate worker account (every machine)")
        XCTAssertEqual(request?.howTo, "In your own Terminal (not an agent): run claude setup-token, sign in with the worker account in a private browser window, paste the token here, then clear the Terminal.")
    }

    func testClaudeLoginFixDeliversWhenTheVaultHasTheToken() async {
        let forms = Seen<VaultRequest>()
        let runner = ScriptedRunner()
        let step = SecretSteps.claudeLogin(for: TestMachines.mac(), form: { forms.add($0) })
        guard case let .run(plan) = step.fix else { return XCTFail("not a Fix") }
        let outcome = await plan.run(fixContext(runner: runner, secrets: FakeSecrets(values: ["claude-worker": claudeValue])))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertTrue(forms.all.isEmpty)
        XCTAssertEqual(runner.calls.count, 1)
        XCTAssertFalse(plan.display.contains("SECRETVALUE"))
        XCTAssertTrue(plan.display.contains("[secret:claude-worker]"))
    }

    func testIssuesTokenFixOpensTheCreationFlowWhenTheVaultLacksIt() async {
        let machine = TestMachines.mac()
        let opened = Seen<URL>(), forms = Seen<VaultRequest>()
        let runner = ScriptedRunner()
        let step = SecretSteps.issuesToken(for: machine, job: machine.sdWorker!, open: { opened.add($0) }, form: { forms.add($0) })
        guard case let .run(plan) = step.fix else { return XCTFail("not a Fix") }
        let outcome = await plan.run(fixContext(runner: runner, secrets: FakeSecrets()))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
        XCTAssertEqual(opened.all.first?.path, "/settings/personal-access-tokens/new")
        XCTAssertEqual(forms.all.first?.name, "m1-github-issues")
    }

    func testIssuesTokenFixDeliversWhenTheVaultHasIt() async {
        let machine = TestMachines.mac()
        let opened = Seen<URL>(), forms = Seen<VaultRequest>()
        let runner = ScriptedRunner()
        let step = SecretSteps.issuesToken(for: machine, job: machine.sdWorker!, open: { opened.add($0) }, form: { forms.add($0) })
        guard case let .run(plan) = step.fix else { return XCTFail("not a Fix") }
        let outcome = await plan.run(fixContext(runner: runner, secrets: FakeSecrets(values: ["m1-github-issues": "github_pat_ABC"])))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertTrue(opened.all.isEmpty && forms.all.isEmpty)
        XCTAssertEqual(runner.calls.first?.stdin, Data("github_pat_ABC".utf8))
    }

    func testTheProbesOfBothStepsLookAtTheirFile() {
        let machine = TestMachines.linux()
        let claude = SecretSteps.claudeLogin(for: machine)
        let issues = SecretSteps.issuesToken(for: machine, job: machine.sdWorker!)
        guard case let .remote(claudeScript, _) = claude.probe, case let .remote(issuesScript, _) = issues.probe else {
            return XCTFail("not remote probes")
        }
        XCTAssertEqual(claudeScript, RemoteShell.stat([MachinePaths.claudeEnv], linux: true))
        XCTAssertEqual(issuesScript, RemoteShell.stat([MachinePaths.githubToken], linux: true))
    }

    // MARK: Token creation

    func testTokenPageURL() {
        let url = TokenCreation.pageURL(secret: "m1-github-issues", machine: "m1", repos: ["dtthaison/lingeva-mono"],
                                        today: date("2026-11-06"))
        let parts = URLComponents(url: url, resolvingAgainstBaseURL: false)
        XCTAssertEqual(parts?.host, "github.com")
        XCTAssertEqual(parts?.path, "/settings/personal-access-tokens/new")
        let items = (parts?.queryItems ?? []).map { [$0.name, $0.value ?? ""] }
        XCTAssertEqual(items, [["name", "m1-github-issues"], ["description", "Issues only, for the sd worker on m1"],
                               ["target_name", "dtthaison"], ["expires_in", "366"], ["issues", "write"]])
    }

    func testTokenClicksNameOnlySelectRepositories() {
        let clicks = TokenCreation.clicks(repos: ["dtthaison/lingeva-mono", "dtthaison/other"])
        XCTAssertEqual(clicks, [
            "Repository access: choose Only select repositories, then pick dtthaison/lingeva-mono, dtthaison/other.",
            "Permissions: Issues Read and write. Leave Contents at No access.",
            "Generate token, copy it, and delete any old token for this machine on the same page.",
            "Paste it into Notchling's form and Save.",
        ])
        XCTAssertFalse(clicks.joined().contains("All repositories"))
    }

    func testTokenRequestPrefillsHowAndExpires() {
        let today = date("2026-11-06")
        let request = TokenCreation.request(secret: "m1-github-issues", machine: "m1", repos: ["dtthaison/lingeva-mono"], today: today)
        XCTAssertEqual(request.op, .request)
        XCTAssertEqual(request.name, "m1-github-issues")
        XCTAssertEqual(request.kind, "token")
        XCTAssertEqual(request.url, "https://github.com")
        XCTAssertEqual(request.note, "Issues-only token for the sd worker on m1: dtthaison/lingeva-mono")
        XCTAssertEqual(request.expires, "2027-11-07")
        XCTAssertEqual(request.reason, "Machines: the GitHub issues token for m1")
        let page = TokenCreation.pageURL(secret: "m1-github-issues", machine: "m1", repos: ["dtthaison/lingeva-mono"], today: today)
        XCTAssertTrue(request.howTo?.contains(page.absoluteString) == true)
        for click in TokenCreation.clicks(repos: ["dtthaison/lingeva-mono"]) {
            XCTAssertTrue(request.howTo?.contains(click) == true, click)
        }
    }

    @MainActor
    func testStartOpensThePageAndTheForm() {
        var opened: [URL] = [], forms: [VaultRequest] = []
        TokenCreation.start(secret: "m1-github-issues", machine: "m1", repos: ["dtthaison/lingeva-mono"],
                            open: { opened.append($0) }, form: { forms.append($0) })
        XCTAssertEqual(opened.count, 1)
        XCTAssertEqual(forms.count, 1)
        XCTAssertEqual(opened.first?.host, "github.com")
        XCTAssertEqual(forms.first?.name, "m1-github-issues")
    }

    // MARK: Renames

    private func entry(_ name: String, kind: VaultEntry.Kind = .token) -> VaultEntry {
        let vars = VaultEntry.defaultVars(name: name, kind: kind)
        return VaultEntry(name: name, kind: kind, userVar: kind == .login ? vars.user : "", secretVar: vars.secret)
    }

    func testLegacyRenames() {
        let list = [entry("sd-worker-m1"), entry("sd-claude"), entry("claude-worker"), entry("other")]
        let renames = SecretNames.pendingRenames(in: list)
        XCTAssertEqual(renames.map(\.0.name), ["sd-worker-m1"])
        XCTAssertEqual(renames.map(\.1), ["m1-github-issues"])
        XCTAssertEqual(SecretNames.legacy.map(\.from), ["sd-worker-m1", "sd-worker-vps-sing-1", "sd-claude"])
        XCTAssertEqual(SecretNames.legacy.map(\.to), ["m1-github-issues", "vps-sing-1-github-issues", "claude-worker"])
        XCTAssertTrue(SecretNames.pendingRenames(in: []).isEmpty)
    }

    func testRenamedEntryVariablesFollowTheNewName() {
        let renamed = entry("sd-worker-m1").renamed(to: "m1-github-issues")
        XCTAssertEqual(renamed.name, "m1-github-issues")
        XCTAssertEqual(renamed.secretVar, "M1_GITHUB_ISSUES_TOKEN")
        var custom = entry("sd-worker-m1")
        custom.secretVar = "GH_TOKEN"
        XCTAssertEqual(custom.renamed(to: "m1-github-issues").secretVar, "GH_TOKEN")
        let login = entry("old-login", kind: .login).renamed(to: "new-login")
        XCTAssertEqual(login.userVar, "NEW_LOGIN_USERNAME")
        XCTAssertEqual(login.secretVar, "NEW_LOGIN_PASSWORD")
    }

    func testRenamedEntryKeepsEverythingElse() {
        var old = entry("sd-claude")
        old.note = "n"; old.url = "https://claude.ai"; old.howTo = "h"; old.expires = "2027-01-01"; old.ask = true
        let renamed = old.renamed(to: "claude-worker")
        XCTAssertEqual(renamed.note, "n")
        XCTAssertEqual(renamed.url, "https://claude.ai")
        XCTAssertEqual(renamed.howTo, "h")
        XCTAssertEqual(renamed.expires, "2027-01-01")
        XCTAssertEqual(renamed.ask, true)
        XCTAssertEqual(renamed.updated, old.updated)
    }

    func testValueChangedOnlyWhenTheValueDiffers() {
        let a = VaultValue(username: "u", secret: "s")
        XCTAssertFalse(VaultStore.changed(old: a, new: VaultValue(username: "u", secret: "s")))
        XCTAssertTrue(VaultStore.changed(old: a, new: VaultValue(username: "u", secret: "t")))
        XCTAssertTrue(VaultStore.changed(old: a, new: VaultValue(username: "v", secret: "s")))
        XCTAssertTrue(VaultStore.changed(old: nil, new: a))
    }

    func testTheNoticeCarriesOnlyTheNameAndOnlyForAChangedValue() {
        let center = NotificationCenter()
        let seen = Seen<[AnyHashable: Any]>()
        let onMain = Seen<Bool>()
        let posted = expectation(description: "posted")
        let token = center.addObserver(forName: .vaultValueChanged, object: nil, queue: nil) { note in
            seen.add(note.userInfo ?? [:]); onMain.add(Thread.isMainThread); posted.fulfill()
        }
        defer { center.removeObserver(token) }
        let a = VaultValue(username: "", secret: "old-secret-value")
        VaultStore.announce(name: "claude-worker", before: a, after: a, center: center)
        VaultStore.announce(name: "claude-worker", before: a, after: VaultValue(username: "", secret: "new-secret-value"), center: center)
        wait(for: [posted], timeout: 2)
        XCTAssertEqual(seen.all.count, 1)
        XCTAssertEqual(seen.all.first?.count, 1)
        XCTAssertEqual(seen.all.first?["name"] as? String, "claude-worker")
        XCTAssertEqual(onMain.all, [true])
    }

    // MARK: Expiry

    func testSetExpiresWritesOnlyTheIndex() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("t7-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("vault.json")
        var a = entry("m1-github-issues"); a.expires = "2026-12-01"; a.note = "keep"
        var b = entry("other"); b.expires = "2026-12-02"
        try SecretVault.encode([a, b]).write(to: url)

        try SecretVault.setExpires("M1-github-issues", day: "2027-11-07", in: url)
        var list = SecretVault.decode(try Data(contentsOf: url))
        XCTAssertEqual(list.count, 2)
        XCTAssertEqual(list.first { $0.name == "m1-github-issues" }?.expires, "2027-11-07")
        XCTAssertEqual(list.first { $0.name == "m1-github-issues" }?.note, "keep")
        XCTAssertEqual(list.first { $0.name == "other" }, SecretVault.decode(try SecretVault.encode([b])).first)

        try SecretVault.setExpires("m1-github-issues", day: nil, in: url)
        list = SecretVault.decode(try Data(contentsOf: url))
        XCTAssertNil(list.first { $0.name == "m1-github-issues" }?.expires)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path), ["vault.json"])
    }

    func testSetExpiresRefusesWhatItCannotRewriteSafely() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("t7-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("vault.json")
        try Data("not json".utf8).write(to: url)
        XCTAssertThrowsError(try SecretVault.setExpires("a", day: "2027-01-01", in: url))
        XCTAssertEqual(try Data(contentsOf: url), Data("not json".utf8))

        try SecretVault.encode([entry("a")]).write(to: url)
        let before = try Data(contentsOf: url)
        XCTAssertThrowsError(try SecretVault.setExpires("missing", day: "2027-01-01", in: url))
        XCTAssertThrowsError(try SecretVault.setExpires("a", day: "tomorrow", in: url))
        XCTAssertEqual(try Data(contentsOf: url), before)
    }

    // MARK: Vault as a source

    func testVaultSecretSourceAsksItsInjectedIndexAndStore() {
        let known = entry("claude-worker")
        let source = VaultSecretSource(entry: { $0 == "claude-worker" ? known : nil },
                                       value: { $0.name == "claude-worker" ? VaultValue(secret: "tok") : nil })
        XCTAssertEqual(source.value(named: "claude-worker"), "tok")
        XCTAssertEqual(source.entry(named: "claude-worker"), known)
        XCTAssertNil(source.value(named: "other"))
        XCTAssertNil(source.entry(named: "other"))
    }
}
