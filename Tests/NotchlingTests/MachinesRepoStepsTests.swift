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

    func testPushRefusedNeedsGitHubsReadOnlyMessage() async {
        let id = StepID.pushRefused(repo)
        let p = probe(id)
        XCTAssertTrue(p.script.contains("push --dry-run origin HEAD:refs/heads/agents-push-test"))
        let ctx = StepContext.test()
        let refused = await p.parse(RemoteOutput(text: "ERROR: The key you are authenticating with has been marked as read only.\nfatal: Could not read from remote repository.\n", code: 128), ctx)
        guard case .pass = refused.state else { return XCTFail("\(refused.state)") }
        let accepted = await p.parse(RemoteOutput(text: "To github:x\n * [new branch] HEAD -> agents-push-test\n", code: 0), ctx)
        guard case let .fail(why) = accepted.state else { return XCTFail("\(accepted.state)") }
        XCTAssertTrue(why.contains("can push"), why)
        let other = await p.parse(RemoteOutput(text: "fatal: not a git repository\nmore\n", code: 128), ctx)
        XCTAssertEqual(other.state, .fail("fatal: not a git repository"))
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
}
