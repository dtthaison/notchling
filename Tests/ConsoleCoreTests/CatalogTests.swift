import XCTest
@testable import ConsoleCore

final class CatalogTests: XCTestCase {
    private func snapshot(_ tree: FixtureTree, alive: Set<Int32> = allPids()) -> [Session] {
        SessionCatalog(paths: tree.paths, now: { fixtureNow }, isAlive: { alive.contains($0) }).snapshot()
    }

    private func prepareSubagentTimes(_ tree: FixtureTree, runningAge: TimeInterval) throws {
        let base = "claude/projects/-work-gamma/sess-parent/subagents/"
        try tree.setModified(fixtureNow.addingTimeInterval(-30 * 60), base + "agent-aaa111.jsonl")
        try tree.setModified(fixtureNow.addingTimeInterval(-runningAge), base + "agent-bbb222.jsonl")
    }

    func testSubagentTreeIsAttachedToTheRightSession() throws {
        let tree = try makeFixtures()
        try prepareSubagentTimes(tree, runningAge: 60)
        let sessions = snapshot(tree)
        let parent = try XCTUnwrap(sessions.first { $0.id == "sess-parent" })

        XCTAssertEqual(parent.subagents.map(\.id), ["bbb222", "aaa111"], "running first")
        let running = parent.subagents[0]
        XCTAssertEqual(running.type, "Explore")
        XCTAssertEqual(running.description, "Scan the repo")
        XCTAssertEqual(running.model, "sonnet")
        XCTAssertEqual(running.depth, 2)
        XCTAssertEqual(running.parentToolUseId, "toolu_BBB")
        XCTAssertFalse(running.finished)
        XCTAssertEqual(running.state, .running)
        XCTAssertEqual(running.lastActivity, fixtureNow.addingTimeInterval(-60))
        XCTAssertEqual(running.transcriptURL.lastPathComponent, "agent-bbb222.jsonl")

        let finished = parent.subagents[1]
        XCTAssertEqual(finished.type, "general-purpose")
        XCTAssertEqual(finished.model, "haiku")
        XCTAssertEqual(finished.depth, 1)
        XCTAssertEqual(finished.parentToolUseId, "toolu_AAA")
        XCTAssertTrue(finished.finished)
        XCTAssertEqual(finished.state, .done)

        for other in sessions where other.id != "sess-parent" {
            XCTAssertTrue(other.subagents.isEmpty, "\(other.id) should have no subagents")
        }
    }

    func testDoneSessionWithRunningSubagentIsRunning() throws {
        let tree = try makeFixtures()
        try prepareSubagentTimes(tree, runningAge: 60)
        XCTAssertEqual(snapshot(tree).first { $0.id == "sess-parent" }?.state, .running)
    }

    func testStaleUnfinishedSubagentCountsAsDone() throws {
        let tree = try makeFixtures()
        try prepareSubagentTimes(tree, runningAge: 20 * 60)
        let parent = try XCTUnwrap(snapshot(tree).first { $0.id == "sess-parent" })
        XCTAssertTrue(parent.subagents.allSatisfy { $0.state == .done })
        XCTAssertEqual(parent.state, .done)
    }

    func testPendingPromptFromNotchlingRequestMakesSessionNeedInput() throws {
        let tree = try makeFixtures()
        let sessions = snapshot(tree)

        let permission = try XCTUnwrap(sessions.first { $0.id == "sess-busy" })
        XCTAssertEqual(permission.state, .needsInput)
        XCTAssertEqual(permission.pending?.kind, .permission)
        XCTAssertEqual(permission.pending?.toolName, "Bash")
        XCTAssertEqual(permission.pending?.summary, "rm -rf build")

        let question = try XCTUnwrap(sessions.first { $0.id == "sess-idle" })
        XCTAssertEqual(question.state, .needsInput)
        guard case .question(let items)? = question.pending?.kind else { return XCTFail("expected a question") }
        XCTAssertEqual(items.first?.header, "Branch")
        XCTAssertEqual(items.first?.options, ["main", "develop"])
        XCTAssertEqual(question.pending?.summary, "Which branch should I use?")

        let stale = try XCTUnwrap(sessions.first { $0.id == "sess-wait" })
        XCTAssertNil(stale.pending, "a request whose hook process is gone is ignored")
    }

    func testRequestFilesAreNeverModified() throws {
        let tree = try makeFixtures()
        let directory = tree.paths.requestsDir
        let before = try FileManager.default.contentsOfDirectory(atPath: directory.path).sorted()
        _ = snapshot(tree)
        let after = try FileManager.default.contentsOfDirectory(atPath: directory.path).sorted()
        XCTAssertEqual(before, after)
    }

    func testSortOrderIsStateThenNewestActivity() throws {
        let tree = try makeFixtures()
        try prepareSubagentTimes(tree, runningAge: 60)
        let sessions = snapshot(tree)
        let states = sessions.map(\.state.rawValue)
        XCTAssertEqual(states, states.sorted())
        for state in SessionState.allCases {
            let activity = sessions.filter { $0.state == state }.map(\.lastActivity)
            XCTAssertEqual(activity, activity.sorted(by: >), "\(state) is newest first")
        }
    }

    func testSdkSessionIsExcludedFromTheCatalog() throws {
        let tree = try makeFixtures()
        XCTAssertFalse(snapshot(tree).contains { $0.id == "sess-sdk" })
    }
}
