import XCTest
@testable import Notchling

/// Records every argv and answers from a closure, so no test starts gh, git, ssh or scp.
private final class MorningFakeRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    private var _calls: [[String]] = []
    var answer: ([String]) -> CommandResult

    init(_ answer: @escaping ([String]) -> CommandResult = { _ in CommandResult(code: 0, out: "", err: "") }) {
        self.answer = answer
    }

    var calls: [[String]] { lock.withLock { _calls } }

    /// The calls to `gh`, without the path of the executable.
    var ghCalls: [[String]] {
        calls.filter { URL(fileURLWithPath: $0[0]).lastPathComponent == "gh" }.map { Array($0.dropFirst()) }
    }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        lock.withLock { _calls.append(argv) }
        return answer(argv)
    }
}

/// Holds an action open until the test lets it go.
private actor Gate {
    private var isOpen = false
    private var waiting: [CheckedContinuation<Void, Never>] = []
    func wait() async {
        if isOpen { return }
        await withCheckedContinuation { waiting.append($0) }
    }
    func open() {
        isOpen = true
        waiting.forEach { $0.resume() }
        waiting = []
    }
}

/// Answers at once, except a fetch, which waits for the gate.
private final class GatedRunner: CommandRunning, @unchecked Sendable {
    let gate = Gate()
    let entered = Counter()
    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        if argv.contains("fetch") {
            entered.bump()
            await gate.wait()
        }
        return CommandResult(code: 0, out: argv.contains("check-ref-format") ? "sd/7\n" : "", err: "")
    }
}

private final class Counter: @unchecked Sendable {
    private let lock = NSLock()
    private var n = 0
    func bump() { lock.withLock { n += 1 } }
    var value: Int { lock.withLock { n } }
}

@MainActor
final class MachinesMorningTests: XCTestCase {
    private let repo = "acme/app"
    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    private func machine(_ name: String = "m1") -> Machine {
        Machine(name: name, host: "\(name).tail", user: "agent", sdPath: "/sd/sd.py", fileVault: false,
                repos: [MachineRepo(github: repo, localPath: "/work/app", checkCommand: "just check")])
    }

    private func ready(_ n: Int, at offset: TimeInterval? = nil) -> ReadyEntry {
        ReadyEntry(repo: repo, issue: n, branch: "sd/\(n)", commit: "abc", worktree: "/wt/\(n)",
                   evidenceDir: "/wt/\(n)/evidence", readyAt: offset.map { t0.addingTimeInterval($0) })
    }

    private func item(_ kind: MorningKind, _ n: Int = 7) -> MorningItem {
        let entry: MorningEntry
        switch kind {
        case .ready: entry = .ready(ready(n))
        case .needsYou: entry = .needsYou(NeedsYouEntry(repo: repo, issue: n, reason: "question"))
        case .stuck: entry = .stuck(StuckEntry(repo: repo, issue: n, reason: "gave up"))
        }
        return MorningItem(id: MorningItems.id(machine: "m1", repo: repo, issue: n), kind: kind, machine: "m1", repo: repo,
                           issue: n, title: "t", entry: entry, comment: nil)
    }

    private func actions(_ runner: MorningFakeRunner, merge: MorningActions.MergeFunction? = nil,
                         retryPush: MorningActions.MergeFunction? = nil, undo: MorningActions.MergeFunction? = nil,
                         cache: URL? = nil) -> MorningActions {
        let cacheRoot = cache ?? FileManager.default.temporaryDirectory.appendingPathComponent("morning-\(UUID().uuidString)")
        let ok: MorningActions.MergeFunction = { _, _, _, _ in MergeOutcome(pushed: true, closed: true) }
        return MorningActions(runner: runner, machines: { [self] in [machine()] }, merge: merge ?? ok,
                              retryPush: retryPush ?? ok, undoMerge: undo ?? { _, _, _, _ in MergeOutcome() },
                              cacheRoot: cacheRoot)
    }

    // MARK: build

    func testBuildOrdersReadyNeedsStuck() {
        var status = WorkerStatus()
        status.stuck = [StuckEntry(repo: repo, issue: 4, reason: "r")]
        status.needsYou = [NeedsYouEntry(repo: repo, issue: 9, reason: "q"), NeedsYouEntry(repo: repo, issue: 3, reason: "q")]
        status.ready = [ready(5, at: 200), ready(8, at: 100), ready(6)]
        let issues = [repo: [GHIssue(number: 8, title: "Eight"), GHIssue(number: 5, title: "Five")]]

        let items = MorningItems.build(reports: ["m1": LastReport(status: status, at: t0)], issues: issues)

        XCTAssertEqual(items.map(\.id), ["m1/acme/app#8", "m1/acme/app#5", "m1/acme/app#6",
                                         "m1/acme/app#3", "m1/acme/app#9", "m1/acme/app#4"])
        XCTAssertEqual(items.map(\.kind), [.ready, .ready, .ready, .needsYou, .needsYou, .stuck])
        XCTAssertEqual(items[0].title, "Eight")
        XCTAssertEqual(items[2].title, "#6", "no matching issue: the number stands in for the title")
    }

    func testBuildSkipsMachinesWithoutStatus() {
        let items = MorningItems.build(reports: ["m1": LastReport(at: t0, error: "timed out")], issues: [:])
        XCTAssertTrue(items.isEmpty)
    }

    // MARK: GHIssue and worker comments

    func testGHIssueDecodesGhOutput() {
        let json = """
        [{"number":12,"title":"Add login","labels":[{"name":"sd:ready","color":"fff"},{"name":"bug"}],
          "comments":[{"author":{"login":"sondoan"},"body":"please","createdAt":"2026-11-05T22:10:31Z"},
                      {"author":{"login":"bot"},"body":"m1: done\\n<!-- sd:worker -->","createdAt":"2026-11-05T23:00:00Z"}]},
         {"number":13,"title":"Empty","labels":[],"comments":[]}]
        """
        let issues = GHIssue.decodeList(json)
        XCTAssertEqual(issues.map(\.number), [12, 13])
        XCTAssertEqual(issues[0].title, "Add login")
        XCTAssertEqual(issues[0].labels, ["sd:ready", "bug"])
        XCTAssertEqual(issues[0].comments.map(\.author), ["sondoan", "bot"])
        XCTAssertEqual(issues[0].comments[1].body, "m1: done\n<!-- sd:worker -->")
        XCTAssertEqual(issues[0].comments[0].createdAt, StatusDates.parse("2026-11-05T22:10:31Z"))
        XCTAssertTrue(GHIssue.decodeList("not json").isEmpty)
    }

    func testWorkerCommentSelectionAndCleaning() {
        let issue = GHIssue(number: 1, title: "t", comments: [
            .init(body: "m1: first worker note\n<!-- sd:worker -->"),
            .init(body: "notchling: use recommended"),
            .init(body: "Verified.\n\n<!-- sd:kind=ready -->\nAll green.\n<!-- sd:worker -->  \n"),
            .init(body: "a human reply after"),
        ])
        XCTAssertEqual(MorningItems.latestWorkerComment(issue, machine: "m1"), "Verified.\n\n\nAll green.")

        let prefixed = GHIssue(number: 2, title: "t", comments: [.init(body: "m1: which database?"), .init(body: "thanks")])
        XCTAssertEqual(MorningItems.latestWorkerComment(prefixed, machine: "m1"), "m1: which database?")
        XCTAssertNil(MorningItems.latestWorkerComment(prefixed, machine: "m2"), "another machine's prefix does not count")
        XCTAssertNil(MorningItems.latestWorkerComment(GHIssue(comments: [.init(body: "<!-- sd:worker -->")]), machine: "m1"),
                     "only marks: nothing to show")
    }

    func testBuildAttachesTheComment() {
        var status = WorkerStatus()
        status.needsYou = [NeedsYouEntry(repo: repo, issue: 2, reason: "q")]
        let issues = [repo: [GHIssue(number: 2, title: "Q", comments: [.init(body: "Which one?\n<!-- sd:worker -->")])]]
        let items = MorningItems.build(reports: ["m1": LastReport(status: status, at: t0)], issues: issues)
        XCTAssertEqual(items.first?.comment, "Which one?")
    }

    func testFetchIssuesAsksEveryLabelForEveryRepo() async {
        let runner = MorningFakeRunner { argv in
            let label = argv[argv.firstIndex(of: "--label")! + 1]
            let n = ["sd:ready": 1, "sd:needs-you": 2, "sd:stuck": 3][label]!
            return CommandResult(code: 0, out: "[{\"number\":\(n),\"title\":\"T\(n)\",\"labels\":[],\"comments\":[]}]", err: "")
        }
        let issues = await MorningItems.fetchIssues(repos: [repo, repo], runner: runner)
        XCTAssertEqual(issues[repo]?.map(\.number), [1, 2, 3])
        let calls = runner.ghCalls
        XCTAssertEqual(calls.count, 3)
        XCTAssertEqual(calls[0], ["issue", "list", "-R", repo, "--label", "sd:ready", "--json",
                                  "number,title,labels,comments", "--limit", "100"])
    }

    func testFetchNamesTheReposWhoseGhCallFailedAndKeepsTheirOldIssues() async {
        let runner = MorningFakeRunner { argv in
            argv.contains("sd:needs-you") ? CommandResult(code: 1, out: "", err: "network") :
                CommandResult(code: 0, out: "[{\"number\":1,\"title\":\"New\",\"labels\":[],\"comments\":[]}]", err: "")
        }
        let result = await MorningItems.fetchIssuesChecked(repos: [repo], runner: runner)
        XCTAssertEqual(result.failed, [repo])
        let old = [repo: [GHIssue(number: 1, title: "Old"), GHIssue(number: 2, title: "Two")]]
        XCTAssertEqual(MorningItems.merged(old: old, fetched: result.issues, failed: result.failed)[repo]?.map(\.title), ["Old", "Two"])
        // a repo that fetched fine takes the new issues
        XCTAssertEqual(MorningItems.merged(old: old, fetched: result.issues, failed: [])[repo]?.map(\.title), ["New"])
        // nothing known before: what the fetch got is better than nothing
        XCTAssertEqual(MorningItems.merged(old: [:], fetched: result.issues, failed: [repo])[repo]?.map(\.title), ["New"])
    }

    // MARK: actions

    func testStaleOutcomeOfAnotherCommitShowsNothing() async {
        let a = actions(MorningFakeRunner(), merge: { _, _, _, _ in MergeOutcome(fetched: true, merged: true, checked: true, error: "push rejected") })
        let first = item(.ready)
        await a.merge(first).value
        XCTAssertEqual(a.outcome(for: first)?.merged, true, "the outcome of this commit shows (checklist and Retry push)")

        // the worker returns the branch with a new commit under the same item id
        var entry = ready(7)
        entry.commit = "def"
        let returned = MorningItem(id: first.id, kind: .ready, machine: "m1", repo: repo, issue: 7, title: "t",
                                   entry: .ready(entry), comment: nil)
        XCTAssertNil(a.outcome(for: returned), "no checklist and no Retry push for a commit that was never merged")

        a.pruneOutcomes(keeping: [returned])
        XCTAssertNil(a.outcomes[first.id], "the stale outcome is dropped")
        await a.merge(first).value
        XCTAssertNotNil(a.outcome(for: first))
        a.pruneOutcomes(keeping: [])
        XCTAssertTrue(a.outcomes.isEmpty, "an item that left the list loses its outcome")
    }

    func testOneActionPerItem() async {
        let runner = MorningFakeRunner()
        let counter = Counter()
        let slow: MorningActions.MergeFunction = { _, _, _, _ in
            counter.bump()
            try? await Task.sleep(nanoseconds: 100_000_000)
            return MergeOutcome(pushed: true, closed: true)
        }
        let a = actions(runner, merge: slow)
        let it = item(.ready)
        let first = a.merge(it)
        let second = a.merge(it)
        XCTAssertTrue(a.running.contains(it.id))
        await first.value
        await second.value
        XCTAssertEqual(counter.value, 1, "the second merge on the same item was ignored")
        XCTAssertTrue(a.running.isEmpty)
        XCTAssertEqual(a.outcomes[it.id], MergeOutcome(pushed: true, closed: true))
        XCTAssertNil(a.lastError[it.id])

        // a different item is not blocked, and the first can run again once it has finished
        await a.merge(item(.ready, 8)).value
        await a.merge(it).value
        XCTAssertEqual(counter.value, 3)
    }

    func testFailedActionFreesTheItemAndKeepsTheError() async {
        let runner = MorningFakeRunner()
        let a = actions(runner, merge: { _, _, _, _ in MergeOutcome(error: "main has local changes in /work/app") })
        let it = item(.ready)
        await a.merge(it).value
        XCTAssertFalse(a.running.contains(it.id))
        XCTAssertEqual(a.lastError[it.id], "main has local changes in /work/app")

        let b = actions(MorningFakeRunner { _ in CommandResult(code: 1, out: "", err: "HTTP 404\n") })
        await b.retry(item(.stuck)).value
        XCTAssertTrue(b.running.isEmpty)
        XCTAssertEqual(b.lastError[item(.stuck).id], "HTTP 404")
    }

    func testMergeWithAPushThatFailedOffersARetry() async {
        let runner = MorningFakeRunner()
        let calls = Counter()
        let ok: MorningActions.MergeFunction = { _, _, _, _ in MergeOutcome(pushed: true, closed: true) }
        let a = MorningActions(runner: runner, machines: { [self] in [machine()] },
                               merge: { _, _, _, _ in MergeOutcome(merged: true, checked: true, pushed: false) },
                               retryPush: { e, _, _, _ in calls.bump(); return await ok(e, self.machine(), self.machine().repos[0], runner) },
                               cacheRoot: FileManager.default.temporaryDirectory)
        let it = item(.ready)
        await a.merge(it).value
        XCTAssertEqual(a.lastError[it.id], "main was not pushed")
        await a.retryPush(it).value
        XCTAssertEqual(calls.value, 1)
        XCTAssertNil(a.lastError[it.id], "a new action clears the old error")
    }

    /// C17: a merge whose check did not pass is never pushed by a retry, whatever the window shows.
    func testRetryPushRefusesAMergeThatWasNeverChecked() async {
        let calls = Counter()
        let retry: MorningActions.MergeFunction = { _, _, _, _ in calls.bump(); return MergeOutcome(pushed: true, closed: true) }
        for outcome in [MergeOutcome(merged: true, checked: false, error: "check failed and main could not be put back; fix it by hand in /work/app"),
                        MergeOutcome(merged: true, checked: false, error: "main has local changes made during the check; nothing pushed"),
                        MergeOutcome(error: "main has local changes in /work/app")] {
            let a = actions(MorningFakeRunner(), merge: { _, _, _, _ in outcome }, retryPush: retry)
            let it = item(.ready)
            await a.merge(it).value
            await a.retryPush(it).value
            XCTAssertEqual(calls.value, 0, "\(outcome)")
            XCTAssertNotNil(a.lastError[it.id])
            XCTAssertFalse(a.running.contains(it.id))
        }
        // and with nothing merged at all
        let b = actions(MorningFakeRunner(), retryPush: retry)
        await b.retryPush(item(.ready)).value
        XCTAssertEqual(calls.value, 0)
    }

    // MARK: undo merge

    func testUndoMergeForgetsTheOutcomeAndLeavesANote() async {
        let seen = Counter()
        let a = actions(MorningFakeRunner(),
                        merge: { _, _, _, _ in MergeOutcome(fetched: true, merged: true, checked: true, commit: "m", error: "push failed: rejected") },
                        undo: { e, m, r, _ in
                            seen.bump()
                            XCTAssertEqual(e.issue, 7)
                            XCTAssertEqual(m.name, "m1")
                            XCTAssertEqual(r.github, "acme/app")
                            return MergeOutcome()
                        })
        let it = item(.ready)
        await a.merge(it).value
        XCTAssertEqual(a.outcome(for: it)?.canUndo, true)
        await a.undoMerge(it).value
        XCTAssertEqual(seen.value, 1)
        XCTAssertNil(a.outcome(for: it), "no checklist, no Retry push, no Undo for a merge that is gone")
        XCTAssertNil(a.lastError[it.id])
        XCTAssertNotNil(a.notes[it.id])
        XCTAssertFalse(a.running.contains(it.id))

        // a new action clears the note; an item that left loses it
        await a.merge(it).value
        XCTAssertNil(a.notes[it.id])
        await a.undoMerge(it).value
        a.pruneOutcomes(keeping: [])
        XCTAssertTrue(a.notes.isEmpty)
    }

    func testUndoMergeThatGitRefusesKeepsTheOutcomeAndShowsTheReason() async {
        let a = actions(MorningFakeRunner(),
                        merge: { _, _, _, _ in MergeOutcome(fetched: true, merged: true, checked: true, error: "push failed: rejected") },
                        undo: { _, _, _, _ in MergeOutcome(error: "cannot undo the merge: main has local changes in /work/app; put them away first") })
        let it = item(.ready)
        await a.merge(it).value
        await a.undoMerge(it).value
        XCTAssertEqual(a.lastError[it.id], "cannot undo the merge: main has local changes in /work/app; put them away first")
        XCTAssertEqual(a.outcome(for: it)?.canUndo, true, "the merge is still there")
        XCTAssertNil(a.notes[it.id])
    }

    func testUndoMergeIsOneActionPerItemAndOnlyForAReadyBranch() async {
        let calls = Counter()
        let gate = Gate()
        let a = actions(MorningFakeRunner(), undo: { _, _, _, _ in calls.bump(); await gate.wait(); return MergeOutcome() })
        let it = item(.ready)
        let first = a.undoMerge(it)
        let second = a.undoMerge(it)
        XCTAssertTrue(a.running.contains(it.id))
        await gate.open()
        await first.value
        await second.value
        XCTAssertEqual(calls.value, 1)

        let stuck = item(.stuck)
        await a.undoMerge(stuck).value
        XCTAssertNotNil(a.lastError[stuck.id])
        XCTAssertEqual(calls.value, 1)

        let unknown = MorningActions(runner: MorningFakeRunner(), machines: { [] }, cacheRoot: FileManager.default.temporaryDirectory)
        await unknown.undoMerge(it).value
        XCTAssertNotNil(unknown.lastError[it.id])
    }

    func testSendBackLabelsAndPrefix() async {
        let runner = MorningFakeRunner()
        let a = actions(runner)
        await a.sendBack(item(.ready), note: "tighten the spacing").value
        XCTAssertEqual(runner.ghCalls, [
            ["issue", "comment", "7", "-R", repo, "--body", "notchling: tighten the spacing"],
            ["issue", "edit", "7", "-R", repo, "--add-label", "sd:queued", "--remove-label", "sd:ready"],
        ])
    }

    func testFailedCommentLeavesTheLabels() async {
        let runner = MorningFakeRunner { _ in CommandResult(code: 1, out: "", err: "rate limited") }
        let a = actions(runner)
        await a.sendBack(item(.ready), note: "x").value
        XCTAssertEqual(runner.ghCalls.count, 1, "the label edit never ran")
        XCTAssertEqual(a.lastError[item(.ready).id], "rate limited")
    }

    func testUseRecommendedSendsPhrase() async {
        let runner = MorningFakeRunner()
        let a = actions(runner)
        await a.useRecommended(item(.needsYou)).value
        XCTAssertEqual(runner.ghCalls, [
            ["issue", "comment", "7", "-R", repo, "--body", "notchling: use recommended"],
            ["issue", "edit", "7", "-R", repo, "--add-label", "sd:queued", "--remove-label", "sd:needs-you"],
        ])
    }

    func testRetryMovesStuckToQueued() async {
        let runner = MorningFakeRunner()
        let a = actions(runner)
        await a.retry(item(.stuck)).value
        XCTAssertEqual(runner.ghCalls, [["issue", "edit", "7", "-R", repo, "--add-label", "sd:queued", "--remove-label", "sd:stuck"]])
    }

    func testCloseAndDropCloseAsNotPlanned() async {
        let runner = MorningFakeRunner()
        let a = actions(runner)
        await a.close(item(.needsYou)).value
        await a.drop(item(.stuck, 9)).value
        XCTAssertEqual(runner.ghCalls, [
            ["issue", "close", "7", "-R", repo, "--reason", "not planned"],
            ["issue", "close", "9", "-R", repo, "--reason", "not planned"],
        ])
    }

    func testUnknownMachineIsAnErrorNotACrash() async {
        let a = MorningActions(runner: MorningFakeRunner(), machines: { [] }, cacheRoot: FileManager.default.temporaryDirectory)
        let it = item(.ready)
        await a.merge(it).value
        XCTAssertNotNil(a.lastError[it.id])
        XCTAssertTrue(a.running.isEmpty)
    }

    // MARK: diff and screenshots

    func testDiffFetchesWithoutTagsIntoMachinesRefThenDiffsAgainstMainByFullNames() async {
        let runner = MorningFakeRunner { argv in
            if argv.contains("check-ref-format") { return CommandResult(code: 0, out: "sd/7\n", err: "") }
            return argv.contains("diff") ? CommandResult(code: 0, out: "DIFF TEXT", err: "") : CommandResult(code: 0, out: "", err: "")
        }
        let text = await actions(runner).diff(item(.ready))
        XCTAssertEqual(text, "DIFF TEXT")
        let git = runner.calls.map { Array($0.dropFirst()) }
        XCTAssertEqual(git.count, 3)
        XCTAssertEqual(runner.calls.map { $0[0] }, Array(repeating: MachineTools.path("git"), count: 3))
        XCTAssertEqual(git[0], ["-C", "/work/app", "check-ref-format", "--branch", "sd/7"])
        XCTAssertEqual(git[1].prefix(2), ["-C", "/work/app"])
        XCTAssertTrue(git[1].contains("--no-tags"), "a fetch for a diff must not bring the machine's tags")
        XCTAssertTrue(git[1].contains("ssh://agent@m1.tail/wt/7"))
        XCTAssertTrue(git[1].contains("+refs/heads/sd/7:refs/machines/m1/7"))
        XCTAssertEqual(git[2], ["-C", "/work/app", "diff", "refs/heads/main...refs/machines/m1/7"])
    }

    func testDiffRefusesABranchNameGitWouldReadAsAnOption() async {
        let runner = MorningFakeRunner()
        var entry = ready(7)
        entry.branch = "--upload-pack=touch /tmp/x"
        let it = MorningItem(id: MorningItems.id(machine: "m1", repo: repo, issue: 7), kind: .ready, machine: "m1", repo: repo,
                             issue: 7, title: "t", entry: .ready(entry), comment: nil)
        let text = await actions(runner).diff(it)
        XCTAssertTrue(text.contains("not a branch name"), text)
        XCTAssertTrue(runner.calls.isEmpty, "nothing ran")
    }

    func testDiffRefusesABranchNameGitRejects() async {
        let runner = MorningFakeRunner { argv in
            argv.contains("check-ref-format") ? CommandResult(code: 1, out: "", err: "fatal: bad") : CommandResult(code: 0, out: "", err: "")
        }
        let text = await actions(runner).diff(item(.ready))
        XCTAssertTrue(text.contains("not a branch name"), text)
        XCTAssertEqual(runner.calls.count, 1, "only the name check ran")
    }

    /// Diff fetches into the ref a merge uses, so the two must never overlap.
    func testDiffNeverRunsWhileAnActionHasTheItem() async {
        let gate = Gate()
        let entered = Counter()
        let runner = MorningFakeRunner()
        let a = actions(runner, merge: { _, _, _, _ in
            entered.bump()
            await gate.wait()
            return MergeOutcome(pushed: true, closed: true)
        })
        let it = item(.ready)
        let merge = a.merge(it)
        while entered.value == 0 { await Task.yield() }
        let text = await a.diff(it)
        XCTAssertTrue(text.contains("action is running"), text)
        XCTAssertTrue(runner.calls.isEmpty, "the diff fetched while a merge was running")
        await gate.open()
        await merge.value
    }

    func testAMergeCannotStartWhileADiffHoldsTheItem() async {
        let runner = GatedRunner()
        let mergeCalls = Counter()
        let a = MorningActions(runner: runner, machines: { [self] in [machine()] },
                               merge: { _, _, _, _ in mergeCalls.bump(); return MergeOutcome(pushed: true, closed: true) },
                               cacheRoot: FileManager.default.temporaryDirectory)
        let it = item(.ready)
        let diff = Task { @MainActor in await a.diff(it) }
        while runner.entered.value == 0 { await Task.yield() }   // the diff is inside its fetch
        XCTAssertTrue(a.running.contains(it.id))
        await a.merge(it).value
        XCTAssertEqual(mergeCalls.value, 0, "a merge started while the diff's fetch was running")

        await runner.gate.open()
        _ = await diff.value
        XCTAssertFalse(a.running.contains(it.id), "the diff gave the item back")
        await a.merge(it).value
        XCTAssertEqual(mergeCalls.value, 1)
    }

    func testScreenshotsCopyOnlyImagesWithQuotedSources() async throws {
        let cache = FileManager.default.temporaryDirectory.appendingPathComponent("morning-shots-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: cache) }
        let runner = MorningFakeRunner { argv in
            if argv[0] == "/usr/bin/ssh" { return CommandResult(code: 0, out: "a.png\nnotes.txt\nB.JPG\nc d.png\n", err: "") }
            return CommandResult(code: argv.last!.hasSuffix("B.JPG") ? 1 : 0, out: "", err: "")
        }
        let urls = await actions(runner, cache: cache).screenshots(item(.ready))

        let dir = cache.appendingPathComponent("m1/7").path
        XCTAssertEqual(urls.map(\.path), ["\(dir)/a.png", "\(dir)/c d.png"], "B.JPG failed to copy and is left out")
        XCTAssertTrue(FileManager.default.fileExists(atPath: dir), "the cache folder was created")
        let scps = runner.calls.filter { $0[0] == "/usr/bin/scp" }
        XCTAssertEqual(scps.count, 3, "the text file is not copied; the failed copy was still tried")
        XCTAssertTrue(scps.contains { $0.contains("agent@m1.tail:'/wt/7/evidence/c d.png'") })
        for scp in scps {
            XCTAssertEqual(scp.firstIndex(of: "--").map { $0 < (scp.firstIndex { $0.hasPrefix("agent@") } ?? 0) }, true,
                           "-- ends the options before the host: \(scp)")
        }
    }
}
