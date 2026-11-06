import XCTest
@testable import Notchling

final class MachinesRepoStepsTests: XCTestCase {
    private let repo = TestMachines.repo
    private let gh = MachineTools.path("gh")
    private let alias = "github-agents-dtthaison-lingeva-mono"

    private let body = "AAAAC3NzaC1lZDI1NTE5AAAAIMachineKeyForTestsOnly0123456789abcdefg"
    private let otherBody = "AAAAC3NzaC1lZDI1NTE5AAAAIOtherKeyThatIsNotGitHubs0123456789abcdef"
    /// GitHub's published ed25519 host key.
    private let githubHostBody = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"
    private var pub: String { "ssh-ed25519 \(body) agents-m1" }

    private func steps(machine: Machine = TestMachines.mac()) -> [SetupStep] {
        RepoSteps.all(for: machine, repo: machine.repos[0])
    }

    private func probe(_ id: StepID, machine: Machine = TestMachines.mac()) -> (script: String, parse: @Sendable (RemoteOutput, StepContext) async -> CheckResult) {
        let step = steps(machine: machine).first { $0.id == id }!
        guard case let .remote(script, parse) = step.probe else { fatalError("not a remote probe") }
        return (script, parse)
    }

    private func plan(_ id: StepID, machine: Machine = TestMachines.mac()) -> FixPlan {
        let step = steps(machine: machine).first { $0.id == id }!
        guard case let .run(plan) = step.fix else { fatalError("not a Fix") }
        return plan
    }

    /// The script as the machine's shell sees it: ssh's argument is the script wrapped in one more layer of quotes.
    private func unquoted(_ arg: String) -> String { arg.replacingOccurrences(of: "'\\''", with: "'") }

    private func remoteText(pub: String, alias aliasLine: String = "alias=yes", ssh: String) -> String {
        "\(pub)\n@@config\n\(aliasLine)\n@@ssh\n\(ssh)\n"
    }

    private let welcome = "Hi dtthaison/lingeva-mono! You've successfully authenticated, but GitHub does not provide shell access."

    private func ghKeys(_ key: String, readOnly: Bool?) -> String {
        let flag = readOnly.map { ",\"read_only\":\($0)" } ?? ""
        return "[{\"id\":1,\"key\":\"\(key)\",\"title\":\"agents-m1\"\(flag)}]"
    }

    private func check(text: String, keys: String, runner: ScriptedRunner? = nil) async -> (CheckResult, ScriptedRunner) {
        let runner = runner ?? ScriptedRunner()
        if runner.calls.isEmpty {
            runner.on({ $0.contains("repos/\(self.repo)/keys") }, result: CommandResult(code: 0, out: keys, err: ""))
        }
        let result = await probe(.deployKey(repo)).parse(RemoteOutput(text: text, code: 0), .test(runner: runner))
        return (result, runner)
    }

    // MARK: Names

    func testAliasKeyAndRemoteNames() {
        XCTAssertEqual(RepoSteps.slug("dtthaison/lingeva-mono"), "dtthaison-lingeva-mono")
        XCTAssertEqual(RepoSteps.slug("Me/My.App"), "me-my-app")
        XCTAssertEqual(RepoSteps.alias(repo), alias)
        XCTAssertEqual(RepoSteps.keyFile(repo), "~/.ssh/agents-dtthaison-lingeva-mono")
        XCTAssertEqual(RepoSteps.remoteURL(repo), "git@\(alias):dtthaison/lingeva-mono.git")
    }

    func testRepoNamesFollowGitHubsRules() {
        for ok in ["dtthaison/lingeva-mono", "org/.github", "org/_x", "a/b.c_d-e", "Me/My.App", "org/...x"] {
            XCTAssertTrue(RepoSteps.isValidRepo(ok), ok)
        }
        for bad in ["org/.", "org/..", "org/-x", "-org/x", ".org/x", "a/b/c", "a/b; touch x", "a", "/b", "a/", "a/b\nc", "a/$(id)"] {
            XCTAssertFalse(RepoSteps.isValidRepo(bad), bad)
        }
    }

    func testTheAliasGuardMatchesIndentedAndPaddedHostLinesOnly() throws {
        let pattern = RepoSteps.aliasPattern(alias)
        let dir = NSTemporaryDirectory() + "repo-steps-\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: dir) }
        func matches(_ line: String) throws -> Bool {
            let file = dir + "/config"
            try (line + "\n").write(toFile: file, atomically: true, encoding: .utf8)
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/grep")
            p.arguments = ["-Eq", pattern, file]
            try p.run()
            p.waitUntilExit()
            return p.terminationStatus == 0
        }
        for line in ["Host \(alias)", "  Host \(alias)", "\tHost  \(alias)", "Host \(alias)  ", "Host \(alias) other"] {
            XCTAssertTrue(try matches(line), line)
        }
        for line in ["Host \(alias)-2", "Host other", "# Host \(alias)", "Hostname \(alias)", "Host x\(alias)"] {
            XCTAssertFalse(try matches(line), line)
        }
    }

    func testTheCheckoutDefaultsToSrcName() {
        var r = MachineRepo(github: repo, localPath: "/x", checkCommand: "true")
        XCTAssertEqual(RepoSteps.checkoutPath(r), "~/src/lingeva-mono")
        r.machinePath = "/srv/mono"
        XCTAssertEqual(RepoSteps.checkoutPath(r), "/srv/mono")
    }

    // MARK: Deploy key Check

    func testDeployKeyPassesOnlyWhenGitHubListsItReadOnlyAndSshTWorks() async {
        let (ok, runner) = await check(text: remoteText(pub: pub, ssh: welcome), keys: ghKeys("ssh-ed25519 \(body)", readOnly: true))
        XCTAssertEqual(ok.state, .pass("read-only key on \(repo)"))
        XCTAssertEqual(runner.calls.map(\.argv), [[gh, "api", "--paginate", "repos/\(repo)/keys"]])

        let (writable, _) = await check(text: remoteText(pub: pub, ssh: welcome), keys: ghKeys("ssh-ed25519 \(body)", readOnly: false))
        guard case let .fail(why) = writable.state else { return XCTFail("\(writable.state)") }
        XCTAssertTrue(why.contains("can write"), why)

        let (absent, _) = await check(text: remoteText(pub: pub, ssh: welcome), keys: ghKeys("ssh-ed25519 \(otherBody)", readOnly: true))
        guard case let .fail(missing) = absent.state else { return XCTFail("\(absent.state)") }
        XCTAssertTrue(missing.contains("not on dtthaison/lingeva-mono yet"), missing)

        let (denied, _) = await check(text: remoteText(pub: pub, ssh: "git@github.com: Permission denied (publickey)."),
                                      keys: ghKeys("ssh-ed25519 \(body)", readOnly: true))
        guard case .fail = denied.state else { return XCTFail("\(denied.state)") }
    }

    func testAKeyWithoutReadOnlyFieldIsNotTrustedAsReadOnly() async {
        let (result, _) = await check(text: remoteText(pub: pub, ssh: welcome), keys: ghKeys("ssh-ed25519 \(body)", readOnly: nil))
        guard case let .fail(why) = result.state else { return XCTFail("\(result.state)") }
        XCTAssertTrue(why.contains("can write"), why)
    }

    func testAMissingAliasOrMissingKeyFileFails() async {
        let keys = ghKeys("ssh-ed25519 \(body)", readOnly: true)
        let (noAlias, _) = await check(text: remoteText(pub: pub, alias: "alias=no", ssh: welcome), keys: keys)
        guard case .fail = noAlias.state else { return XCTFail("\(noAlias.state)") }
        let (noKey, runner) = await check(text: remoteText(pub: "", alias: "alias=no", ssh: "x"), keys: keys)
        guard case let .fail(why) = noKey.state else { return XCTFail("\(noKey.state)") }
        XCTAssertTrue(why.contains("no deploy key"), why)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    func testGhFailingIsCantTellNotFail() async {
        let runner = ScriptedRunner()
        runner.on({ $0.contains("repos/\(self.repo)/keys") }, result: CommandResult(code: 1, out: "", err: "gh: Not Found (HTTP 404)"))
        let (result, _) = await check(text: remoteText(pub: pub, ssh: welcome), keys: "", runner: runner)
        guard case .cantTell = result.state else { return XCTFail("\(result.state)") }
    }

    func testAPublicKeyThatIsNotOneEd25519LineIsRefused() async {
        let keys = ghKeys("ssh-ed25519 \(body)", readOnly: true)
        let cases = [
            "ssh-ed25519 AAAA$(touch x)",
            "\(pub)\nssh-ed25519 \(otherBody)",
            "ssh-rsa \(body) agents-m1",
            "ssh-ed25519 \(body) x$(id)",
            "ssh-ed25519 \(body) a;b",
            "ssh-ed25519 \(body)\nrm -rf x",
            // a key file that holds marker lines must not fake the later parts
            "ssh-ed25519 \(body)\n@@config\nalias=yes\n@@ssh\n\(welcome)",
        ]
        for text in cases {
            let runner = ScriptedRunner()
            let full = text.contains("@@config") ? "\(text)\n@@config\nalias=no\n@@ssh\nPermission denied" : remoteText(pub: text, ssh: welcome)
            let result = await probe(.deployKey(repo)).parse(RemoteOutput(text: full, code: 0), .test(runner: runner))
            guard case let .fail(why) = result.state else { return XCTFail("\(text): \(result.state)") }
            XCTAssertTrue(why.contains("unexpected public key"), "\(text): \(why)")
            XCTAssertTrue(runner.calls.isEmpty, "gh was called for: \(text)")
            XCTAssertFalse(why.contains("touch"), why)
        }
    }

    func testTheProbeOnlyReadsAndAsksGitHubWithSshT() {
        let script = probe(.deployKey(repo)).script
        XCTAssertTrue(script.contains("head -c 1024"))
        XCTAssertTrue(script.contains("ssh -T -o BatchMode=yes -o ConnectTimeout=8 \(alias) </dev/null"))
        XCTAssertTrue(script.contains("grep -Eq '\(RepoSteps.aliasPattern(alias))'"))
        XCTAssertFalse(script.contains("ssh-keygen"))
        XCTAssertFalse(script.contains(">>"))
        // the private key is never printed
        XCTAssertFalse(script.contains("cat "))
    }

    // MARK: Deploy key Fix

    private let ghMeta = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCj7ndNxQowgcQnjshcLrqPEiiphnt+VTTvDP6mHBL9j1aNUkY4Ue1gvwnGLVlOhGeYrnZaMgRK6\n"
    private func fixRunner(scanned: String? = nil, addCode: Int32 = 0, onGitHub: String = "[]") -> ScriptedRunner {
        let runner = ScriptedRunner()
        runner.on({ $0.contains("repos/\(self.repo)/keys") }, result: CommandResult(code: 0, out: onGitHub, err: ""))
        runner.on({ $0.last?.contains("ssh-keygen") == true }, result: CommandResult(code: 0, out: pub + "\n", err: ""))
        runner.on({ $0.contains("meta") }, result: CommandResult(code: 0, out: ghMeta + "ssh-ed25519 \(githubHostBody)\n", err: ""))
        runner.on({ $0.last?.contains("ssh-keyscan") == true },
                  result: CommandResult(code: 0, out: scanned ?? "github.com ssh-ed25519 \(githubHostBody)\n", err: ""))
        runner.on({ $0.contains("deploy-key") }, result: CommandResult(code: addCode, out: "", err: addCode == 0 ? "" : "boom"))
        return runner
    }

    func testDeployKeyFixOrder() async {
        let runner = fixRunner()
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertEqual(outcome.ok, true, outcome.message)
        let calls = runner.calls.map(\.argv)
        XCTAssertEqual(calls.count, 6)
        XCTAssertEqual(calls[0].first, "/usr/bin/ssh")
        XCTAssertTrue(unquoted(calls[0].last!).contains("ssh-keygen -q -t ed25519 -N ''"))
        XCTAssertTrue(unquoted(calls[0].last!).contains("-C 'agents-m1'"))
        XCTAssertTrue(calls[0].last!.contains("Host \(alias)"))
        XCTAssertTrue(calls[0].last!.contains("IdentityFile ~/.ssh/agents-dtthaison-lingeva-mono"))
        XCTAssertEqual(calls[1], [gh, "api", "meta", "--jq", ".ssh_keys[]"])
        XCTAssertEqual(calls[2].first, "/usr/bin/ssh")
        XCTAssertTrue(calls[2].last!.contains("ssh-keyscan -t ed25519 github.com 2>/dev/null"))
        XCTAssertEqual(calls[3].first, "/usr/bin/ssh")
        XCTAssertTrue(calls[3].last!.contains("known_hosts"))
        XCTAssertTrue(calls[3].last!.contains("github.com ssh-ed25519 \(githubHostBody)"))
        XCTAssertFalse(calls[3].last!.contains(otherBody))
        // a key GitHub does not list yet is added, after the list call
        XCTAssertEqual(calls[4], [gh, "api", "--paginate", "repos/\(repo)/keys"])
        XCTAssertEqual(Array(calls[5].prefix(4)), [gh, "repo", "deploy-key", "add"])
        XCTAssertTrue(calls[5][4].hasSuffix("m1-dtthaison-lingeva-mono.pub"), calls[5][4])
        XCTAssertEqual(Array(calls[5].suffix(4)), ["-R", repo, "--title", "agents-m1"])
        XCTAssertEqual(calls[5].count, 9)
    }

    func testAKeyAlreadyListedReadOnlyIsNotAddedAgain() async {
        let runner = fixRunner(onGitHub: ghKeys("ssh-ed25519 \(body)", readOnly: true))
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertEqual(outcome.ok, true, outcome.message)
        let calls = runner.calls.map(\.argv)
        XCTAssertFalse(calls.contains { $0.contains("deploy-key") })
        // the machine was still repaired first
        XCTAssertTrue(calls[0].last!.contains("ssh-keygen"))
        XCTAssertTrue(calls.contains { $0.last?.contains("known_hosts") == true })
    }

    func testAKeyAlreadyListedAsWritableStopsWithoutAdding() async {
        let runner = fixRunner(onGitHub: ghKeys("ssh-ed25519 \(body)", readOnly: false))
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("can write"), outcome.message)
        XCTAssertTrue(outcome.message.contains("Settings > Deploy keys"), outcome.message)
        XCTAssertFalse(runner.calls.map(\.argv).contains { $0.contains("deploy-key") })
    }

    func testAnotherKeyOnGitHubStillGetsOursAdded() async {
        let runner = fixRunner(onGitHub: ghKeys("ssh-ed25519 \(otherBody)", readOnly: true))
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertEqual(outcome.ok, true, outcome.message)
        XCTAssertTrue(runner.calls.map(\.argv).contains { $0.contains("deploy-key") })
    }

    func testAListThatCannotBeReadAddsNothing() async {
        let runner = fixRunner(onGitHub: "not json")
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertFalse(runner.calls.map(\.argv).contains { $0.contains("deploy-key") })
    }

    func testTheWriteFlagCanNeverAppear() async {
        let runner = fixRunner()
        _ = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        for argv in runner.calls.map(\.argv) {
            XCTAssertFalse(argv.contains { $0.contains("allow-write") }, "\(argv)")
            XCTAssertFalse(argv.contains("-w"), "\(argv)")
        }
        XCTAssertFalse(plan(.deployKey(repo)).display.contains("allow-write"))
        // and not in the source either
        let source = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Sources/Notchling/Machines/RepoSteps.swift")
        let text = try? String(contentsOf: source, encoding: .utf8)
        XCTAssertNotNil(text)
        XCTAssertFalse(text?.contains("allow-write") ?? true)
    }

    func testOnlyThePublicKeyIsEverReadFromTheMachine() async {
        let runner = fixRunner()
        _ = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        for argv in runner.calls.map(\.argv) where argv.first == "/usr/bin/ssh" {
            for line in unquoted(argv.last!).split(separator: "\n") where line.hasPrefix("cat ") {
                XCTAssertTrue(line.contains(".pub'"), String(line))
            }
        }
        XCTAssertTrue(runner.calls.allSatisfy { $0.stdin == nil })
    }

    func testAHostKeyMismatchStopsBeforeAnythingIsAdded() async {
        let runner = fixRunner(scanned: "github.com ssh-ed25519 \(otherBody)\n")
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("differs from gh api meta"), outcome.message)
        let calls = runner.calls.map(\.argv)
        XCTAssertFalse(calls.contains { $0.last?.contains("known_hosts") == true })
        XCTAssertFalse(calls.contains { $0.contains("deploy-key") })
    }

    func testAKeyscanThatSeesNothingAddsNothing() async {
        let runner = fixRunner(scanned: "")
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertFalse(runner.calls.map(\.argv).contains { $0.contains("deploy-key") })
    }

    func testAnUnexpectedKeyFromTheMachineIsNeverSentToGitHub() async {
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("ssh-keygen") == true }, result: CommandResult(code: 0, out: "ssh-rsa AAAA\nrm -rf /\n", err: ""))
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("unexpected public key"), outcome.message)
        XCTAssertEqual(runner.calls.count, 1)
    }

    func testAFailedAddIsReportedWithGhsFirstLine() async {
        let runner = fixRunner(addCode: 1)
        let outcome = await plan(.deployKey(repo)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("boom"), outcome.message)
    }

    func testAnUnsafeMachineNameRunsNothing() async {
        var machine = TestMachines.mac()
        machine.name = "m1; touch x"
        let runner = fixRunner()
        let outcome = await plan(.deployKey(repo), machine: machine).run(.test(.test(machine: machine, runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    func testAnUnsafeRepoNameGetsNoScripts() {
        let bad = MachineRepo(github: "a/b; touch x", localPath: "/x", checkCommand: "true")
        for step in RepoSteps.all(for: TestMachines.mac(), repo: bad) {
            if case .remote = step.probe { XCTFail("\(step.id)") }
            if case .run = step.fix { XCTFail("\(step.id)") }
        }
    }

    // MARK: Clone

    func testCloneUsesTheAliasAndRefusesAnotherCheckout() async {
        let id = StepID.clone(repo)
        let p = probe(id)
        XCTAssertTrue(p.script.contains("git -C \"$HOME\"/'src/lingeva-mono' remote get-url origin 2>/dev/null || echo none"), p.script)
        let ctx = StepContext.test()
        let none = await p.parse(RemoteOutput(text: "none\n", code: 0), ctx)
        XCTAssertEqual(none.state, .fail("not cloned"))
        let other = await p.parse(RemoteOutput(text: "git@github.com:someone/else.git\n", code: 0), ctx)
        guard case let .fail(why) = other.state else { return XCTFail("\(other.state)") }
        XCTAssertTrue(why.contains("git@github.com:someone/else.git"), why)
        let same = await p.parse(RemoteOutput(text: "git@\(alias):dtthaison/lingeva-mono.git\n", code: 0), ctx)
        XCTAssertEqual(same.state, .pass("~/src/lingeva-mono"))

        let fix = plan(id)
        XCTAssertTrue(fix.display.contains("git clone -- git@\(alias):dtthaison/lingeva-mono.git"))
        XCTAssertTrue(fix.display.contains("if [ -e \"$p\" ]; then echo \"$p already exists; Notchling won't touch it\" >&2; exit 1; fi"))
        XCTAssertFalse(fix.needsAdmin)

        let runner = ScriptedRunner()
        let outcome = await fix.run(.test(.test(runner: runner)))
        XCTAssertEqual(outcome.ok, true)
        XCTAssertEqual(runner.calls.count, 1)
        XCTAssertEqual(runner.calls[0].argv.first, "/usr/bin/ssh")
        XCTAssertTrue(runner.calls[0].argv.last!.contains("git clone -- git@\(alias):dtthaison/lingeva-mono.git"))
    }

    func testCloneRefusesAnExistingFolderWithTheMachinesMessage() async {
        let runner = ScriptedRunner()
        runner.on({ _ in true }, result: CommandResult(code: 1, out: "", err: "/home/agent/src/lingeva-mono already exists; Notchling won't touch it\n"))
        let outcome = await plan(.clone(repo)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("already exists"), outcome.message)
    }

    func testAMachinePathWithShellCharactersIsQuoted() {
        var machine = TestMachines.mac()
        var r = machine.repos[0]
        r.machinePath = "/srv/a b'; touch x; '"
        machine.sdWorker?.repos = [r]
        let step = RepoSteps.all(for: machine, repo: r).first { $0.id == .clone(repo) }!
        guard case let .remote(script, _) = step.probe else { return XCTFail() }
        XCTAssertTrue(script.contains(LaneStore.quote("/srv/a b'; touch x; '")), script)
    }

    // MARK: Push is refused

    private let readOnly = "ERROR: The key you are authenticating with has been marked as read only.\nfatal: Could not read from remote repository."

    /// What the probe prints: git's output, its exit code on a marker line, then one token line per login check.
    private func pushOutput(git: String, code: Int = 128, checks: [String] = ["gh=1", "ssh=denied", "keychain=44", "credfile=1"]) -> RemoteOutput {
        RemoteOutput(text: ([git, "@@push \(code)"] + checks).joined(separator: "\n") + "\n", code: 0)
    }

    func testPushRefusedNeedsGitHubsReadOnlyMessage() async {
        let id = StepID.pushRefused(repo)
        let p = probe(id)
        XCTAssertTrue(p.script.contains("push --dry-run origin HEAD:refs/heads/agents-push-test"))
        let ctx = StepContext.test()
        let refused = await p.parse(pushOutput(git: readOnly), ctx)
        XCTAssertEqual(refused.state, .pass("GitHub refuses a push with this key; no other GitHub login on this account"))
        let accepted = await p.parse(pushOutput(git: "To github:x\n * [new branch] HEAD -> agents-push-test", code: 0), ctx)
        guard case let .fail(why) = accepted.state else { return XCTFail("\(accepted.state)") }
        XCTAssertTrue(why.contains("can push"), why)
        let other = await p.parse(pushOutput(git: "fatal: not a git repository\nmore", code: 128), ctx)
        XCTAssertEqual(other.state, .fail("fatal: not a git repository"))
        // an output without the marker (the script was cut) is a failure with its first line, not a pass
        let cut = await p.parse(RemoteOutput(text: "fatal: something\n", code: 1), ctx)
        XCTAssertEqual(cut.state, .fail("fatal: something"))
    }

    /// I5: the worker's own GitHub logins. Each check's output stays on the machine; only a token line comes back.
    func testPushRefusedFailsWhenTheWorkerHasItsOwnGitHubLogin() async {
        let p = probe(.pushRefused(repo))
        let ctx = StepContext.test()
        let cases: [(check: String, why: String)] = [
            ("gh=0", "gh is logged in as the worker; run gh auth logout"),
            ("ssh=hi:sondoan", "a default ssh key logs in to GitHub as sondoan"),
            ("keychain=0", "the keychain holds a github.com password"),
            ("credfile=0", "~/.git-credentials holds a github.com login"),
        ]
        for (check, why) in cases {
            let name = String(check.prefix { $0 != "=" })
            var checks = ["gh=1", "ssh=denied", "keychain=44", "credfile=1"]
            checks[checks.firstIndex { $0.hasPrefix(name + "=") }!] = check
            let result = await p.parse(pushOutput(git: readOnly, checks: checks), ctx)
            XCTAssertEqual(result.state, .fail(why), check)
            XCTAssertTrue(result.rows.contains { !$0.ok }, "the failing check has a row: \(check)")
        }
        // two at once: both reasons
        let both = await p.parse(pushOutput(git: readOnly, checks: ["gh=0", "ssh=denied", "keychain=0", "credfile=1"]), ctx)
        XCTAssertEqual(both.state, .fail("gh is logged in as the worker; run gh auth logout; the keychain holds a github.com password"))
    }

    func testAnSshKeyThatAnswersForARepoOrNotAtAllIsNotALogin() async {
        let p = probe(.pushRefused(repo))
        let ctx = StepContext.test()
        // a deploy key answers "Hi owner/repo!": it is repo-scoped, and a writable one is caught by the dry run
        let deploy = await p.parse(pushOutput(git: readOnly, checks: ["gh=1", "ssh=hi:dtthaison/lingeva-mono", "keychain=44", "credfile=1"]), ctx)
        guard case .pass = deploy.state else { return XCTFail("\(deploy.state)") }
        // no GitHub answer at all (offline, host key unknown): can't tell, and not a pass
        let unknown = await p.parse(pushOutput(git: readOnly, checks: ["gh=1", "ssh=unknown", "keychain=44", "credfile=1"]), ctx)
        guard case .cantTell = unknown.state else { return XCTFail("\(unknown.state)") }
        // a keychain that can't be read is not "no password"
        let locked = await p.parse(pushOutput(git: readOnly, checks: ["gh=1", "ssh=denied", "keychain=36", "credfile=1"]), ctx)
        guard case .cantTell = locked.state else { return XCTFail("\(locked.state)") }
        // gh not installed: nothing to log out of
        let noGH = await p.parse(pushOutput(git: readOnly, checks: ["gh=none", "ssh=denied", "keychain=44", "credfile=1"]), ctx)
        guard case .pass = noGH.state else { return XCTFail("\(noGH.state)") }
        // a name that is not a GitHub login is not shown
        let odd = await p.parse(pushOutput(git: readOnly, checks: ["gh=1", "ssh=hi:$(touch x)", "keychain=44", "credfile=1"]), ctx)
        XCTAssertEqual(odd.state, .fail("a default ssh key logs in to GitHub"))
        XCTAssertFalse("\(odd.state) \(odd.rows)".contains("touch"))
    }

    /// The login checks read exit codes and a name only: no `-g`/`-w` on the keychain, no output kept, nothing writes.
    func testTheLoginChecksKeepNoOutputAndWriteNothing() {
        for machine in [TestMachines.mac(), TestMachines.linux()] {
            let script = probe(.pushRefused(repo), machine: machine).script
            XCTAssertTrue(script.contains("gh auth status --hostname github.com >/dev/null 2>&1"), script)
            // a user's ssh config with StrictHostKeyChecking accept-new would let this check add a known_hosts entry
            XCTAssertTrue(script.contains("ssh -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=yes -T git@github.com"), script)
            XCTAssertTrue(script.contains("grep -q 'github.com' ~/.git-credentials 2>/dev/null"), script)
            XCTAssertFalse(script.contains("gh auth token"), "the token is never printed")
            XCTAssertFalse(script.contains("gh auth login") || script.contains("gh auth logout") || script.contains("gh auth setup-git"))
            XCTAssertFalse(script.contains("credential fill") || script.contains("credential approve"))
            XCTAssertFalse(script.contains(" -g") || script.contains(" -w"), "no keychain secret is asked for")
            XCTAssertFalse(script.contains("accept-new") || script.contains("StrictHostKeyChecking=no"), "a check must not write known_hosts")
            XCTAssertEqual(script.components(separatedBy: " push ").count, 2, "the only push is the dry run")
        }
        XCTAssertTrue(probe(.pushRefused(repo), machine: TestMachines.mac()).script.contains("security find-internet-password -s github.com >/dev/null 2>&1"))
        XCTAssertFalse(probe(.pushRefused(repo), machine: TestMachines.linux()).script.contains("security "), "no keychain on Linux")
    }

    /// Run in a real shell with stubs for gh, ssh and security: only token lines come back, never what they printed.
    func testTheLoginChecksRunToTokenLinesAndNeverPrintTheToolsOutput() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-login-\(UUID().uuidString)", isDirectory: true)
        let bin = dir.appendingPathComponent("bin", isDirectory: true), home = dir.appendingPathComponent("home", isDirectory: true)
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        func stub(_ name: String, _ body: String) throws {
            let url = bin.appendingPathComponent(name)
            try ("#!/bin/sh\n" + body + "\n").write(to: url, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        }
        try stub("gh", "echo 'Logged in to github.com account sondoan (token gho_SECRETSECRET)'; exit 0")
        try stub("ssh", "echo 'Hi sondoan! You have successfully authenticated, but GitHub does not provide shell access.' >&2; exit 1")
        try stub("security", "echo 'keychain: \"srvr\"<blob>=\"github.com\" password: SECRETSECRET'; exit 0")
        try stub("git", "echo 'remote: ERROR: The key you are authenticating with has been marked as read only.' >&2; exit 128")
        try "https://sondoan:SECRETSECRET@github.com\n".write(to: home.appendingPathComponent(".git-credentials"), atomically: true, encoding: .utf8)
        let script = probe(.pushRefused(repo), machine: TestMachines.mac()).script
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = ["-c", script]
        process.environment = ["PATH": "\(bin.path):/usr/bin:/bin", "HOME": home.path]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        try process.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        let out = String(decoding: data, as: UTF8.self)
        XCTAssertFalse(out.contains("SECRETSECRET"), out)
        XCTAssertFalse(out.contains("gho_"), out)
        XCTAssertFalse(out.contains("blob"), out)
        for token in ["@@push 128", "gh=0", "ssh=hi:sondoan", "keychain=0", "credfile=0"] {
            XCTAssertTrue(out.contains(token), "\(token) in:\n\(out)")
        }
    }

    func testPushRefusedNeverSendsAPush() {
        // the only push in any script is a dry run, and the how-to adds no command
        let step = steps().first { $0.id == .pushRefused(repo) }!
        guard case let .remote(script, _) = step.probe else { return XCTFail() }
        XCTAssertEqual(script.components(separatedBy: " push ").count, 2)
        XCTAssertTrue(script.contains("push --dry-run"))
        // a checkout's own pre-push hook must not run, and the push reads no stdin
        XCTAssertTrue(script.contains("git -c core.hooksPath=/dev/null -C"))
        XCTAssertTrue(script.contains("</dev/null 2>&1"))
        guard case let .howTo(text) = step.fix else { return XCTFail() }
        XCTAssertTrue(text.contains("Settings > Deploy keys"))
        XCTAssertEqual(step.cadence == .slow, true)
    }

    /// A line of machine or gh output is masked before it is cut to 200 characters: the cut falls inside the token.
    func testMachineTextIsMaskedBeforeItIsCutInChecksAndFixes() async {
        let token = testLongToken
        let mask = testMask(token, as: "[secret:m1-github-issues]")
        let said = String(repeating: "f", count: 170) + token
        let ctx = StepContext.test(mask: mask)

        // the push check: the first line of what git said
        let push = await probe(.pushRefused(repo)).parse(RemoteOutput(text: said + "\n", code: 128), ctx)
        assertNoFragment(of: token, in: "\(push.state)")
        XCTAssertTrue("\(push.state)".contains("[secret:m1-github-issues]"), "\(push.state)")

        // the clone check: the origin line
        let clone = await probe(.clone(repo)).parse(RemoteOutput(text: said + "\n", code: 0), ctx)
        assertNoFragment(of: token, in: "\(clone.state)")

        // the deploy key check: gh's error when it can't list the keys, and what ssh said to GitHub
        let runner = ScriptedRunner()
        runner.on({ $0.contains("repos/\(self.repo)/keys") }, result: CommandResult(code: 1, out: "", err: said))
        let listed = await probe(.deployKey(repo)).parse(RemoteOutput(text: remoteText(pub: pub, ssh: welcome), code: 0),
                                                         .test(runner: runner, mask: mask))
        assertNoFragment(of: token, in: "\(listed.state)")
        let ssh = await probe(.deployKey(repo)).parse(RemoteOutput(text: remoteText(pub: pub, ssh: said), code: 0), {
            let ok = ScriptedRunner()
            ok.on({ $0.contains("repos/\(self.repo)/keys") }, result: CommandResult(code: 0, out: self.ghKeys(self.body, readOnly: true), err: ""))
            return .test(runner: ok, mask: mask)
        }())
        assertNoFragment(of: token, in: "\(ssh.state) \(ssh.rows)")

        // the Fixes: clone's failure line and gh's, each cut to 200 characters
        let failing = ScriptedRunner()
        failing.on({ _ in true }, result: CommandResult(code: 128, out: "", err: said))
        let cloned = await plan(.clone(repo)).run(.test(.test(runner: failing, mask: mask)))
        XCTAssertFalse(cloned.ok)
        assertNoFragment(of: token, in: cloned.message)
        XCTAssertTrue(cloned.message.contains("[secret:m1-github-issues]"), cloned.message)
        let key = await plan(.deployKey(repo)).run(.test(.test(runner: failing, mask: mask)))
        assertNoFragment(of: token, in: key.message)
    }

    func testStepsKeepTheirOrderAndDependencies() {
        let all = steps()
        XCTAssertEqual(all.map(\.id), [.deployKey(repo), .clone(repo), .pushRefused(repo)])
        XCTAssertEqual(all[0].needs, [.reachable, .baseTools])
        XCTAssertEqual(all[1].needs, [.deployKey(repo)])
        XCTAssertEqual(all[2].needs, [.clone(repo)])
    }

    // MARK: Replace key

    private let newBody = "AAAAC3NzaC1lZDI1NTE5AAAAINewKeyForReplaceTestsOnly0123456789abcdefgh"
    private var newPub: String { "ssh-ed25519 \(newBody) agents-m1-20270115" }
    private let newTitle = "agents-m1-20270115"

    private func replacePlan(machine: Machine = TestMachines.mac()) -> FixPlan {
        let step = steps(machine: machine).first { $0.id == .deployKey(repo) }!
        guard let plan = step.replace else { fatalError("no Replace key plan") }
        return plan
    }

    private func keyJSON(id: String, body: String, title: String, readOnly: Bool) -> String {
        let json = title.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
            .replacingOccurrences(of: "\n", with: "\\n")
        return "{\"id\":\(id),\"key\":\"ssh-ed25519 \(body)\",\"title\":\"\(json)\",\"read_only\":\(readOnly)}"
    }

    /// A machine and GitHub for Replace key: the old key is id 11 on GitHub, the new one becomes id 22.
    private func replaceRunner(sshOK: Bool = true, pushAccepted: Bool = false, newReadOnly: Bool? = true,
                               oldID: String? = "11", deleteOldCode: Int32 = 0, deleteNewCode: Int32 = 0,
                               addCode: Int32 = 0, keygenOut: String? = nil, oldTitle: String = "agents-m1",
                               oldPubText: String? = nil, extraListed: [String] = [], newListedBeforeAdd: Bool = false,
                               newTitleOnGitHub: String? = nil, swapCode: Int32 = 0) -> ScriptedRunner {
        let runner = ScriptedRunner()
        var before: [String] = extraListed
        if let oldID { before.append(keyJSON(id: oldID, body: body, title: oldTitle, readOnly: true)) }
        var after = before
        if let newReadOnly { after.append(keyJSON(id: "22", body: newBody, title: newTitleOnGitHub ?? newTitle, readOnly: newReadOnly)) }
        if newListedBeforeAdd { before = after }
        // the first listing is the one before the add; every later one is after it
        let listCalls = ListCalls()
        let isList: ([String]) -> Bool = { $0.contains("repos/\(self.repo)/keys") }
        runner.on({ argv in isList(argv) && listCalls.next() == 1 }, result: CommandResult(code: 0, out: "[" + before.joined(separator: ",") + "]", err: ""))
        runner.on(isList, result: CommandResult(code: 0, out: "[" + after.joined(separator: ",") + "]", err: ""))
        runner.on({ $0.last?.contains("mv -f") == true }, result: CommandResult(code: swapCode, out: "", err: ""))
        runner.on({ $0.first == self.gh && $0.contains("delete") && $0.contains("11") },
                  result: CommandResult(code: deleteOldCode, out: "", err: deleteOldCode == 0 ? "" : "delete boom"))
        runner.on({ $0.first == self.gh && $0.contains("delete") && $0.contains("22") },
                  result: CommandResult(code: deleteNewCode, out: "", err: deleteNewCode == 0 ? "" : "delete new boom"))
        runner.on({ $0.first == self.gh && $0.contains("add") }, result: CommandResult(code: addCode, out: "", err: addCode == 0 ? "" : "add boom"))
        runner.on({ $0.last?.contains("ssh-keygen") == true }, result: CommandResult(code: 0, out: keygenOut ?? newPub + "\n", err: ""))
        runner.on({ $0.last?.contains("head -c 1024") == true }, result: CommandResult(code: 0, out: (oldPubText ?? pub) + "\n", err: ""))
        runner.on({ $0.last?.contains("-T git@github.com") == true },
                  result: CommandResult(code: 0, out: sshOK ? welcome + "\n" : "git@github.com: Permission denied (publickey).\n", err: ""))
        runner.on({ $0.last?.contains("push --dry-run") == true },
                  result: CommandResult(code: 0, out: pushAccepted ? "To github.com:\(repo).git\n * [new branch] HEAD -> agents-push-test\n@@push 0\n"
                                        : readOnly + "\n@@push 128\n", err: ""))
        return runner
    }

    /// What each call was, in order: `gh list`, `gh add`, `gh delete <id>`, or the kind of script sent to the machine.
    private func kinds(_ runner: ScriptedRunner) -> [String] {
        runner.calls.map { call in
            let argv = call.argv
            if argv.first == gh {
                if argv.contains("add") { return "gh add" }
                if argv.contains("delete") { return "gh delete \(argv[argv.firstIndex(of: "delete")! + 1])" }
                return "gh list"
            }
            let script = unquoted(argv.last ?? "")
            if script.contains("mv -f") { return "swap" }
            if script.contains("ssh-keygen") { return "keygen" }
            if script.contains("head -c 1024") { return "read old" }
            if script.contains("-T git@github.com") { return "ssh -T" }
            if script.contains("push --dry-run") { return "push" }
            if script.contains("rm -f") { return "rm" }
            return "other"
        }
    }

    private func run(_ plan: FixPlan, _ runner: ScriptedRunner) async -> FixOutcome {
        await plan.run(.test(.test(runner: runner)))
    }

    func testReplaceKeyHappyPathRunsInOrderAndDeletesOnlyTheOldKeyLast() async {
        let runner = replaceRunner()
        let outcome = await run(replacePlan(), runner)
        XCTAssertEqual(outcome.ok, true, outcome.message)
        XCTAssertEqual(kinds(runner), ["read old", "keygen", "gh list", "gh add", "ssh -T", "push", "gh list", "swap", "gh delete 11"])
        let calls = runner.calls.map(\.argv)
        let keygen = unquoted(calls[1].last!)
        XCTAssertTrue(keygen.contains("ssh-keygen -q -t ed25519 -N '' -C 'agents-m1-20270115' -f"), keygen)
        XCTAssertTrue(keygen.contains(".ssh/agents-dtthaison-lingeva-mono.new'"), keygen)
        XCTAssertTrue(keygen.contains("cat \"$HOME\"/'.ssh/agents-dtthaison-lingeva-mono.new.pub'"), keygen)
        XCTAssertEqual(calls[2], [gh, "api", "--paginate", "repos/\(repo)/keys"], "the keys are listed before the add")
        XCTAssertEqual(Array(calls[3].prefix(4)), [gh, "repo", "deploy-key", "add"])
        XCTAssertEqual(Array(calls[3].suffix(4)), ["-R", repo, "--title", newTitle])
        XCTAssertEqual(calls[3].count, 9, "no flag besides -R and --title")
        let ssh = unquoted(calls[4].last!)
        XCTAssertTrue(ssh.contains(".ssh/agents-dtthaison-lingeva-mono.new'"), ssh)
        XCTAssertTrue(ssh.contains("IdentitiesOnly=yes") && ssh.contains("BatchMode=yes") && ssh.contains("-F /dev/null"), ssh)
        XCTAssertTrue(ssh.contains("StrictHostKeyChecking=yes"), ssh)
        XCTAssertTrue(ssh.contains("</dev/null"), ssh)
        let push = unquoted(calls[5].last!)
        XCTAssertTrue(push.contains("core.hooksPath=/dev/null") && push.contains("push --dry-run") && push.contains("</dev/null"), push)
        XCTAssertTrue(push.contains("GIT_TERMINAL_PROMPT=0") && push.contains("IdentitiesOnly=yes") && push.contains(".new"), push)
        XCTAssertTrue(push.contains("StrictHostKeyChecking=yes") && push.contains("-i '$k'"), push)
        XCTAssertEqual(calls[6], [gh, "api", "--paginate", "repos/\(repo)/keys"])
        let swap = unquoted(calls[7].last!)
        XCTAssertTrue(swap.contains(".new.pub") && swap.contains("mv -f"), swap)
        XCTAssertFalse(swap.contains(".ssh/config"), "the alias file name is the same, so ssh config is not touched")
        XCTAssertEqual(calls[8], [gh, "repo", "deploy-key", "delete", "11", "-R", repo])
        XCTAssertTrue(runner.calls.allSatisfy { $0.stdin == nil })
        XCTAssertTrue(outcome.message.contains("old key was deleted"), outcome.message)
    }

    func testReplaceKeyNeverUsesTheWriteFlagAndTheDisplayShowsTheCommands() async {
        let runner = replaceRunner()
        _ = await run(replacePlan(), runner)
        for argv in runner.calls.map(\.argv) {
            XCTAssertFalse(argv.contains { $0.contains("allow-write") }, "\(argv)")
            XCTAssertFalse(argv.contains("-w"), "\(argv)")
        }
        let display = replacePlan().display
        XCTAssertFalse(display.contains("allow-write"))
        XCTAssertTrue(display.contains("ssh-keygen"))
        XCTAssertTrue(display.contains("deploy-key add"))
        XCTAssertTrue(display.contains("deploy-key delete"))
        XCTAssertTrue(display.contains("push --dry-run"))
        XCTAssertTrue(display.contains(".new"))
        XCTAssertFalse(replacePlan().needsAdmin)
        // every GitHub call is shown, the cleanup's too, and the date is said to be UTC
        XCTAssertTrue(display.contains("deploy-key delete <id of the new key>"), display)
        XCTAssertTrue(display.contains("deploy-key delete <id of the old key>"), display)
        XCTAssertTrue(display.components(separatedBy: "\(gh) api --paginate repos/\(repo)/keys").count >= 4, "the list before the add, the check, the cleanup")
        XCTAssertTrue(display.contains("today's date in UTC"), display)
    }

    private func assertCleanedUp(_ runner: ScriptedRunner, file: StaticString = #filePath, line: UInt = #line) {
        let k = kinds(runner)
        XCTAssertFalse(k.contains("swap"), "\(k)", file: file, line: line)
        XCTAssertFalse(k.contains("gh delete 11"), "the old key stays on GitHub: \(k)", file: file, line: line)
        XCTAssertTrue(k.contains("rm"), "the .new files are removed: \(k)", file: file, line: line)
        XCTAssertTrue(k.contains("gh delete 22"), "the new key is deleted from GitHub: \(k)", file: file, line: line)
        let rm = runner.calls.map(\.argv).first { unquoted($0.last ?? "").contains("rm -f") && !unquoted($0.last ?? "").contains("ssh-keygen") }
        let text = unquoted(rm?.last ?? "")
        XCTAssertTrue(text.contains(".new'") && text.contains(".new.pub'"), text, file: file, line: line)
    }

    func testReplaceKeyWhoseNewKeyCanPushIsUndoneAndSaysWhy() async {
        let runner = replaceRunner(pushAccepted: true)
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("can write"), outcome.message)
        assertCleanedUp(runner)
        XCTAssertTrue(outcome.message.contains("old key"), outcome.message)
    }

    func testReplaceKeyWhoseNewKeyCannotAuthenticateIsUndone() async {
        let runner = replaceRunner(sshOK: false)
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("ssh"), outcome.message)
        XCTAssertFalse(kinds(runner).contains("push"), "the push is not tried when ssh failed")
        assertCleanedUp(runner)
    }

    func testReplaceKeyWhoseNewKeyGitHubListsAsWritableIsUndone() async {
        let runner = replaceRunner(newReadOnly: false)
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("can write"), outcome.message)
        assertCleanedUp(runner)
    }

    func testReplaceKeyWhoseAddFailsLooksForTheNewKeyAndStopsBeforeAnyCheck() async {
        let runner = replaceRunner(newReadOnly: nil, addCode: 1)
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("add boom"), outcome.message)
        let k = kinds(runner)
        XCTAssertFalse(k.contains("ssh -T") || k.contains("swap") || k.contains("gh delete 11"), "\(k)")
        XCTAssertTrue(k.contains("rm"), "\(k)")
    }

    func testReplaceKeyCleanupThatCannotDeleteTheNewKeyTellsWhereToRemoveIt() async {
        let runner = replaceRunner(pushAccepted: true, deleteNewCode: 1)
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("Settings → Deploy keys"), outcome.message)
        XCTAssertTrue(outcome.message.contains(newTitle), outcome.message)
        XCTAssertFalse(kinds(runner).contains("gh delete 11"))
    }

    func testReplaceKeyWhoseKeygenGivesSomethingElseStopsAndCleansUp() async {
        let runner = replaceRunner(keygenOut: "ssh-rsa AAAA\nrm -rf /\n")
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("unexpected public key"), outcome.message)
        XCTAssertEqual(kinds(runner), ["read old", "keygen", "rm"])
    }

    func testReplaceKeyDeleteOfTheOldKeyFailingAfterTheSwapIsNotASilentPass() async {
        let runner = replaceRunner(deleteOldCode: 1)
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("new key in use"), outcome.message)
        XCTAssertTrue(outcome.message.contains("old key is still on GitHub"), outcome.message)
        XCTAssertTrue(outcome.message.contains("Settings → Deploy keys"), outcome.message)
        XCTAssertEqual(Array(kinds(runner).suffix(2)), ["swap", "gh delete 11"])
        XCTAssertFalse(kinds(runner).contains("rm"), "the swap happened; nothing is rolled back")
    }

    func testReplaceKeyWithAnOldIdThatIsNotAnIntegerDeletesNothing() async {
        for bad in ["\"11; touch x\"", "\"abc\"", "\"11\"", "1.5", "true", "null", "-3", "0"] {
            let runner = replaceRunner(oldID: bad)
            let outcome = await run(replacePlan(), runner)
            XCTAssertFalse(outcome.ok, bad)
            XCTAssertTrue(outcome.message.contains("not a number"), "\(bad): \(outcome.message)")
            let k = kinds(runner)
            XCTAssertFalse(k.contains { $0.hasPrefix("gh delete") && $0 != "gh delete 22" }, "\(bad): \(k)")
            XCTAssertFalse(k.contains("swap"), "\(bad): nothing was replaced: \(k)")
            for argv in runner.calls.map(\.argv) { XCTAssertFalse(argv.contains { $0.contains("touch") }, "\(argv)") }
        }
    }

    func testReplaceKeyWhenGitHubDoesNotListTheOldKeyDeletesNothingAndSaysSo() async {
        let runner = replaceRunner(oldID: nil)
        let outcome = await run(replacePlan(), runner)
        XCTAssertEqual(outcome.ok, true, outcome.message)
        XCTAssertTrue(outcome.message.contains("not listed"), outcome.message)
        let k = kinds(runner)
        XCTAssertTrue(k.contains("swap"))
        XCTAssertFalse(k.contains { $0.hasPrefix("gh delete") }, "\(k)")
    }

    func testReplaceKeyMatchesTheOldKeyByItsKeyTextNotByTitle() async {
        // another key with the old key's title and a lower id must not be deleted
        let decoy = keyJSON(id: "5", body: otherBody, title: "agents-m1", readOnly: true)
        let runner = replaceRunner(extraListed: [decoy])
        let outcome = await run(replacePlan(), runner)
        XCTAssertEqual(outcome.ok, true, outcome.message)
        XCTAssertEqual(runner.calls.last?.argv, [gh, "repo", "deploy-key", "delete", "11", "-R", repo])
        XCTAssertFalse(runner.calls.map(\.argv).contains { $0.contains("delete") && $0.contains("5") })
    }

    // MARK: Replace key: a delete only removes a key of this run or of this machine (I1)

    private let otherPub = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOtherKeyThatIsNotGitHubs0123456789abcdef agents-m2"

    func testAHostileKeygenThatReturnsAListedKeyIsRefusedAndNothingIsDeleted() async {
        // the "new" key is m2's, already on the repo under m2's title
        let m2 = keyJSON(id: "5", body: otherBody, title: "agents-m2", readOnly: true)
        let runner = replaceRunner(newReadOnly: nil, keygenOut: otherPub + "\n", extraListed: [m2])
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("the new key is already on GitHub; nothing changed"), outcome.message)
        XCTAssertEqual(kinds(runner), ["read old", "keygen", "gh list", "rm"], "no add, no GitHub delete, the .new files removed")
    }

    func testAnOldPubThatHoldsAnotherMachinesKeyIsNeverDeleted() async {
        let m2 = keyJSON(id: "5", body: otherBody, title: "agents-m2", readOnly: true)
        let runner = replaceRunner(oldID: nil, oldPubText: otherPub, extraListed: [m2])
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(kinds(runner).contains { $0.hasPrefix("gh delete") }, "\(kinds(runner))")
        XCTAssertTrue(kinds(runner).contains("swap"), "the new key is in use")
        XCTAssertFalse(outcome.ok, "the old key is left on GitHub: not a silent pass")
        // it names the title found, says nothing was deleted, and stays red
        XCTAssertTrue(outcome.message.contains("replaced the key on m1; the old key on GitHub is titled 'agents-m2', not agents-m1, so nothing was deleted; remove it in Settings → Deploy keys if it's m1's"), outcome.message)
    }

    func testTheTitleFoundOnGitHubIsMaskedAndCutBeforeItIsShown() async {
        let token = testLongToken
        let title = "agents-x" + String(repeating: "f", count: 170) + token
        let runner = replaceRunner(oldTitle: title)
        let outcome = await replacePlan().run(.test(.test(runner: runner, mask: testMask(token, as: "[secret:m1-github-issues]"))))
        XCTAssertFalse(outcome.ok)
        assertNoFragment(of: token, in: outcome.message)
        XCTAssertTrue(outcome.message.contains("titled 'agents-x"), outcome.message)
        XCTAssertFalse(kinds(runner).contains { $0.hasPrefix("gh delete") })
    }

    func testTheOldKeyIsDeletedOnlyUnderThisMachinesOwnTitles() async {
        for (title, deleted) in [("agents-m1", true), ("agents-m1-20261001", true), ("agents-m2", false), ("agents-m1-2026", false),
                                 ("agents-m10", false), ("agents-m1-20261001x", false), ("agents-m1-", false), ("", false),
                                 ("Agents-m1", false), ("deploy", false), ("xagents-m1", false), ("agents-m1\n", false),
                                 ("agents-m1-20261001\n", false), ("agents-m1\n-20261001", false), ("agents-m1-2026100\u{0661}", false),
                                 ("agents-m1-20261001-20261001", false), ("agents-m1 ", false)] {
            let runner = replaceRunner(oldTitle: title)
            let outcome = await run(replacePlan(), runner)
            XCTAssertEqual(kinds(runner).contains("gh delete 11"), deleted, "title '\(title)': \(kinds(runner))")
            XCTAssertEqual(outcome.ok, deleted, "title '\(title)': \(outcome.message)")
            XCTAssertTrue(kinds(runner).contains("swap"), title)
        }
    }

    func testCleanupDeletesOnlyAKeyOfThisRunUnderThisRunsTitle() async {
        // ssh fails after the add; GitHub now lists a key with the new text but another title: not ours to delete
        let other = replaceRunner(sshOK: false, newTitleOnGitHub: "somebody-elses-title")
        let outcome = await run(replacePlan(), other)
        XCTAssertFalse(outcome.ok)
        XCTAssertFalse(kinds(other).contains { $0.hasPrefix("gh delete") }, "\(kinds(other))")
        XCTAssertTrue(kinds(other).contains("rm"))
        XCTAssertTrue(outcome.message.contains("nothing was deleted"), outcome.message)
        XCTAssertTrue(outcome.message.contains("Settings → Deploy keys"), outcome.message)
    }

    func testCleanupAfterAFailedAddDeletesNothingThatWasAlreadyListed() async {
        // The add fails, and the listing after it holds a key with the new key's text that this run did not add: it
        // has another id and another title (someone else's, put there meanwhile). The cleanup used to delete any
        // key with that text; it must delete nothing and say so. m2's key, listed all along, stays too.
        let m2 = keyJSON(id: "5", body: otherBody, title: "agents-m2", readOnly: true)
        let runner = replaceRunner(newReadOnly: true, addCode: 1, extraListed: [m2], newTitleOnGitHub: "agents-m2-someone-elses")
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("add boom"), outcome.message)
        XCTAssertTrue(outcome.message.contains("nothing was deleted"), outcome.message)
        XCTAssertFalse(kinds(runner).contains { $0.hasPrefix("gh delete") }, "\(kinds(runner))")
        XCTAssertTrue(kinds(runner).contains("rm"))
    }

    func testListingFailingBeforeTheAddStopsBeforeTheAdd() async {
        let broken = ScriptedRunner()
        broken.on({ $0.last?.contains("head -c 1024") == true }, result: CommandResult(code: 0, out: pub + "\n", err: ""))
        broken.on({ $0.last?.contains("ssh-keygen") == true }, result: CommandResult(code: 0, out: newPub + "\n", err: ""))
        broken.on({ $0.contains("--paginate") }, result: CommandResult(code: 1, out: "", err: "list boom"))
        let outcome = await run(replacePlan(), broken)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("list boom"), outcome.message)
        XCTAssertFalse(kinds(broken).contains("gh add"))
        XCTAssertFalse(kinds(broken).contains { $0.hasPrefix("gh delete") })
    }

    func testKeyTextIsNormalisedToTypeAndBase64OnAOneLineInput() {
        XCTAssertEqual(RepoSteps.normalisedKey("ssh-ed25519 \(body) agents-m1"), "ssh-ed25519 \(body)")
        XCTAssertEqual(RepoSteps.normalisedKey("ssh-ed25519 \(body) other comment here\n"), "ssh-ed25519 \(body)")
        XCTAssertEqual(RepoSteps.normalisedKey("ssh-ed25519 \(body)"), "ssh-ed25519 \(body)")
        for bad in ["ssh-ed25519 \(body)\nssh-ed25519 \(otherBody)", "ssh-ed25519 \(body) x\ny", "ssh-ed25519 \(body)\r\nz", "ssh-rsa \(body)",
                    "ssh-ed25519", "ssh-ed25519 !!!notbase64!!!", "", "\n"] {
            XCTAssertNil(RepoSteps.normalisedKey(bad), bad)
        }
    }

    func testAMultiLineKeyFieldFromGitHubNeverMatches() async {
        // a listed "key" that holds two lines, the first one the new key plus a comment: not a match. (The comment's
        // space is what a first-two-fields comparison would have matched on.)
        let hostile = "{\"id\":7,\"key\":\"ssh-ed25519 \(newBody) x\\nssh-ed25519 \(otherBody)\",\"title\":\"x\",\"read_only\":true}"
        let runner = replaceRunner(extraListed: [hostile])
        let outcome = await run(replacePlan(), runner)
        XCTAssertEqual(outcome.ok, true, outcome.message)
    }

    // MARK: Replace key: an ambiguous swap deletes nothing (I2)

    func testASwapThatReturns255IsCantTellAndDeletesNothing() async {
        // 1, 2 and 3 are what a signal looks like to the runner (SIGHUP, SIGINT, SIGQUIT): they prove nothing either
        for code in [255, 124, 125, 137, 4, 15, 9, 1, 2, 3, 40, 44] as [Int32] {
            let runner = replaceRunner(swapCode: code)
            let outcome = await run(replacePlan(), runner)
            XCTAssertFalse(outcome.ok, "\(code)")
            XCTAssertTrue(outcome.message.contains("can't tell whether the key was replaced on m1"), outcome.message)
            XCTAssertTrue(outcome.message.contains("the swap may have happened; Check again before anything else"), outcome.message)
            XCTAssertTrue(outcome.message.contains("Nothing was removed from GitHub"), outcome.message)
            XCTAssertFalse(outcome.message.contains("untouched"), "never claims what it didn't prove: \(outcome.message)")
            let k = kinds(runner)
            XCTAssertEqual(k.last, "swap", "\(code): nothing after the swap, no delete of either key: \(k)")
            XCTAssertFalse(k.contains("rm"), "\(code): the machine's files are left as they are")
        }
    }

    func testASwapThatFailsWithItsOwnCodesIsUndoneAsBefore() async {
        // the swap script's own failure codes: 41 the private key would not move, 43 a file was missing
        for code in [41, 43] as [Int32] {
            let runner = replaceRunner(swapCode: code)
            let outcome = await run(replacePlan(), runner)
            XCTAssertFalse(outcome.ok, "\(code)")
            let k = kinds(runner)
            XCTAssertTrue(k.contains("rm"), "\(code): \(k)")
            XCTAssertTrue(k.contains("gh delete 22"), "\(code): \(k)")
            XCTAssertFalse(k.contains("gh delete 11"), "\(code): \(k)")
            XCTAssertTrue(outcome.message.contains("the old key and the ssh alias are untouched"), outcome.message)
        }
    }

    func testASwapWhoseSecondMoveFailedSaysTheKeyIsInUseAndThePubMayBeStale() async {
        let runner = replaceRunner(swapCode: 42)
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("new key in use"), outcome.message)
        XCTAssertTrue(outcome.message.contains("Check again"), outcome.message)
        XCTAssertFalse(kinds(runner).contains { $0.hasPrefix("gh delete") })
        let swap = unquoted(runner.calls.map(\.argv).first { unquoted($0.last ?? "").contains("mv -f") }!.last!)
        XCTAssertTrue(swap.contains("ssh-keygen -y"), "the public key file is rewritten from the private key when its move fails: \(swap)")
    }

    func testReplaceKeyWithAnUnreadableOldKeyChangesNothing() async {
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("head -c 1024") == true }, result: CommandResult(code: 0, out: "ssh-rsa AAAA\nrm -rf x\n", err: ""))
        let outcome = await run(replacePlan(), runner)
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("unexpected public key"), outcome.message)
        XCTAssertEqual(runner.calls.count, 1, "nothing but the read of the old key ran")
    }

    func testReplaceKeyWithAnUnsafeMachineNameRunsNothing() async {
        var machine = TestMachines.mac()
        machine.name = "m1; touch x"
        let runner = replaceRunner()
        let outcome = await replacePlan(machine: machine).run(.test(.test(machine: machine, runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    /// A machine named `m1-20261001` would own the title `agents-m1-20261001`, which is how `m1`'s replaced key is
    /// titled: Replace key refuses such a name, before any command.
    func testReplaceKeyRefusesAMachineNameThatLooksLikeAReplacedKeyTitle() async {
        for name in ["m1-20261001", "vps-99999999", "a-00000000"] {
            var machine = TestMachines.mac()
            machine.name = name
            let runner = replaceRunner()
            let outcome = await replacePlan(machine: machine).run(.test(.test(machine: machine, runner: runner)))
            XCTAssertFalse(outcome.ok, name)
            XCTAssertTrue(outcome.message.contains("a machine name can't end in -<8 digits>"), outcome.message)
            XCTAssertTrue(runner.calls.isEmpty, name)
        }
        for name in ["m1-2026100", "m1-202610011", "20261001", "m1-20261001-x", "m1"] {
            var machine = TestMachines.mac()
            machine.name = name
            let runner = replaceRunner()
            _ = await replacePlan(machine: machine).run(.test(.test(machine: machine, runner: runner)))
            XCTAssertFalse(runner.calls.isEmpty, "\(name) is a fine name")
        }
    }

    func testReplaceKeyMasksWhatItShowsBeforeCuttingIt() async {
        let token = testLongToken
        let said = String(repeating: "f", count: 170) + token
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("head -c 1024") == true }, result: CommandResult(code: 0, out: pub + "\n", err: ""))
        runner.on({ $0.last?.contains("ssh-keygen") == true }, result: CommandResult(code: 1, out: "", err: said))
        let outcome = await replacePlan().run(.test(.test(runner: runner, mask: testMask(token, as: "[secret:m1-github-issues]"))))
        XCTAssertFalse(outcome.ok)
        assertNoFragment(of: token, in: outcome.message)
        XCTAssertTrue(outcome.message.contains("[secret:m1-github-issues]"), outcome.message)
    }

    func testReplaceKeyIsOfferedWhenTheCheckPassesOrTheKeyIsNotOnGitHub() {
        XCTAssertTrue(RepoSteps.offersReplace(CheckResult(state: .pass("read-only key"))))
        let notListed = CheckResult(state: .fail("the key is not on \(repo) yet"),
                                    rows: [SubRow(name: "Key on GitHub", ok: false, detail: "not listed")])
        XCTAssertTrue(RepoSteps.offersReplace(notListed))
        // no key on the machine, a writable key, a missing alias, can't tell, no result yet
        XCTAssertFalse(RepoSteps.offersReplace(CheckResult(state: .fail("no deploy key on m1 yet"))))
        XCTAssertFalse(RepoSteps.offersReplace(CheckResult(state: .fail("can write"), rows: [SubRow(name: "Key on GitHub", ok: false, detail: "can write")])))
        XCTAssertFalse(RepoSteps.offersReplace(CheckResult(state: .fail("no ssh alias"),
                                                           rows: [SubRow(name: "Key on GitHub", ok: true, detail: "read-only")])))
        XCTAssertFalse(RepoSteps.offersReplace(CheckResult(state: .cantTell("waits for Reachable"))))
        XCTAssertFalse(RepoSteps.offersReplace(nil))
    }

    func testTheProbeNamesTheRowThatMakesReplaceKeyAvailable() async {
        // the Check itself produces the row Replace key looks for
        let (absent, _) = await check(text: remoteText(pub: pub, ssh: welcome), keys: ghKeys("ssh-ed25519 \(otherBody)", readOnly: true))
        XCTAssertTrue(RepoSteps.offersReplace(absent))
        let (ok, _) = await check(text: remoteText(pub: pub, ssh: welcome), keys: ghKeys("ssh-ed25519 \(body)", readOnly: true))
        XCTAssertTrue(RepoSteps.offersReplace(ok))
        let (writable, _) = await check(text: remoteText(pub: pub, ssh: welcome), keys: ghKeys("ssh-ed25519 \(body)", readOnly: false))
        XCTAssertFalse(RepoSteps.offersReplace(writable))
    }

    func testOnlyTheDeployKeyStepOfAValidRepoHasAReplacePlan() {
        let all = steps()
        XCTAssertNotNil(all[0].replace)
        XCTAssertNil(all[1].replace)
        XCTAssertNil(all[2].replace)
        let bad = MachineRepo(github: "a/b; touch x", localPath: "/x", checkCommand: "true")
        for step in RepoSteps.all(for: TestMachines.mac(), repo: bad) { XCTAssertNil(step.replace, step.id.raw) }
    }

    /// The scripts run in a real shell against a throwaway HOME: they parse, the new pair appears beside the old one,
    /// and the swap leaves the new key under the old file names and no `.new` file behind.
    func testReplaceKeyScriptsRunInARealShellOnAThrowawayHome() async throws {
        let home = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-replace-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: home.appendingPathComponent(".ssh"), withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: home) }
        let keyPath = home.appendingPathComponent(".ssh/agents-dtthaison-lingeva-mono").path
        try "OLD-PRIVATE".write(toFile: keyPath, atomically: true, encoding: .utf8)
        try pub.write(toFile: keyPath + ".pub", atomically: true, encoding: .utf8)
        // no network even if a script reached them: git and ssh are stubs that fail, first in the PATH
        let bin = home.appendingPathComponent("bin", isDirectory: true)
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        for tool in ["git", "ssh"] {
            let url = bin.appendingPathComponent(tool)
            try "#!/bin/sh\nexit 255\n".write(to: url, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        }

        func shell(_ script: String) throws -> (Int32, String) {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/bin/sh")
            p.arguments = ["-c", script]
            p.environment = ["PATH": "\(bin.path):/usr/bin:/bin", "HOME": home.path]
            let pipe = Pipe()
            p.standardOutput = pipe
            p.standardError = pipe
            try p.run()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            return (p.terminationStatus, String(decoding: data, as: UTF8.self))
        }
        // what the plan sends to the machine, with the ssh and gh calls left out
        let runner = replaceRunner()
        _ = await run(replacePlan(), runner)
        func script(containing needle: String) -> String {
            // ssh's last argument is `/bin/sh -c '<prelude + script>'`: the script is what sits inside the quotes
            var text = unquoted(runner.calls.map(\.argv).first { unquoted($0.last ?? "").contains(needle) && $0.first == "/usr/bin/ssh" }!.last!)
            text.removeFirst("/bin/sh -c '".count + RemoteShell.prelude.count)
            text.removeLast()
            return text
        }
        // every script is valid sh
        for needle in ["head -c 1024", "ssh-keygen", "-T git@github.com", "push --dry-run", "mv -f"] {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/bin/sh")
            p.arguments = ["-n", "-c", script(containing: needle)]
            try p.run()
            p.waitUntilExit()
            XCTAssertEqual(p.terminationStatus, 0, "syntax: \(needle)")
        }
        let (readCode, readOut) = try shell(script(containing: "head -c 1024"))
        XCTAssertEqual(readCode, 0)
        XCTAssertEqual(readOut, pub)

        let (genCode, genOut) = try shell(script(containing: "ssh-keygen"))
        XCTAssertEqual(genCode, 0, genOut)
        XCTAssertNotNil(RepoSteps.singleEd25519Line(genOut), genOut)
        XCTAssertTrue(FileManager.default.fileExists(atPath: keyPath + ".new"))
        XCTAssertEqual(try String(contentsOfFile: keyPath, encoding: .utf8), "OLD-PRIVATE", "the old key is untouched until the swap")
        let newPubText = try String(contentsOfFile: keyPath + ".new.pub", encoding: .utf8)

        let (swapCode, swapOut) = try shell(script(containing: "mv -f"))
        XCTAssertEqual(swapCode, 0, swapOut)
        XCTAssertEqual(try String(contentsOfFile: keyPath + ".pub", encoding: .utf8), newPubText)
        XCTAssertNotEqual(try String(contentsOfFile: keyPath, encoding: .utf8), "OLD-PRIVATE")
        XCTAssertFalse(FileManager.default.fileExists(atPath: keyPath + ".new"))
        XCTAssertFalse(FileManager.default.fileExists(atPath: keyPath + ".new.pub"))

        // a second swap has nothing to move: it fails without touching the key now in place
        let (again, _) = try shell(script(containing: "mv -f"))
        XCTAssertEqual(again, 43)
        XCTAssertEqual(try String(contentsOfFile: keyPath + ".pub", encoding: .utf8), newPubText)
    }

    /// The push check in a real shell with a stub git: a throwaway repo is made, the dry run goes to the repo's
    /// GitHub URL through the new key alone, and the exit code comes back on the marker line.
    func testReplaceKeyPushCheckRunsAThrowawayRepoThroughTheNewKeyOnly() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-pushcheck-\(UUID().uuidString)", isDirectory: true)
        let bin = dir.appendingPathComponent("bin", isDirectory: true), home = dir.appendingPathComponent("home", isDirectory: true)
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let log = dir.appendingPathComponent("git.log").path
        let stub = "#!/bin/sh\necho \"GIT_SSH_COMMAND=$GIT_SSH_COMMAND :: $*\" >> \(log)\n"
            + "case \"$*\" in *push*) echo 'ERROR: The key you are authenticating with has been marked as read only.' >&2; exit 128;; esac\nexit 0\n"
        let gitURL = bin.appendingPathComponent("git")
        try stub.write(to: gitURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: gitURL.path)
        // no network even if a stub is bypassed: ssh is a stub too
        let sshURL = bin.appendingPathComponent("ssh")
        try "#!/bin/sh\nexit 255\n".write(to: sshURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: sshURL.path)

        let runner = replaceRunner()
        _ = await run(replacePlan(), runner)
        var text = unquoted(runner.calls.map(\.argv).first { unquoted($0.last ?? "").contains("push --dry-run") && $0.first == "/usr/bin/ssh" }!.last!)
        // the prelude would replace the PATH that holds the stub: leave it out
        text.removeFirst("/bin/sh -c '".count + RemoteShell.prelude.count)
        text.removeLast()
        XCTAssertFalse(text.contains("export PATH"))
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", text]
        p.environment = ["PATH": "\(bin.path):/usr/bin:/bin", "HOME": home.path]
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        try p.run()
        let out = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        p.waitUntilExit()
        XCTAssertTrue(out.contains("marked as read only"), out)
        XCTAssertEqual(out.components(separatedBy: "\n").last { $0.hasPrefix("@@push ") }, "@@push 128")
        let calls = try String(contentsOfFile: log, encoding: .utf8)
        XCTAssertTrue(calls.contains("push --dry-run git@github.com:\(repo).git HEAD:refs/heads/agents-push-test"), calls)
        XCTAssertTrue(calls.contains("-F /dev/null -i '\(home.path)/.ssh/agents-dtthaison-lingeva-mono.new' -o IdentitiesOnly=yes"), calls)
        XCTAssertTrue(calls.contains("-o StrictHostKeyChecking=yes"), calls)
        for line in calls.split(separator: "\n") { XCTAssertTrue(line.contains("core.hooksPath=/dev/null"), String(line)) }
        XCTAssertFalse(calls.contains("github-agents-"), "the alias would lead to the old key")
    }

    func testTheSwapEndsOnTheSameFilesTheProbeAndFixUse() async {
        let runner = replaceRunner()
        _ = await run(replacePlan(), runner)
        let swap = unquoted(runner.calls.map(\.argv).first { unquoted($0.last ?? "").contains("mv -f") }!.last!)
        XCTAssertTrue(swap.contains("\"$HOME\"/'.ssh/agents-dtthaison-lingeva-mono'"), swap)
        XCTAssertTrue(swap.contains("\"$HOME\"/'.ssh/agents-dtthaison-lingeva-mono.pub'"), swap)
    }
}

/// Counts the listings of a repo's keys: the first is the one before the add.
private final class ListCalls: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    func next() -> Int { lock.withLock { count += 1; return count } }
}
