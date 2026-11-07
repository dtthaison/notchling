import XCTest
@testable import ConsoleCore

final class OwnershipTests: XCTestCase {
    func testSessionFileOfTheChildIsMapped() throws {
        let tree = try makeFixtures()
        let sessionsDir = tree.paths.sessionsDir
        XCTAssertEqual(SessionPidMapper.sessionId(forRootPid: 101, sessionsDir: sessionsDir, parentOf: { _ in nil }), "sess-busy")
    }

    func testSessionFileOfADescendantIsMapped() throws {
        let tree = try makeFixtures()
        // The launcher is pid 5000; the real claude process (pid 102) is its child.
        let parents: [Int32: Int32] = [102: 5000]
        let id = SessionPidMapper.sessionId(forRootPid: 5000, sessionsDir: tree.paths.sessionsDir, parentOf: { parents[$0] })
        XCTAssertEqual(id, "sess-wait")
    }

    func testUnrelatedRootMapsToNothing() throws {
        let tree = try makeFixtures()
        XCTAssertNil(SessionPidMapper.sessionId(forRootPid: 9, sessionsDir: tree.paths.sessionsDir, parentOf: { _ in nil }))
    }

    func testDescendantChainIsBounded() {
        let chain: [Int32: Int32] = [10: 9, 9: 8, 8: 7, 7: 6, 6: 5, 5: 4, 4: 3, 3: 2, 2: 1000]
        XCTAssertFalse(OwnedSessions.descends(10, from: 1000, parentOf: { chain[$0] }), "more than 8 hops is not followed")
        XCTAssertTrue(OwnedSessions.descends(3, from: 1000, parentOf: { chain[$0] }))
    }

    func testCatalogMarksOwnedSessionsAsConsolePTY() throws {
        let tree = try makeFixtures()
        let catalog = SessionCatalog(paths: tree.paths, now: { fixtureNow }, isAlive: { allPids().contains($0) },
                                     isOwned: { $0 == 101 })
        let sessions = catalog.snapshot()
        XCTAssertEqual(sessions.first { $0.id == "sess-busy" }?.control, .consolePTY)
        XCTAssertEqual(sessions.first { $0.id == "sess-wait" }?.control, .viewOnly)
        XCTAssertEqual(sessions.first { $0.id == "job-work" }?.control, .job("job00001"))
    }
}
