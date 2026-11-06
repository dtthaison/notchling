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
    var rules: [(String, CommandResult)]

    init(_ rules: [(String, CommandResult)] = []) { self.rules = rules }

    var calls: [[String]] { lock.withLock { _calls } }
    var cwds: [String?] { lock.withLock { _cwds } }
    var timeouts: [TimeInterval] { lock.withLock { _timeouts } }
    /// Each call as one line, executable by name.
    var lines: [String] { calls.map(Self.line) }

    static func line(_ argv: [String]) -> String {
        ([(argv.first.map { ($0 as NSString).lastPathComponent } ?? "")] + argv.dropFirst()).joined(separator: " ")
    }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        let line = Self.line(argv)
        return lock.withLock {
            _calls.append(argv)
            _cwds.append(cwd)
            _timeouts.append(timeout)
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
    private let ref = "refs/machines/m1/7"

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

    /// Rules for a run where every step passes.
    private func happyRules() -> [(String, CommandResult)] {
        [("worktree list", ok(mainListing)),
         ("rev-parse \(ref)", ok(sha + "\n")),
         ("rev-parse HEAD", ok(merged + "\n")),
         ("merge --no-ff", ok("Merge made by the 'ort' strategy.\n"))]
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
        let runner = MergeFakeRunner([("worktree list", ok(list)), ("rev-parse \(ref)", ok(sha)), ("rev-parse HEAD", ok(merged))])
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        let own = FileManager.default.homeDirectoryForCurrentUser.path + "/Library/Application Support/Notchling/merge/acme-app"
        XCTAssertEqual(runner.lines[1], "mkdir -p " + (own as NSString).deletingLastPathComponent)
        XCTAssertEqual(runner.lines[2], "git -C /Users/me/code/app worktree add \(own) main")
        XCTAssertEqual(runner.lines[3], "git -C \(own) status --porcelain")
        XCTAssertFalse(runner.lines.joined().contains("~"))
    }

    // MARK: Refusals

    func testDirtyMainRefuses() async {
        let runner = MergeFakeRunner([("worktree list", ok(mainListing)), ("status --porcelain", ok(" M file.swift\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "main has local changes in \(target)")
        XCTAssertFalse(o.fetched)
        XCTAssertEqual(runner.lines, [
            "git -C /Users/me/code/app worktree list --porcelain",
            t("status --porcelain"),
        ])
    }

    func testFetchFailureChangesNothing() async {
        let runner = MergeFakeRunner([("worktree list", ok(mainListing)), ("fetch ssh://", bad("ssh: could not resolve host"))])
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
        let runner = MergeFakeRunner([("worktree list", ok(mainListing)), ("rev-parse \(ref)", ok("9999999999999999999999999999999999999999\n"))])
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.error, "branch moved on the machine; refresh")
        XCTAssertTrue(o.fetched)
        XCTAssertFalse(o.merged)
        XCTAssertFalse(runner.lines.contains { $0.contains(" merge --no-ff") || $0.contains(" push ") || $0.hasPrefix("gh ") })
    }

    // MARK: Conflict, check, push

    func testConflictAbortsAndLabelsNeedsYou() async {
        var rules = happyRules()
        rules.insert(("merge --no-ff", bad("CONFLICT (content): Merge conflict in a.swift", out: "Automatic merge failed")), at: 0)
        rules.insert(("diff --name-only", ok("a.swift\nb/c.swift\n")), at: 0)
        let runner = MergeFakeRunner(rules)
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o.conflictFiles, ["a.swift", "b/c.swift"])
        XCTAssertFalse(o.merged)
        let n = runner.calls.count
        XCTAssertEqual(Array(runner.lines.suffix(4)).prefix(2), [t("diff --name-only --diff-filter=U"), t("merge --abort")])
        let comment = runner.calls[n - 2], edit = runner.calls[n - 1]
        XCTAssertEqual(Array(comment.dropFirst().prefix(5)), ["issue", "comment", "7", "-R", "acme/app"])
        XCTAssertTrue(comment.last?.hasPrefix("notchling: ") ?? false)
        XCTAssertTrue(comment.last?.contains("a.swift, b/c.swift") ?? false)
        XCTAssertEqual(Array(edit.dropFirst()), ["issue", "edit", "7", "-R", "acme/app", "--remove-label", "sd:ready", "--add-label", "sd:needs-you"])
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.contains("sh -c") })
    }

    func testMergeFailureWithoutConflictLabelsNothing() async {
        let runner = MergeFakeRunner([("merge --no-ff", bad("fatal: refusing to merge unrelated histories"))] + happyRules())
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.error?.contains("unrelated histories") ?? false)
        XCTAssertTrue(o.conflictFiles.isEmpty)
        XCTAssertFalse(runner.lines.contains { $0.hasPrefix("gh ") })
        XCTAssertEqual(runner.lines.last, t("merge --abort"))
    }

    func testNothingToMergeStopsBeforeTheCheck() async {
        let runner = MergeFakeRunner([("merge --no-ff", ok("Already up to date.\n"))] + happyRules())
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.merged)
        XCTAssertTrue(o.error?.contains("already in main") ?? false)
        XCTAssertFalse(runner.lines.contains { $0.contains(" reset ") || $0.contains(" push ") || $0.hasPrefix("sh ") })
    }

    func testCheckFailureResetsAndKeepsReady() async {
        let runner = MergeFakeRunner([("sh -c", bad("error: 2 tests failed", out: "building...\nFAIL FooTests"))] + happyRules())
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.checked)
        XCTAssertFalse(o.merged)
        XCTAssertFalse(o.pushed)
        XCTAssertTrue(o.error?.contains("2 tests failed") ?? false)
        XCTAssertEqual(runner.lines.last, t("reset --hard ORIG_HEAD"))
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") })
        // the check ran in the target, with a long timeout
        guard let i = runner.lines.firstIndex(where: { $0.hasPrefix("sh -c") }) else { return XCTFail("the check never ran") }
        XCTAssertEqual(runner.calls[i], ["/bin/sh", "-c", "just check"])
        XCTAssertEqual(runner.cwds[i], target)
        XCTAssertEqual(runner.timeouts[i], 1800)
    }

    func testPushFailureKeepsIssueOpen() async {
        let runner = MergeFakeRunner([("push origin main", bad("remote: rejected"))] + happyRules())
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.merged)
        XCTAssertTrue(o.checked)
        XCTAssertFalse(o.pushed)
        XCTAssertFalse(o.closed)
        XCTAssertEqual(o.commit, merged)
        XCTAssertTrue(o.error?.contains("rejected") ?? false)
        XCTAssertFalse(runner.lines.contains { $0.hasPrefix("gh ") || $0.contains(" reset ") })
    }

    // MARK: Happy path

    func testHappyPathOrder() async {
        let runner = MergeFakeRunner(happyRules())
        let o = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertEqual(o, MergeOutcome(fetched: true, merged: true, checked: true, pushed: true, closed: true, commit: merged, error: nil, conflictFiles: []))
        XCTAssertEqual(runner.lines, [
            "git -C /Users/me/code/app worktree list --porcelain",
            t("status --porcelain"),
            t("fetch origin main"),
            t("merge --ff-only origin/main"),
            t("fetch ssh://agent@m1.tail/work/app 7 +sd/issue-7:\(ref)"),
            t("rev-parse \(ref)"),
            t("merge --no-ff -m Merge sd/issue-7 (acme/app#7) \(ref)"),
            t("rev-parse HEAD"),
            "sh -c just check",
            t("push origin main"),
            "gh issue close 7 -R acme/app --comment notchling: merged in \(merged)",
            "gh issue edit 7 -R acme/app --remove-label sd:ready",
        ])
    }

    func testOnlyMainIsPushedAndNeverForced() async {
        let runner = MergeFakeRunner(happyRules())
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        let pushes = runner.calls.filter { $0.contains("push") }
        XCTAssertEqual(pushes, [["/usr/bin/git", "-C", target, "push", "origin", "main"]])
        for argv in runner.calls where argv.first != "/bin/sh" {
            XCTAssertFalse(argv.contains { $0 == "--force" || $0 == "-f" || $0.hasPrefix("--force-") || $0.hasPrefix("+refs/heads") || $0.hasPrefix("+main") })
        }
    }

    func testGitCallsUseFullPathsAndTimeouts() async {
        let runner = MergeFakeRunner(happyRules())
        _ = await MergePlan.merge(entry, machine: machine, repo: repo, runner: runner)
        for (argv, timeout) in zip(runner.calls, runner.timeouts) {
            XCTAssertTrue(argv[0].hasPrefix("/"), "\(argv[0]) is not a full path")
            if argv[0] == "/usr/bin/git" && argv.contains("fetch") { XCTAssertEqual(timeout, 300) }
            else if argv[0] == "/usr/bin/git" { XCTAssertEqual(timeout, 60) }
        }
    }

    // MARK: Retry

    func testRetryPushRepeatsOnlyPushAndClose() async {
        let runner = MergeFakeRunner(happyRules())
        let o = await MergePlan.retryPush(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertTrue(o.pushed)
        XCTAssertTrue(o.closed)
        XCTAssertEqual(o.commit, merged)
        XCTAssertEqual(runner.lines, [
            "git -C /Users/me/code/app worktree list --porcelain",
            t("merge-base --is-ancestor \(ref) HEAD"),
            t("rev-parse HEAD"),
            t("push origin main"),
            "gh issue close 7 -R acme/app --comment notchling: merged in \(merged)",
            "gh issue edit 7 -R acme/app --remove-label sd:ready",
        ])
    }

    func testRetryPushRefusesWhenTheMergeIsNotInMain() async {
        let runner = MergeFakeRunner([("merge-base --is-ancestor", bad("", code: 1))] + happyRules())
        let o = await MergePlan.retryPush(entry, machine: machine, repo: repo, runner: runner)
        XCTAssertFalse(o.pushed)
        XCTAssertNotNil(o.error)
        XCTAssertFalse(runner.lines.contains { $0.contains(" push ") || $0.hasPrefix("gh ") })
    }
}
