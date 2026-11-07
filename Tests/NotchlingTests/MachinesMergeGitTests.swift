import XCTest
@testable import Notchling

/// MergePlan, MorningActions.diff and undoMerge against real git in a temp folder: an origin (bare), the
/// Mac's `main` checkout, and the worker's repo. The runner maps the `ssh://` URL to the worker's folder and
/// fakes `gh`; everything else is the real `ProcessRunner`. The fake-runner tests pin the order of the calls;
/// these pin what git does with them.
@MainActor
final class MachinesMergeGitTests: XCTestCase {
    // MARK: Fixture

    private var root = ""
    private var origin: String { root + "/origin.git" }
    private var mac: String { root + "/mac" }
    private var machinePath: String { root + "/machine" }
    private var runner: LocalRunner!

    override func setUpWithError() throws {
        let raw = FileManager.default.temporaryDirectory.appendingPathComponent("machines-git-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: raw, withIntermediateDirectories: true)
        // git prints real paths, so the root is one
        root = String(cString: realpath(raw.path, nil))
        runner = LocalRunner(machinePath: machinePath)

        sh("git init -q --bare -b main '\(origin)'")
        sh("git init -q -b main '\(mac)'")
        config(mac)
        write(mac, "f.txt", "base\n")
        sh("git -C '\(mac)' add f.txt && git -C '\(mac)' commit -q -m init")
        sh("git -C '\(mac)' remote add origin '\(origin)' && git -C '\(mac)' push -q -u origin main")

        sh("git clone -q '\(origin)' '\(machinePath)' 2>/dev/null")
        config(machinePath)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(atPath: root)
    }

    /// Runs a shell line with the user's own git config out of the way.
    @discardableResult private func sh(_ line: String) -> String {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", line]
        p.environment = ProcessInfo.processInfo.environment.merging(LocalRunner.gitEnvironment) { $1 }
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        // not waitUntilExit(): it polls the run loop and adds a tenth of a second to every call
        let done = DispatchSemaphore(value: 0)
        p.terminationHandler = { _ in done.signal() }
        try? p.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        done.wait()
        return String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    @discardableResult private func git(_ dir: String, _ args: String) -> String { sh("git -C '\(dir)' \(args)") }

    private func config(_ dir: String) {
        git(dir, "config user.email t@t")
        git(dir, "config user.name t")
        git(dir, "config commit.gpgsign false")
    }

    private func write(_ dir: String, _ file: String, _ text: String) {
        try? text.write(toFile: dir + "/" + file, atomically: true, encoding: .utf8)
    }

    /// A branch on the worker with one commit on top of main; returns its sha.
    @discardableResult private func workerBranch(_ name: String, file: String = "f.txt", text: String = "base\nwork\n",
                                                 tag: String? = nil) -> String {
        git(machinePath, "checkout -q -b '\(name)' main")
        write(machinePath, file, text)
        git(machinePath, "add -A")
        git(machinePath, "commit -q -m 'work on \(name)'")
        if let tag { git(machinePath, "tag '\(tag)'") }
        let sha = git(machinePath, "rev-parse HEAD")
        git(machinePath, "checkout -q main")
        return sha
    }

    private func repo(check: String = "true") -> MachineRepo {
        MachineRepo(github: "acme/app", localPath: mac, checkCommand: check)
    }

    private func machine(_ repo: MachineRepo) -> Machine {
        Machine(name: "m1", host: "m1.tail", user: "agent", sdPath: "/sd.py", fileVault: false, repos: [repo])
    }

    private func entry(_ issue: Int, branch: String, commit: String) -> ReadyEntry {
        ReadyEntry(repo: "acme/app", issue: issue, branch: branch, commit: commit, worktree: "/w", evidenceDir: "", readyAt: nil)
    }

    private func merge(_ e: ReadyEntry, check: String = "true") async -> MergeOutcome {
        let r = repo(check: check)
        return await MergePlan.merge(e, machine: machine(r), repo: r, runner: runner)
    }

    private func undo(_ e: ReadyEntry) async -> MergeOutcome {
        let r = repo()
        return await MergePlan.undoMerge(e, machine: machine(r), repo: r, runner: runner)
    }

    private func retry(_ e: ReadyEntry) async -> MergeOutcome {
        let r = repo()
        return await MergePlan.retryPush(e, machine: machine(r), repo: r, runner: runner)
    }

    private var originMain: String { git(origin, "rev-parse refs/heads/main") }
    private var macMain: String { git(mac, "rev-parse refs/heads/main") }
    private var originSubjects: String { git(origin, "log --format=%s refs/heads/main").replacingOccurrences(of: "\n", with: "|") }
    private var originFiles: String { git(origin, "ls-tree -r --name-only refs/heads/main").replacingOccurrences(of: "\n", with: "|") }

    private func assertNothingReachedOrigin(_ before: String, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(originMain, before, "origin/main moved", file: file, line: line)
        XCTAssertFalse(runner.calls.contains { $0.contains("push") }, "something was pushed", file: file, line: line)
        XCTAssertFalse(runner.calls.contains { URL(fileURLWithPath: $0[0]).lastPathComponent == "gh" }, "gh was called", file: file, line: line)
    }

    // MARK: Test isolation

    func testTheRunnerRefusesAnythingThatNamesTheRealLibraryFolder() async {
        var seen: [String] = []
        runner.onViolation = { seen.append($0) }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let never = "\(home)/Library/Application Support/Notchling/merge/never-made-\(UUID().uuidString)"
        let refused: [([String], String?)] = [
            (["/bin/mkdir", "-p", never], nil),
            (["/usr/bin/git", "-C", never, "worktree", "add", never, "main"], nil),
            (["/bin/echo", "x", "\(home)/Library/Caches/Notchling/machines/m1/7"], nil),
            (["/bin/pwd"], "\(home)/Library"),
        ]
        for (argv, cwd) in refused {
            let r = await runner.run(argv, cwd: cwd, timeout: 5)
            XCTAssertEqual(r.code, 126, "\(argv)")
        }
        XCTAssertEqual(seen.count, refused.count)
        XCTAssertTrue(seen.allSatisfy { $0.contains("test isolation") })
        XCTAssertFalse(FileManager.default.fileExists(atPath: never))

        // a temp folder is fine
        seen = []
        let ok = await runner.run(["/bin/echo", "hi"], cwd: root, timeout: 5)
        XCTAssertEqual(ok.out, "hi\n")
        XCTAssertTrue(seen.isEmpty)
    }

    // MARK: Happy path

    func testHappyPathPushesOnlyMainAndLeavesNoTags() async {
        let commit = workerBranch("sd/issue-7", tag: "v-from-the-machine")
        let before = originMain
        let o = await merge(entry(7, branch: "sd/issue-7", commit: commit))
        XCTAssertNil(o.error)
        XCTAssertTrue(o.fetched && o.merged && o.checked && o.pushed && o.closed)
        XCTAssertEqual(originMain, macMain)
        XCTAssertNotEqual(originMain, before)
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^2"), commit)
        XCTAssertEqual(git(origin, "for-each-ref --format='%(refname)'"), "refs/heads/main", "origin holds main and nothing else")
        XCTAssertEqual(git(mac, "tag -l"), "", "no tag came from the machine")
        XCTAssertEqual(git(mac, "status --porcelain"), "")
        XCTAssertEqual(runner.ghCalls.first.map { Array($0.prefix(3)) }, ["issue", "close", "7"])
    }

    // MARK: C1: tags from a machine

    /// A tag named `origin/main` left by an agent on an unreviewed commit, brought in by the fetch behind Diff.
    func testATagFromAMachineCannotMoveOrPushMain() async {
        let evil = workerBranch("sd/issue-9", file: "evil.txt", text: "EVIL\n", tag: "origin/main")
        let good = workerBranch("sd/issue-10", file: "good.txt", text: "good\n")
        git(machinePath, "tag main \(evil)")

        // the user only looks at #9's diff and rejects it
        let item = MorningItem(id: "m1|acme/app|9", kind: .ready, machine: "m1", repo: "acme/app", issue: 9, title: "t",
                               entry: .ready(entry(9, branch: "sd/issue-9", commit: evil)), comment: nil)
        let r = repo()
        let actions = MorningActions(runner: runner, machines: { [self] in [machine(r)] },
                                     cacheRoot: URL(fileURLWithPath: root + "/cache"))
        let diff = await actions.diff(item)
        XCTAssertTrue(diff.contains("EVIL"), "the diff still shows the branch: \(diff)")
        XCTAssertEqual(git(mac, "tag -l"), "", "the diff fetch planted a tag from the machine")

        // days later #10 is merged
        let o = await merge(entry(10, branch: "sd/issue-10", commit: good))
        XCTAssertNil(o.error)
        XCTAssertTrue(o.pushed)
        XCTAssertEqual(git(mac, "tag -l"), "")
        XCTAssertFalse(originFiles.contains("evil.txt"), "the unreviewed commit reached origin: \(originSubjects)")
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^2"), good)
        XCTAssertEqual(originSubjects.components(separatedBy: "|").count, 3, "merge, good, init: \(originSubjects)")
    }

    /// The reviewer's exact case: only a tag named `origin/main`, left on a commit nobody merged.
    func testAnOriginMainTagFromAMachineDoesNotPushAnUnreviewedCommit() async {
        let evil = workerBranch("sd/issue-9", file: "evil.txt", text: "EVIL\n", tag: "origin/main")
        let good = workerBranch("sd/issue-10", file: "good.txt", text: "good\n")
        let item = MorningItem(id: "m1|acme/app|9", kind: .ready, machine: "m1", repo: "acme/app", issue: 9, title: "t",
                               entry: .ready(entry(9, branch: "sd/issue-9", commit: evil)), comment: nil)
        let r = repo()
        let actions = MorningActions(runner: runner, machines: { [self] in [machine(r)] },
                                     cacheRoot: URL(fileURLWithPath: root + "/cache"))
        _ = await actions.diff(item)
        let o = await merge(entry(10, branch: "sd/issue-10", commit: good))
        XCTAssertNil(o.error)
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^2"), good)
        XCTAssertFalse(originFiles.contains("evil.txt"), "an unreviewed commit reached origin: \(originSubjects)")
    }

    /// Tags that are already in the user's repo (left by an older fetch) must not redirect a short name.
    func testTagsAlreadyInTheRepoDoNotRedirectMainOrOriginMain() async {
        git(mac, "checkout -q -b scratch")
        write(mac, "evil.txt", "EVIL\n")
        git(mac, "add -A")
        git(mac, "commit -q -m evil")
        let evil = git(mac, "rev-parse HEAD")
        git(mac, "checkout -q main")
        git(mac, "tag origin/main \(evil)")
        git(mac, "tag main \(evil)")
        git(mac, "branch -q -D scratch")
        let commit = workerBranch("sd/issue-7")

        let o = await merge(entry(7, branch: "sd/issue-7", commit: commit))
        XCTAssertNil(o.error)
        XCTAssertTrue(o.pushed)
        XCTAssertFalse(originFiles.contains("evil.txt"), originSubjects)
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^2"), commit)
    }

    /// A tag on the machine with the branch's own name must not stand in for the branch.
    func testATagWithTheBranchsNameOnTheMachineIsNotTheBranch() async {
        let evil = workerBranch("evil-side", file: "evil.txt", text: "EVIL\n")
        let commit = workerBranch("sd/issue-7")
        git(machinePath, "tag sd/issue-7 \(evil)")
        let o = await merge(entry(7, branch: "sd/issue-7", commit: commit))
        XCTAssertNil(o.error)
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^2"), commit)
        XCTAssertFalse(originFiles.contains("evil.txt"))
    }

    // MARK: Branch names

    func testBranchNamesGitWouldReadAsOptionsOrCannotBeRefsAreRefused() async {
        let commit = workerBranch("sd/issue-7")
        let before = originMain
        for bad in ["-x", "--upload-pack=touch /tmp/pwned", "a..b", "x y", "a~1", "", "@{-1}", "feature/"] {
            runner.clear()
            let o = await merge(entry(7, branch: bad, commit: commit))
            XCTAssertNotNil(o.error, "'\(bad)' was accepted")
            XCTAssertFalse(o.fetched, "'\(bad)' reached the fetch")
            XCTAssertFalse(o.merged)
            XCTAssertFalse(runner.calls.contains { $0.contains { $0.hasPrefix("ssh://") } }, "'\(bad)' reached the machine")
        }
        XCTAssertEqual(originMain, before)
        XCTAssertEqual(macMain, before)
    }

    // MARK: I1: a failed check

    func testFailedCheckWhoseResetFailsIsAHardErrorWithNoRetry() async {
        let commit = workerBranch("sd/issue-7")
        let before = originMain
        let o = await merge(entry(7, branch: "sd/issue-7", commit: commit),
                            check: "touch \"$(git rev-parse --git-dir)/index.lock\"; echo tests failed; exit 1")
        sh("rm -f '\(mac)/.git/index.lock'")
        XCTAssertTrue(o.error?.hasPrefix("check failed and main could not be put back; fix it by hand in \(mac)") ?? false, o.error ?? "nil error")
        XCTAssertTrue(o.merged, "the merge is still on main")
        XCTAssertFalse(o.checked)
        XCTAssertFalse(o.canRetryPush, "Retry push would push a merge whose check failed")
        XCTAssertFalse(o.pushed)
        assertNothingReachedOrigin(before)
    }

    func testFailedCheckPutsMainBack() async {
        let commit = workerBranch("sd/issue-7")
        let before = originMain
        let o = await merge(entry(7, branch: "sd/issue-7", commit: commit), check: "echo tests failed; exit 1")
        XCTAssertTrue(o.error?.contains("check failed") ?? false)
        XCTAssertTrue(o.error?.contains("tests failed") ?? false)
        XCTAssertFalse(o.merged)
        XCTAssertFalse(o.canRetryPush)
        XCTAssertFalse(o.canUndo)
        XCTAssertEqual(macMain, before)
        XCTAssertEqual(git(mac, "status --porcelain"), "")
        assertNothingReachedOrigin(before)
    }

    // MARK: I3: an edit during the check

    func testEditDuringTheCheckBlocksThePushAndKeepsTheMergeLocal() async {
        let commit = workerBranch("sd/issue-7")
        let before = originMain
        let e = entry(7, branch: "sd/issue-7", commit: commit)
        let o = await merge(e, check: "printf 'user edit\\n' >> f.txt; true")
        XCTAssertEqual(o.error, "main has local changes made during the check; nothing pushed")
        XCTAssertTrue(o.merged)
        XCTAssertFalse(o.pushed)
        XCTAssertFalse(o.canRetryPush, "the check ran on a tree that is not the commit to push")
        XCTAssertTrue(o.canUndo)
        assertNothingReachedOrigin(before)
        XCTAssertEqual(git(mac, "status --porcelain"), "M f.txt")

        // undo waits for the user's edit to go; it never discards it
        let refused = await undo(e)
        XCTAssertTrue(refused.error?.contains("local changes") ?? false, refused.error ?? "nil error")
        XCTAssertEqual(git(mac, "rev-parse HEAD^2"), commit)

        git(mac, "checkout -q -- f.txt")
        let done = await undo(e)
        XCTAssertNil(done.error)
        XCTAssertEqual(macMain, before)
        XCTAssertEqual(git(mac, "status --porcelain"), "")
    }

    // MARK: I4: a way out when origin moved, or after a quit

    private func pushFromAnotherClone(once marker: String) -> String {
        "[ -e '\(root)/\(marker)' ] || { touch '\(root)/\(marker)'; rm -rf '\(root)/other'; git clone -q '\(origin)' '\(root)/other' 2>/dev/null; cd '\(root)/other'; "
            + "git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 'someone else'; git push -q origin main; }; true"
    }

    func testPushRejectedBecauseOriginMovedThenUndoAndMergeAgain() async {
        let commit = workerBranch("sd/issue-7")
        let e = entry(7, branch: "sd/issue-7", commit: commit)
        let check = pushFromAnotherClone(once: "pushed-once")
        let before = originMain

        let first = await merge(e, check: check)
        XCTAssertTrue(first.error?.hasPrefix("push failed") ?? false, first.error ?? "nil error")
        XCTAssertTrue(first.merged && first.checked && !first.pushed)
        XCTAssertTrue(first.canRetryPush)
        XCTAssertTrue(first.canUndo)
        XCTAssertNotEqual(originMain, before, "someone else's push is on origin")
        let theirs = originMain

        // retrying the push cannot work while origin has moved
        let again = await retry(e)
        XCTAssertFalse(again.pushed)
        XCTAssertEqual(originMain, theirs)

        // main goes back to where it was, then the merge runs again on top of the new origin
        let undone = await undo(e)
        XCTAssertNil(undone.error, undone.error ?? "")
        XCTAssertEqual(macMain, before)
        XCTAssertEqual(git(mac, "status --porcelain"), "")

        let second = await merge(e, check: check)
        XCTAssertNil(second.error, second.error ?? "")
        XCTAssertTrue(second.pushed && second.closed)
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^1"), theirs)
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^2"), commit)
    }

    /// Notchling quit during the check: main holds this item's unchecked merge and the outcome is gone.
    func testMainAheadByThisItemsMergeIsReportedAndCanBeUndone() async {
        let commit = workerBranch("sd/issue-7")
        let e = entry(7, branch: "sd/issue-7", commit: commit)
        let before = originMain
        git(mac, "fetch -q --no-tags '\(machinePath)' '+refs/heads/sd/issue-7:refs/machines/m1/7'")
        git(mac, "merge -q --no-ff -m 'Merge sd/issue-7 (acme/app#7)' refs/machines/m1/7")
        let local = macMain

        let o = await merge(e)
        XCTAssertTrue(o.error?.contains("merged locally, not pushed") ?? false, o.error ?? "nil error")
        XCTAssertTrue(o.merged)
        XCTAssertFalse(o.checked)
        XCTAssertFalse(o.pushed)
        XCTAssertEqual(o.commit, local)
        XCTAssertFalse(o.canRetryPush, "the check never ran on this merge")
        XCTAssertTrue(o.canUndo)
        assertNothingReachedOrigin(before)
        XCTAssertEqual(macMain, local)

        let undone = await undo(e)
        XCTAssertNil(undone.error, undone.error ?? "")
        XCTAssertEqual(macMain, before)

        runner.clear()
        let second = await merge(e)
        XCTAssertNil(second.error)
        XCTAssertTrue(second.pushed)
    }

    func testUnpushedCommitsOfTheUsersOwnAreNotMergedOntoOrPushed() async {
        let commit = workerBranch("sd/issue-7")
        let e = entry(7, branch: "sd/issue-7", commit: commit)
        let before = originMain
        write(mac, "mine.txt", "mine\n")
        git(mac, "add -A")
        git(mac, "commit -q -m 'my own work'")
        let mine = macMain

        let o = await merge(e)
        XCTAssertEqual(o.error, "main has 1 commit(s) that are not on origin; push or undo them yourself")
        XCTAssertFalse(o.merged)
        XCTAssertFalse(o.canUndo)
        assertNothingReachedOrigin(before)
        XCTAssertEqual(macMain, mine)

        let refused = await undo(e)
        XCTAssertNotNil(refused.error, "undo must not touch commits that are not a merge of this branch")
        XCTAssertEqual(macMain, mine)
    }

    // MARK: Undo guards

    /// Makes main hold a local merge of `branch`, returns the merge commit.
    private func localMerge(_ branch: String, issue: Int) -> String {
        git(mac, "fetch -q --no-tags '\(machinePath)' '+refs/heads/\(branch):refs/machines/m1/\(issue)'")
        git(mac, "merge -q --no-ff -m 'Merge \(branch)' refs/machines/m1/\(issue)")
        return macMain
    }

    func testUndoRefusesWhenTheMergeIsNotThisItemsOrThereIsMoreOnTop() async {
        let seven = workerBranch("sd/issue-7")
        let eight = workerBranch("sd/issue-8", file: "g.txt", text: "g\n")
        let e7 = entry(7, branch: "sd/issue-7", commit: seven)

        // HEAD is another branch's merge
        let mergeOfEight = localMerge("sd/issue-8", issue: 8)
        let wrong = await undo(e7)
        XCTAssertNotNil(wrong.error)
        XCTAssertEqual(macMain, mergeOfEight)

        // this item's merge sits under a commit of the user's
        let e8 = entry(8, branch: "sd/issue-8", commit: eight)
        write(mac, "mine.txt", "mine\n")
        git(mac, "add -A")
        git(mac, "commit -q -m 'my own work'")
        let top = macMain
        let notHead = await undo(e8)
        XCTAssertNotNil(notHead.error)
        XCTAssertEqual(macMain, top)
    }

    func testUndoRefusesWhenOtherUnpushedCommitsSitUnderTheMerge() async {
        let seven = workerBranch("sd/issue-7")
        write(mac, "mine.txt", "mine\n")
        git(mac, "add -A")
        git(mac, "commit -q -m 'my own work'")
        let merged = localMerge("sd/issue-7", issue: 7)
        let o = await undo(entry(7, branch: "sd/issue-7", commit: seven))
        XCTAssertNotNil(o.error, "two commits ahead of origin: the user's work would be lost")
        XCTAssertEqual(macMain, merged)
    }

    func testUndoRefusesAThreeParentHead() async {
        let seven = workerBranch("sd/issue-7")
        let eight = workerBranch("sd/issue-8", file: "g.txt", text: "g\n")
        let e7 = entry(7, branch: "sd/issue-7", commit: seven)
        // (a Mac that is not on main would get Notchling's own worktree, so that case is a fake-runner test)

        git(mac, "fetch -q --no-tags '\(machinePath)' '+refs/heads/sd/issue-7:refs/machines/m1/7' '+refs/heads/sd/issue-8:refs/machines/m1/8'")
        git(mac, "merge -q --no-ff -m octopus refs/machines/m1/7 refs/machines/m1/8")
        XCTAssertEqual(git(mac, "rev-list --parents -n 1 HEAD").split(separator: " ").count, 4, "a 3-parent merge")
        let octopus = macMain
        let o = await undo(e7)
        XCTAssertNotNil(o.error)
        XCTAssertEqual(macMain, octopus)
        _ = eight
    }

    func testUndoGoesBackToTheFirstParentAndKeepsOriginUntouched() async {
        let seven = workerBranch("sd/issue-7")
        let before = originMain
        _ = localMerge("sd/issue-7", issue: 7)
        let o = await undo(entry(7, branch: "sd/issue-7", commit: seven))
        XCTAssertNil(o.error, o.error ?? "")
        XCTAssertFalse(o.merged)
        XCTAssertEqual(macMain, before)
        XCTAssertEqual(git(mac, "status --porcelain"), "")
        assertNothingReachedOrigin(before)
    }

    // MARK: Smaller fixes

    /// The tip is merged by its sha, so nothing that rewrites the private ref after the check matters.
    func testMergeUsesTheVerifiedCommitEvenIfTheRefIsRewrittenMeanwhile() async {
        let commit = workerBranch("sd/issue-7")
        let other = git(mac, "commit-tree 'HEAD^{tree}' -p HEAD -m other")
        runner.afterCall = { [mac] argv in
            if argv.contains("rev-parse"), argv.last == "refs/machines/m1/7" {
                self.sh("git -C '\(mac)' update-ref refs/machines/m1/7 \(other)")
            }
        }
        let o = await merge(entry(7, branch: "sd/issue-7", commit: commit))
        XCTAssertNil(o.error)
        XCTAssertEqual(git(origin, "rev-parse refs/heads/main^2"), commit, "the other commit was merged")
    }

    /// `merge --abort` says nothing is in progress when the merge never started; the merge's own error must show.
    func testAMergeThatNeverStartsReportsItsOwnError() async {
        git(machinePath, "checkout -q --orphan unrelated")
        git(machinePath, "rm -rf -q .")
        write(machinePath, "z.txt", "z\n")
        git(machinePath, "add -A")
        git(machinePath, "commit -q -m unrelated")
        let commit = git(machinePath, "rev-parse HEAD")
        git(machinePath, "checkout -q main")
        let before = originMain
        let o = await merge(entry(7, branch: "unrelated", commit: commit))
        XCTAssertTrue(o.error?.contains("unrelated histories") ?? false, o.error ?? "nil error")
        XCTAssertFalse(o.error?.contains("aborted") ?? true)
        XCTAssertTrue(o.conflictFiles.isEmpty)
        XCTAssertFalse(o.merged)
        assertNothingReachedOrigin(before)
        XCTAssertEqual(git(mac, "status --porcelain"), "")
    }

    func testARealConflictStillAbortsAndNamesTheFiles() async {
        let commit = workerBranch("sd/issue-7", text: "base\nfrom the machine\n")
        write(mac, "f.txt", "base\nfrom the mac\n")
        git(mac, "commit -q -am 'mac edit'")
        git(mac, "push -q origin main")
        let before = originMain
        let o = await merge(entry(7, branch: "sd/issue-7", commit: commit))
        XCTAssertEqual(o.conflictFiles, ["f.txt"])
        XCTAssertTrue(o.error?.contains("merge conflict in f.txt") ?? false)
        XCTAssertEqual(git(mac, "status --porcelain"), "", "the conflict was aborted")
        XCTAssertEqual(macMain, before)
        XCTAssertEqual(runner.ghCalls.map { Array($0.prefix(2)) }, [["issue", "comment"], ["issue", "edit"]])
    }
}

/// The real `ProcessRunner` for git and the shell, with the `ssh://` URL of the worker mapped to its folder,
/// `gh` faked, and the login-shell check run with `/bin/sh`.
private final class LocalRunner: CommandRunning, @unchecked Sendable {
    /// Keeps the developer's own git config (signing, hooks, followTags) out of the repos.
    static let gitEnvironment = ["GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
                                 "GIT_TERMINAL_PROMPT": "0", "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
                                 "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"]

    private let machinePath: String
    private let lock = NSLock()
    private var _calls: [[String]] = []
    /// Runs after each call, before its result is returned: a way to change the world between two steps.
    var afterCall: (([String]) -> Void)?
    /// Called, instead of running the command, when one would touch the real `~/Library` (the app's own worktree folder
    /// and caches live there). A test must never create files in the user's real data: this fails the test loudly.
    var onViolation: (String) -> Void = { XCTFail($0) }

    /// The real home's Library folder, spelled both ways (git prints real paths).
    static var libraryFolders: [String] {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let real = realpath(home, nil).map { String(cString: $0) } ?? home
        return Array(Set([home + "/Library", real + "/Library"]))
    }

    private func violation(_ argv: [String], _ cwd: String?) -> String? {
        for text in argv + [cwd].compactMap({ $0 }) {
            if let folder = Self.libraryFolders.first(where: { text.contains($0) }) {
                return "test isolation: refused \(argv.joined(separator: " ")) (cwd \(cwd ?? "none")): it names \(folder), the real user's data"
            }
        }
        return nil
    }

    init(machinePath: String) { self.machinePath = machinePath }

    var calls: [[String]] { lock.withLock { _calls } }
    var ghCalls: [[String]] {
        calls.filter { URL(fileURLWithPath: $0[0]).lastPathComponent == "gh" }.map { Array($0.dropFirst()) }
    }
    func clear() { lock.withLock { _calls = [] } }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], timeout: timeout)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        lock.withLock { _calls.append(argv) }
        if let why = violation(argv, cwd) {
            onViolation(why)
            return CommandResult(code: 126, out: "", err: why)
        }
        var a = argv.map { $0.hasPrefix("ssh://") ? machinePath : $0 }
        if URL(fileURLWithPath: a[0]).lastPathComponent == "gh" { return CommandResult(code: 0, out: "", err: "") }
        // the check runs through the user's login shell in the app; the test runs it with sh
        if a.count == 3, a[1] == "-lc" { a = ["/bin/sh", "-c", a[2]] }
        let r = await ProcessRunner().run(a, cwd: cwd, env: env.merging(Self.gitEnvironment) { $1 }, timeout: timeout)
        afterCall?(a)
        return r
    }
}
