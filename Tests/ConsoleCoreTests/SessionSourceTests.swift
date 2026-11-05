import XCTest
@testable import ConsoleCore

final class SessionSourceTests: XCTestCase {
    private func makeSource(_ tree: FixtureTree, clock: @escaping () -> Date, alive: @escaping () -> Set<Int32>) -> SessionSource {
        SessionSource(paths: tree.paths,
                      locator: TranscriptLocator(projectsDir: tree.paths.projectsDir, now: clock),
                      now: clock, isAlive: { alive().contains($0) })
    }

    func testStateMappingForSessionsAndJobs() throws {
        let tree = try makeFixtures()
        let source = makeSource(tree, clock: { fixtureNow }, alive: allPids)
        let byId = Dictionary(uniqueKeysWithValues: source.poll().map { ($0.id, $0) })

        XCTAssertEqual(byId["sess-busy"]?.state, .running)
        XCTAssertEqual(byId["sess-wait"]?.state, .needsInput)
        XCTAssertEqual(byId["sess-idle"]?.state, .done)
        XCTAssertEqual(byId["job-work"]?.state, .running)
        XCTAssertEqual(byId["job-block"]?.state, .needsInput)
        XCTAssertEqual(byId["job-done"]?.state, .done)
        // A live session attached to a job overrides the job's own "done".
        XCTAssertEqual(byId["job-attached"]?.state, .running)
        XCTAssertEqual(byId["job-attached"]?.pid, 107)
        XCTAssertNil(byId["job-old"], "done more than 24 h ago is hidden")
        XCTAssertNil(byId["job-stale"], "blocked months ago with no process counts as done, then hidden")

        XCTAssertEqual(byId["job-work"]?.control, .job("job00001"))
        XCTAssertEqual(byId["job-work"]?.entrypoint, .job)
        XCTAssertEqual(byId["sess-busy"]?.control, .viewOnly)
        XCTAssertEqual(byId["sess-busy"]?.entrypoint, .cli)
        XCTAssertEqual(byId["sess-idle"]?.entrypoint, .desktop)
        XCTAssertEqual(byId["sess-busy"]?.name, "Alpha work")
        XCTAssertEqual(byId["sess-wait"]?.name, "alpha", "no name: falls back to the project folder")
        XCTAssertEqual(byId["sess-busy"]?.project, "alpha")
        XCTAssertEqual(byId["sess-busy"]?.pid, 101)
        XCTAssertEqual(byId["sess-busy"]?.statusUpdatedAt, fixtureNow)
        XCTAssertEqual(byId["sess-busy"]?.startedAt, Date(timeIntervalSince1970: 1_793_541_000))
    }

    func testHeadlessSdkSessionIsExcluded() throws {
        let tree = try makeFixtures()
        let source = makeSource(tree, clock: { fixtureNow }, alive: allPids)
        XCTAssertFalse(source.poll().contains { $0.id == "sess-sdk" })
        XCTAssertEqual(Entrypoint(raw: "sdk-cli"), .sdk)
    }

    func testMalformedSessionFileAndKeyFilesAreSkipped() throws {
        let tree = try makeFixtures()
        let source = makeSource(tree, clock: { fixtureNow }, alive: allPids)
        let ids = source.poll().map(\.id)
        XCTAssertFalse(ids.contains("secret-token-not-json"))
        XCTAssertEqual(ids.count, Set(ids).count)
    }

    func testDeadPidBecomesExitedThenHiddenAfter24Hours() throws {
        let tree = try makeFixtures()
        var clock = fixtureNow
        var alive = allPids()
        let source = makeSource(tree, clock: { clock }, alive: { alive })

        XCTAssertEqual(source.poll().first { $0.id == "sess-dead" }?.state, .running)

        alive.remove(106)
        clock = fixtureNow.addingTimeInterval(60)
        let exited = source.poll().first { $0.id == "sess-dead" }
        XCTAssertEqual(exited?.state, .exited)
        XCTAssertEqual(exited?.statusUpdatedAt, clock, "the exit time is when we noticed it")

        clock = fixtureNow.addingTimeInterval(60 + 23 * 3600)
        XCTAssertEqual(source.poll().first { $0.id == "sess-dead" }?.state, .exited)

        clock = fixtureNow.addingTimeInterval(60 + 25 * 3600)
        XCTAssertNil(source.poll().first { $0.id == "sess-dead" })
    }

    func testDeadPidNeverSeenAliveIsNotListed() throws {
        let tree = try makeFixtures()
        var alive = allPids()
        alive.remove(106)
        let source = makeSource(tree, clock: { fixtureNow }, alive: { alive })
        XCTAssertNil(source.poll().first { $0.id == "sess-dead" })
    }

    func testDoneSessionsAreHiddenAfter24Hours() throws {
        let tree = try makeFixtures()
        var clock = fixtureNow
        let source = makeSource(tree, clock: { clock }, alive: allPids)
        XCTAssertNotNil(source.poll().first { $0.id == "sess-idle" })
        XCTAssertNotNil(source.poll().first { $0.id == "job-done" })

        clock = fixtureNow.addingTimeInterval(25 * 3600)
        let ids = source.poll().map(\.id)
        XCTAssertFalse(ids.contains("sess-idle"))
        XCTAssertFalse(ids.contains("job-done"))
        XCTAssertTrue(ids.contains("sess-busy"), "running sessions are never hidden")
    }
}
