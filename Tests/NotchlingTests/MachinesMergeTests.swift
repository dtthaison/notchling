import XCTest
@testable import Notchling

/// Records every call and answers from scripted rules, so no test starts git, gh or ssh.
/// A rule matches when its text is in the call written as `git -C /t status --porcelain`
/// (the executable shown by name only); the first match wins and anything else succeeds with no output.
private final class MergeFakeRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    private var _calls: [[String]] = []
    private var _cwds: [String?] = []
    private var _timeouts: [TimeInterval] = []
    private var _envs: [[String: String]] = []
    var rules: [(String, CommandResult)]
    /// Rules that answer one call at a time; the last answer repeats. They win over `rules`.
    var queues: [(String, [CommandResult])]
    private var used: [Int: Int] = [:]

    init(_ rules: [(String, CommandResult)] = [], queues: [(String, [CommandResult])] = []) {
        self.rules = rules
        self.queues = queues
    }

    var calls: [[String]] { lock.withLock { _calls } }
    var cwds: [String?] { lock.withLock { _cwds } }
    var timeouts: [TimeInterval] { lock.withLock { _timeouts } }
    /// The environment override of each call (empty for a call made without one).
    var envs: [[String: String]] { lock.withLock { _envs } }
    /// Each call as one line, executable by name.
    var lines: [String] { calls.map(Self.line) }

    static func line(_ argv: [String]) -> String {
        ([(argv.first.map { ($0 as NSString).lastPathComponent } ?? "")] + argv.dropFirst()).joined(separator: " ")
    }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], timeout: timeout)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        let line = Self.line(argv)
        return lock.withLock {
            _calls.append(argv)
            _cwds.append(cwd)
            _timeouts.append(timeout)
            _envs.append(env)
            if let q = queues.firstIndex(where: { line.contains($0.0) }) {
                let n = used[q, default: 0]
                used[q] = n + 1
                return queues[q].1[min(n, queues[q].1.count - 1)]
            }
            return rules.first { line.contains($0.0) }?.1 ?? CommandResult(code: 0, out: "", err: "")
        }
    }
}

private func ok(_ out: String = "") -> CommandResult { CommandResult(code: 0, out: out, err: "") }
private func bad(_ err: String = "boom", code: Int32 = 1, out: String = "") -> CommandResult { CommandResult(code: code, out: out, err: err) }

final class MachinesMergeTests: XCTestCase {
    private let target = "/Users/me/code/app"
    private let sha = "1111111111111111111111111111111111111111"
    private let merged = "2222222222222222222222222222222222222222"
    private let pre = "0000000000000000000000000000000000000abc"
    private let ref = "refs/machines/m1/7"
    private let aheadRange = "refs/remotes/origin/main..refs/heads/main"

    private var repo: MachineRepo { MachineRepo(github: "acme/app", localPath: "/Users/me/code/app", checkCommand: "just check") }
    private var machine: Machine {
        Machine(name: "m1", host: "m1.tail", user: "agent", sdPath: "/sd/sd.py", fileVault: false, repos: [repo])
    }
    private var entry: ReadyEntry {
        ReadyEntry(repo: "acme/app", issue: 7, branch: "sd/issue-7", commit: sha, worktree: "/work/app 7", evidenceDir: "", readyAt: nil)
    }
    private var mainListing: String {
        "worktree /Users/me/code/app\nHEAD aaa\nbranch refs/heads/main\n"
    }
    /// What the check's shell is called here: the login shell of whoever runs the tests.
    private var shell: String { (MergePlan.checkInvocation("x").argv[0] as NSString).lastPathComponent }
    private var git: String { MachineTools.path("git") }

    /// Rules for a run where every step passes. `rev-parse HEAD` answers the sha before the merge, then the
    /// merge commit (also at each re-check).
    private func happyRules() -> [(String, CommandResult)] {
        [("worktree list", ok(mainListing)),
         ("check-ref-format --branch sd/issue-7", ok("sd/issue-7\n")),
         ("rev-parse \(ref)", ok(sha + "\n")),
         ("--count \(aheadRange)", ok("0\n")),
         ("symbolic-ref HEAD", ok("refs/heads/main\n")),
         ("merge --no-ff", ok("Merge made by the 'ort' strategy.\n"))]
    }

    private var happyQueues: [(String, [CommandResult])] { [("rev-parse HEAD", [ok(pre + "\n"), ok(merged + "\n")])] }

    private func happyRunner(_ extra: [(String, CommandResult)] = [], queues: [(String, [CommandResult])]? = nil) -> MergeFakeRunner {
        MergeFakeRunner(extra + happyRules(), queues: queues ?? happyQueues)
    }

    private func t(_ rest: String) -> String { "git -C \(target) \(rest)" }

    // MARK: Target

    func testMergeTargetPrefersCleanMainWorktree() {
        let list = """
        worktree /Users/me/code/app-feature
        HEAD aaa
        branch refs/heads/feature

        worktree /Users/me/code/app main
        HEAD bbb
        branch refs/heads/main

        worktree /tmp/other
        HEAD ccc
        branch refs/heads/main-old

        """
        XCTAssertEqual(MergePlan.target(repo: repo, worktreeList: list), .existing(path: "/Users/me/code/app main"))
    }

    func testMergeTargetMakesOwnWhenMainUnchecked() {
        let list = "worktree /Users/me/code/app\nHEAD aaa\nbranch refs/heads/feature\n\nworktree /x\nHEAD bbb\ndetached\n"
        XCTAssertEqual(MergePlan.target(repo: repo, worktreeList: list),
                       .own(path: "~/Library/Application Support/Notchling/merge/acme-app"))
    }

    func testOwnWorktreeIsMadeUnderTheRealHomeFolder() async {
        let list = "worktree /Users/me/code/app\nHEAD aaa\nbranch refs/heads/feature\n"
        let runner = MergeFakeRunner([("worktree list", ok(list)), ("check-ref-format", ok("sd/issue-7")), ("rev-parse \(ref)", ok(sha)),
                                      ("--count \(aheadRange)", ok("0"))], queues: happyQueues)
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        let own = FileManager.default.homeDirectoryForCurrentUser.path + "/Library/Application Support/Notchling/merge/acme-app"
        XCTAssertEqual(runner.lines[1], "mkdir -p " + (own as NSString).deletingLastPathComponent)
        XCTAssertEqual(runner.lines[2], "git -C /Users/me/code/app worktree add \(own) main")
        XCTAssertEqual(runner.lines[3], "git -C \(own) check-ref-format --branch sd/issue-7")
        XCTAssertEqual(runner.lines[4], "git -C \(own) status --porcelain")
        XCTAssertFalse(runner.lines.joined().contains("~"))
    }

    // MARK: Refusals

    func testDirtyMainRefuses() async {
        let runner = MergeFakeRunner([("worktree list", ok(mainListing)), ("check-ref-format", ok("sd/issue-7")),
                                      ("status --porcelain", ok(" M file.swift\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main has local changes in \(target)")
        XCTAssertFalse(o.fetched)
        XCTAssertEqual(runner.lines, [
            "git -C /Users/me/code/app worktree list --porcelain",
            t("check-ref-format --branch sd/issue-7"),
            t("status --porcelain"),
        ])
    }

    func testFetchFailureChangesNothing() async {
        let runner = happyRunner([("fetch --no-tags ssh://", bad("ssh: could not resolve host"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.fetched)
        XCTAssertFalse(o.merged)
        XCTAssertTrue(o.error?.contains("could not resolve host") ?? false)
        let joined = runner.lines.joined(separator: "\n")
        XCTAssertFalse(joined.contains(" merge --no-ff"))
        XCTAssertFalse(joined.contains(" push "))
        XCTAssertFalse(runner.lines.contains { $0.hasPrefix("gh ") })
        XCTAssertFalse(runner.lines.contains { $0.contains(" reset ") })
    }

    func testMovedBranchRefuses() async {
        let runner = happyRunner([("rev-parse \(ref)", ok("9999999999999999999999999999999999999999\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "branch moved on the machine; refresh")
        XCTAssertTrue(o.fetched)
        XCTAssertFalse(o.merged)
        XCTAssertFalse(runner.lines.contains { $0.contains(" merge --no-ff") || $0.contains(" push ") || $0.hasPrefix("gh ") })
    }

    // MARK: Branch names (C16)

    func testABranchThatLooksLikeAnOptionNeverReachesGit() async {
        for bad in ["-x", "--upload-pack=sh", "", "a b", "a\nb"] {
            let runner = happyRunner()
            var e = entry
            e.branch = bad
            let o = await MergePlan.merge(e, machine: machine, repo: repo, runner: runner)
            XCTAssertNotNil(o.error, "'\(bad)' was accepted")
            XCTAssertTrue(runner.lines.isEmpty, "'\(bad)' reached a command: \(runner.lines)")
        }
    }

    func testGitMustAcceptTheBranchNameAndPrintItBackUnchanged() async {
        // refused by git
        var runner = happyRunner([("check-ref-format", bad("fatal: 'a..b' is not a valid branch name"))])
        var o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "\"sd/issue-7\" is not a branch name git accepts")
        XCTAssertEqual(runner.lines.suffix(1), [t("check-ref-format --branch sd/issue-7")])
        XCTAssertFalse(o.fetched)

        // accepted, but expanded to another name (`@{-1}` is "the branch before this one")
        runner = happyRunner([("check-ref-format", ok("some-other-branch\n"))])
        o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "\"sd/issue-7\" is not a branch name git accepts")
        XCTAssertFalse(runner.lines.contains { $0.contains("fetch") || $0.contains("merge") || $0.contains("push") })
    }

    // MARK: Main ahead of origin (C18)

    func testUnpushedMainRefusesWithoutSuggestingAPush() async {
        let runner = happyRunner([("--first-parent --count", ok("2\n")), ("--count \(aheadRange)", ok("2\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main has 2 commit(s) that are not on origin; push or undo them yourself")
        XCTAssertFalse(o.fetched)
        XCTAssertFalse(o.merged)
        XCTAssertFalse(o.canUndo)
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.contains(" reset ") || $0.contains(" merge --no-ff") })
    }

    func testMainAheadByExactlyThisItemsMergeIsReportedAndOffersUndoNeverARetry() async {
        let runner = happyRunner([("--first-parent --count", ok("1\n")), ("--count \(aheadRange)", ok("2\n")),
                                  ("rev-list --parents -n 1 refs/heads/main", ok("\(merged) \(pre) \(sha)\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.error?.hasPrefix("merged locally, not pushed") ?? false, o.error ?? "nil")
        XCTAssertTrue(o.merged)
        XCTAssertFalse(o.checked, "its check never ran")
        XCTAssertFalse(o.pushed)
        XCTAssertEqual(o.commit, merged)
        XCTAssertTrue(o.canUndo)
        XCTAssertFalse(o.canRetryPush)
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.contains(" reset ") || $0.hasPrefix("gh ") || $0.contains("fetch --no-tags ssh") })
    }

    func testMainAheadByAnotherBranchsMergeIsNotThisItems() async {
        let other = "4444444444444444444444444444444444444444"
        let runner = happyRunner([("--first-parent --count", ok("1\n")), ("--count \(aheadRange)", ok("2\n")),
                                  ("rev-list --parents -n 1 refs/heads/main", ok("\(merged) \(pre) \(other)\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main has 2 commit(s) that are not on origin; push or undo them yourself")
        XCTAssertFalse(o.merged)
    }

    // MARK: Conflict, check, push

    func testConflictAbortsAndLabelsNeedsYou() async {
        let runner = happyRunner([("merge --no-ff", bad("CONFLICT (content): Merge conflict in a.swift", out: "Automatic merge failed")),
                                  ("diff --name-only", ok("a.swift\nb/c.swift\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.conflictFiles, ["a.swift", "b/c.swift"])
        XCTAssertFalse(o.merged)
        let n = runner.calls.count
        XCTAssertEqual(Array(runner.lines.suffix(5)).prefix(3), [t("rev-parse -q --verify MERGE_HEAD"), t("diff --name-only --diff-filter=U"), t("merge --abort")])
        let comment = runner.calls[n - 2], edit = runner.calls[n - 1]
        XCTAssertEqual(Array(comment.dropFirst().prefix(5)), ["issue", "comment", "7", "-R", "acme/app"])
        XCTAssertTrue(comment.last?.hasPrefix("notchling: ") ?? false)
        XCTAssertTrue(comment.last?.contains("a.swift, b/c.swift") ?? false)
        XCTAssertEqual(Array(edit.dropFirst()), ["issue", "edit", "7", "-R", "acme/app", "--remove-label", "sd:ready", "--add-label", "sd:needs-you"])
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.contains("-lc") })
    }

    func testMergeFailureWithoutConflictLabelsNothing() async {
        let runner = happyRunner([("merge --no-ff", bad("fatal: refusing to merge unrelated histories"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.error?.contains("unrelated histories") ?? false)
        XCTAssertTrue(o.conflictFiles.isEmpty)
        XCTAssertFalse(runner.lines.contains { $0.hasPrefix("gh ") })
    }

    /// `merge --abort` says "no merge in progress" when the merge never started; that must not hide the real error.
    func testAMergeThatNeverStartedReportsItsOwnErrorAndDoesNotAbort() async {
        let runner = happyRunner([("merge --no-ff", bad("fatal: refusing to merge unrelated histories")),
                                  ("--verify MERGE_HEAD", bad("", code: 1)), ("merge --abort", bad("fatal: There is no merge to abort (MERGE_HEAD missing).", code: 128))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "merge failed: fatal: refusing to merge unrelated histories")
        XCTAssertFalse(runner.lines.contains { $0.contains("merge --abort") || $0.contains("diff --name-only") || $0.hasPrefix("gh ") })
        XCTAssertEqual(runner.lines.last, t("rev-parse -q --verify MERGE_HEAD"))
    }

    func testNothingToMergeStopsBeforeTheCheck() async {
        let runner = happyRunner(queues: [("rev-parse HEAD", [ok(pre + "\n")])])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.merged)
        XCTAssertTrue(o.error?.contains("already in main") ?? false)
        XCTAssertFalse(runner.lines.contains { $0.contains(" reset ") || $0.contains(" push ") || $0.hasPrefix(shell) })
    }

    func testCheckFailureResetsAndKeepsReady() async {
        let runner = happyRunner([("-lc", bad("error: 2 tests failed", out: "building...\nFAIL FooTests"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.checked)
        XCTAssertFalse(o.merged)
        XCTAssertFalse(o.pushed)
        XCTAssertFalse(o.canRetryPush)
        XCTAssertFalse(o.canUndo)
        XCTAssertTrue(o.error?.contains("2 tests failed") ?? false)
        XCTAssertEqual(runner.lines.suffix(3), [t("symbolic-ref HEAD"), t("rev-parse HEAD"), t("reset --keep \(pre)")])
        XCTAssertFalse(runner.lines.contains { $0.contains("ORIG_HEAD") || $0.contains("--hard") })
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") })
        // the check ran in the target, with a long timeout
        guard let i = runner.lines.firstIndex(where: { $0.contains(" -lc ") }) else { return XCTFail("the check never ran") }
        XCTAssertEqual(runner.cwds[i], target)
        XCTAssertEqual(runner.timeouts[i], 1800)
    }

    /// C17: the check failed and putting main back failed too. The merge is still on main, and no button may push it.
    func testFailedCheckWhoseResetFailsIsAHardErrorWithNoRetry() async {
        let runner = happyRunner([("-lc", bad("error: 2 tests failed")), ("reset --keep", bad("fatal: Unable to create index.lock"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.error?.hasPrefix("check failed and main could not be put back; fix it by hand in \(target)") ?? false, o.error ?? "nil")
        XCTAssertTrue(o.error?.contains("2 tests failed") ?? false)
        XCTAssertTrue(o.merged)
        XCTAssertFalse(o.checked)
        XCTAssertFalse(o.canRetryPush)
        XCTAssertFalse(o.pushed)
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") })
    }

    func testPushFailureKeepsIssueOpen() async {
        let runner = happyRunner([("push origin", bad("remote: rejected"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.merged)
        XCTAssertTrue(o.checked)
        XCTAssertFalse(o.pushed)
        XCTAssertFalse(o.closed)
        XCTAssertEqual(o.commit, merged)
        XCTAssertTrue(o.canRetryPush)
        XCTAssertTrue(o.canUndo)
        XCTAssertTrue(o.error?.contains("rejected") ?? false)
        XCTAssertFalse(runner.lines.contains { $0.hasPrefix("gh ") || $0.contains(" reset ") })
    }

    // MARK: Happy path

    func testHappyPathOrder() async {
        let runner = happyRunner()
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o, MergeOutcome(fetched: true, merged: true, checked: true, pushed: true, closed: true, commit: merged, error: nil, conflictFiles: []))
        XCTAssertEqual(runner.lines, [
            "git -C /Users/me/code/app worktree list --porcelain",
            t("check-ref-format --branch sd/issue-7"),
            t("status --porcelain"),
            t("fetch --no-tags origin +refs/heads/main:refs/remotes/origin/main"),
            t("merge --ff-only refs/remotes/origin/main"),
            t("rev-list --count \(aheadRange)"),
            t("fetch --no-tags ssh://agent@m1.tail/work/app 7 +refs/heads/sd/issue-7:\(ref)"),
            t("rev-parse \(ref)"),
            t("rev-parse HEAD"),
            t("merge --no-ff -m Merge sd/issue-7 (acme/app#7) \(sha)"),
            t("rev-parse HEAD"),
            "\(shell) -lc just check",
            t("symbolic-ref HEAD"),
            t("rev-parse HEAD"),
            t("status --porcelain"),
            t("push origin refs/heads/main:refs/heads/main"),
            "gh issue close 7 -R acme/app --comment notchling: merged in \(merged)",
            "gh issue edit 7 -R acme/app --remove-label sd:ready",
        ])
    }

    /// C16: every ref git is asked about is spelled in full, and no fetch asks for tags.
    func testNoRefIsAShortNameAndEveryFetchSaysNoTags() async {
        let runner = happyRunner()
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        let fetches = runner.calls.filter { $0.contains("fetch") }
        XCTAssertEqual(fetches.count, 2)
        for f in fetches { XCTAssertTrue(f.contains("--no-tags"), "\(f)") }
        for argv in runner.calls where URL(fileURLWithPath: argv[0]).lastPathComponent == "git" {
            for word in argv {
                XCTAssertFalse(["main", "origin/main"].contains(word), "short ref '\(word)' in \(argv)")
            }
        }
        XCTAssertEqual(runner.calls.filter { $0.contains("push") }, [[git, "-C", target, "push", "origin", "refs/heads/main:refs/heads/main"]])
    }

    func testOnlyMainIsPushedAndNeverForced() async {
        let runner = happyRunner()
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        let pushes = runner.calls.filter { $0.contains("push") }
        XCTAssertEqual(pushes, [[git, "-C", target, "push", "origin", "refs/heads/main:refs/heads/main"]])
        for argv in runner.calls where argv.contains("-lc") == false && argv.contains("fetch") == false {
            XCTAssertFalse(argv.contains { $0 == "--force" || $0 == "-f" || $0.hasPrefix("--force-") || $0.hasPrefix("+refs/heads") || $0.hasPrefix("+main") })
        }
    }

    func testGitCallsUseFullPathsAndTimeouts() async {
        let runner = happyRunner()
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        for (argv, timeout) in zip(runner.calls, runner.timeouts) {
            XCTAssertTrue(argv[0].hasPrefix("/"), "\(argv[0]) is not a full path")
            if argv[0] == git && argv.contains("fetch") { XCTAssertEqual(timeout, 300) }
            else if argv[0] == git { XCTAssertEqual(timeout, 60) }
        }
    }

    /// C13: git and gh come from the one lookup the rest of the Machines code uses.
    func testGitAndGhComeFromMachineTools() async {
        let runner = happyRunner()
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        for argv in runner.calls {
            switch URL(fileURLWithPath: argv[0]).lastPathComponent {
            case "git": XCTAssertEqual(argv[0], MachineTools.path("git"))
            case "gh": XCTAssertEqual(argv[0], MachineTools.path("gh"))
            default: break
            }
        }
        XCTAssertTrue(runner.calls.contains { URL(fileURLWithPath: $0[0]).lastPathComponent == "gh" })
    }

    // MARK: The check's shell (C19)

    func testCheckRunsInTheLoginShellWithToolDirectoriesFirstOnPath() async {
        let runner = happyRunner()
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        guard let i = runner.calls.firstIndex(where: { $0.contains("-lc") }) else { return XCTFail("the check never ran") }
        let expected = MergePlan.checkInvocation("just check")
        XCTAssertEqual(runner.calls[i], expected.argv)
        XCTAssertEqual(Array(runner.calls[i].dropFirst()), ["-lc", "just check"])
        XCTAssertEqual(runner.envs[i], expected.env)
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        XCTAssertTrue(runner.envs[i]["PATH"]?.hasPrefix("\(home)/.cargo/bin:\(home)/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:") ?? false,
                      runner.envs[i]["PATH"] ?? "no PATH")
        XCTAssertTrue(runner.envs[i]["PATH"]?.hasSuffix(ProcessInfo.processInfo.environment["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin") ?? false)
        XCTAssertEqual(runner.cwds[i], target)
        // no other command gets an environment
        XCTAssertEqual(runner.envs.enumerated().filter { !$0.element.isEmpty }.map(\.offset), [i])
    }

    func testCheckInvocationUsesTheUsersShellWhenItIsOneWeCanRun() {
        let env = ["SHELL": "/bin/bash", "PATH": "/usr/bin:/bin"]
        let call = MergePlan.checkInvocation("just check-affected", environment: env, home: "/Users/me", isExecutable: { _ in true })
        XCTAssertEqual(call.argv, ["/bin/bash", "-lc", "just check-affected"])
        XCTAssertEqual(call.env, ["PATH": "/Users/me/.cargo/bin:/Users/me/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"])
    }

    func testCheckInvocationFallsBackToZshWithoutAUsableShell() {
        for env in [[:], ["SHELL": ""], ["SHELL": "zsh"], ["SHELL": "/nonexistent/shell"]] as [[String: String]] {
            let call = MergePlan.checkInvocation("c", environment: env, home: "/Users/me", isExecutable: { $0 == "/bin/zsh" })
            XCTAssertEqual(call.argv, ["/bin/zsh", "-lc", "c"], "\(env)")
            XCTAssertEqual(call.env["PATH"], "/Users/me/.cargo/bin:/Users/me/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
        }
    }

    // MARK: Guards added in fix round 1

    func testBranchSwitchedDuringFailedCheckBlocksReset() async {
        let runner = happyRunner([("-lc", bad("tests failed")), ("symbolic-ref HEAD", ok("refs/heads/feature\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main changed during the check; nothing pushed")
        XCTAssertFalse(runner.lines.contains { $0.contains(" reset ") || $0.contains(" push ") || $0.hasPrefix("gh ") })
    }

    func testMovedHeadDuringFailedCheckBlocksReset() async {
        let other = "3333333333333333333333333333333333333333"
        let runner = happyRunner([("-lc", bad("tests failed"))], queues: [("rev-parse HEAD", [ok(pre), ok(merged), ok(other)])])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main changed during the check; nothing pushed")
        XCTAssertFalse(runner.lines.contains { $0.contains(" reset ") })
    }

    func testBranchSwitchedDuringCheckBlocksPush() async {
        let runner = happyRunner([("symbolic-ref HEAD", ok("refs/heads/feature\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main changed during the check; nothing pushed")
        XCTAssertFalse(o.checked, "the check ran on another tree than the one to push")
        XCTAssertTrue(o.merged)
        XCTAssertFalse(o.pushed)
        XCTAssertFalse(o.canRetryPush)
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") || $0.contains(" reset ") })
    }

    func testCommitAddedDuringCheckBlocksPush() async {
        let other = "3333333333333333333333333333333333333333"
        let runner = happyRunner(queues: [("rev-parse HEAD", [ok(pre), ok(merged), ok(other)])])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main changed during the check; nothing pushed")
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") })
    }

    /// I3: the check built a tree with an edit in it; what would be pushed is a different tree.
    func testLocalChangesMadeDuringTheCheckBlockThePushAndKeepTheMergeLocal() async {
        let runner = happyRunner(queues: [("status --porcelain", [ok(""), ok(" M f.txt\n")]), ("rev-parse HEAD", [ok(pre), ok(merged)])])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main has local changes made during the check; nothing pushed")
        XCTAssertTrue(o.merged)
        XCTAssertFalse(o.checked)
        XCTAssertFalse(o.pushed)
        XCTAssertTrue(o.canUndo, "the merge stays local, so Undo is the way out")
        XCTAssertFalse(o.canRetryPush)
        XCTAssertEqual(runner.lines.suffix(3), [t("symbolic-ref HEAD"), t("rev-parse HEAD"), t("status --porcelain")])
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") || $0.contains(" reset ") })
    }

    /// An edit made during a failed check does not stop the reset: `--keep` leaves the edit alone.
    func testAFailedCheckStillPutsMainBackWhenTheTreeGotDirtyMeanwhile() async {
        let runner = happyRunner([("-lc", bad("tests failed"))],
                                 queues: [("status --porcelain", [ok(""), ok(" M f.txt\n")]), ("rev-parse HEAD", [ok(pre), ok(merged)])])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.merged, "reset --keep ran, and kept the edit")
        XCTAssertEqual(runner.lines.last, t("reset --keep \(pre)"))
    }

    func testFailedAbortStopsBeforeGh() async {
        let runner = happyRunner([("merge --no-ff", bad("CONFLICT")), ("diff --name-only", ok("a.swift\n")),
                                  ("merge --abort", bad("fatal: cannot abort"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "merge could not be aborted in \(target); fix it by hand")
        XCTAssertFalse(runner.lines.contains { $0.hasPrefix("gh ") })
    }

    func testFailedDiffIsReportedAndLabelsNothing() async {
        let runner = happyRunner([("merge --no-ff", bad("fatal: weird")), ("diff --name-only", bad("fatal: diff broke"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.error?.contains("could not be listed") ?? false)
        XCTAssertTrue(o.conflictFiles.isEmpty)
        XCTAssertFalse(runner.lines.contains { $0.hasPrefix("gh ") })
        XCTAssertEqual(runner.lines.last, t("merge --abort"))
    }

    // MARK: Outcome flags (C17, C18)

    func testOutcomeFlagsDecideWhichButtonsShow() {
        // (merged, checked, pushed) -> (canRetryPush, canUndo)
        let table: [(Bool, Bool, Bool, Bool, Bool)] = [
            (false, false, false, false, false),  // nothing merged
            (true, false, false, false, true),    // merged locally, never checked: undo only
            (true, true, false, true, true),      // checked, push failed: both
            (true, true, true, false, false),     // pushed
            (false, true, false, false, false),
        ]
        for (merged, checked, pushed, retry, undo) in table {
            let o = MergeOutcome(merged: merged, checked: checked, pushed: pushed)
            XCTAssertEqual(o.canRetryPush, retry, "merged \(merged) checked \(checked) pushed \(pushed)")
            XCTAssertEqual(o.canUndo, undo, "merged \(merged) checked \(checked) pushed \(pushed)")
        }
    }

    // MARK: Retry

    /// A target where the failed push left exactly one merge of this branch on top of origin/main.
    private func retryRunner(_ extra: [(String, CommandResult)] = []) -> MergeFakeRunner {
        MergeFakeRunner(extra + [("worktree list", ok(mainListing)),
                                 ("status --porcelain", ok("")),
                                 ("symbolic-ref HEAD", ok("refs/heads/main\n")),
                                 ("rev-parse HEAD^2", ok(sha + "\n")),
                                 ("--first-parent --count", ok("1\n")),
                                 ("rev-parse HEAD", ok(merged + "\n"))])
    }

    func testRetryPushRepeatsOnlyPushAndClose() async {
        let runner = retryRunner()
        let o = await MergePlan.retryPush(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.pushed)
        XCTAssertTrue(o.closed)
        XCTAssertEqual(o.commit, merged)
        XCTAssertEqual(runner.lines, [
            "git -C /Users/me/code/app worktree list --porcelain",
            t("status --porcelain"),
            t("symbolic-ref HEAD"),
            t("rev-parse HEAD^2"),
            t("rev-list --first-parent --count \(aheadRange)"),
            t("rev-parse HEAD"),
            t("push origin refs/heads/main:refs/heads/main"),
            "gh issue close 7 -R acme/app --comment notchling: merged in \(merged)",
            "gh issue edit 7 -R acme/app --remove-label sd:ready",
        ])
    }

    private func assertRetryRefuses(_ extra: [(String, CommandResult)], file: StaticString = #filePath, line: UInt = #line) async {
        let runner = retryRunner(extra)
        let o = await MergePlan.retryPush(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.pushed, file: file, line: line)
        XCTAssertNotNil(o.error, file: file, line: line)
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") }, file: file, line: line)
    }

    func testRetryPushRefusesDirtyTree() async { await assertRetryRefuses([("status --porcelain", ok(" M a.swift\n"))]) }
    func testRetryPushRefusesWhenNotOnMain() async { await assertRetryRefuses([("symbolic-ref HEAD", ok("refs/heads/feature\n"))]) }
    func testRetryPushRefusesWhenHeadIsNotThisMerge() async { await assertRetryRefuses([("rev-parse HEAD^2", ok("9999999999999999999999999999999999999999\n"))]) }
    func testRetryPushRefusesWhenHeadIsNoMerge() async { await assertRetryRefuses([("rev-parse HEAD^2", bad("fatal: ambiguous argument"))]) }
    func testRetryPushRefusesOtherUnpushedCommits() async { await assertRetryRefuses([("--first-parent --count", ok("2\n"))]) }
    func testRetryPushRefusesWhenNothingIsAhead() async { await assertRetryRefuses([("--first-parent --count", ok("0\n"))]) }

    // MARK: Undo (C18)

    /// A target whose main holds exactly this item's merge on top of origin.
    private func undoRunner(_ extra: [(String, CommandResult)] = []) -> MergeFakeRunner {
        MergeFakeRunner(extra + [("worktree list", ok(mainListing)),
                                 ("status --porcelain", ok("")),
                                 ("symbolic-ref HEAD", ok("refs/heads/main\n")),
                                 ("rev-list --parents -n 1 HEAD", ok("\(merged) \(pre) \(sha)\n")),
                                 ("--first-parent --count", ok("1\n"))])
    }

    func testUndoGoesBackToTheFirstParentAfterEveryGuard() async {
        let runner = undoRunner()
        let o = await MergePlan.undoMerge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertNil(o.error)
        XCTAssertFalse(o.merged)
        XCTAssertFalse(o.canUndo)
        XCTAssertEqual(runner.lines, [
            "git -C /Users/me/code/app worktree list --porcelain",
            t("status --porcelain"),
            t("symbolic-ref HEAD"),
            t("rev-list --parents -n 1 HEAD"),
            t("fetch --no-tags origin +refs/heads/main:refs/remotes/origin/main"),
            t("rev-list --first-parent --count \(aheadRange)"),
            t("reset --keep HEAD^1"),
        ])
        XCTAssertEqual(runner.timeouts[4], 300)
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") || $0.contains("--hard") })
    }

    private func assertUndoRefuses(_ extra: [(String, CommandResult)], mentions: String? = nil,
                                   file: StaticString = #filePath, line: UInt = #line) async {
        let runner = undoRunner(extra)
        let o = await MergePlan.undoMerge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertNotNil(o.error, file: file, line: line)
        if let mentions { XCTAssertTrue(o.error?.contains(mentions) ?? false, o.error ?? "nil", file: file, line: line) }
        XCTAssertFalse(runner.lines.contains { $0.contains(" reset ") || $0.contains(" push ") || $0.hasPrefix("gh ") }, file: file, line: line)
    }

    func testUndoRefusesDirtyTree() async { await assertUndoRefuses([("status --porcelain", ok(" M a.swift\n"))], mentions: "local changes") }
    func testUndoRefusesWhenNotOnMain() async { await assertUndoRefuses([("symbolic-ref HEAD", ok("refs/heads/feature\n"))], mentions: "not on main") }
    func testUndoRefusesWhenHeadIsAnotherBranchsMerge() async {
        await assertUndoRefuses([("rev-list --parents -n 1 HEAD", ok("\(merged) \(pre) 9999999999999999999999999999999999999999\n"))])
    }
    func testUndoRefusesWhenHeadIsNoMerge() async { await assertUndoRefuses([("rev-list --parents -n 1 HEAD", ok("\(merged) \(pre)\n"))]) }
    func testUndoRefusesAnOctopusMerge() async {
        await assertUndoRefuses([("rev-list --parents -n 1 HEAD", ok("\(merged) \(pre) \(sha) 5555555555555555555555555555555555555555\n"))])
    }
    func testUndoRefusesWhenTheMergeIsNotAloneOnTopOfOrigin() async { await assertUndoRefuses([("--first-parent --count", ok("2\n"))], mentions: "not on origin") }
    func testUndoRefusesWhenOriginCannotBeFetched() async { await assertUndoRefuses([("fetch --no-tags origin", bad("could not resolve host"))], mentions: "could not resolve host") }
    func testUndoReportsAFailedReset() async {
        let runner = undoRunner([("reset --keep", bad("error: Entry 'f.txt' not uptodate. Cannot merge."))])
        let o = await MergePlan.undoMerge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.error?.contains("Cannot merge") ?? false, o.error ?? "nil")
        XCTAssertEqual(runner.lines.last, t("reset --keep HEAD^1"))
    }
}
