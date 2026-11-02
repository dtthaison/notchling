import XCTest
@testable import Notchling

/// The switching rules. These decide when a real account switch happens, so they are checked here
/// instead of by switching (see CLAUDE.md).
final class LimitLogicTests: XCTestCase {
    let now = Date(timeIntervalSince1970: 1_800_000_000)

    func testLevels() {
        XCTAssertEqual(LimitLogic.level(89.9), 0)
        XCTAssertEqual(LimitLogic.level(90), 90)
        XCTAssertEqual(LimitLogic.level(99.9), 90)
        XCTAssertEqual(LimitLogic.level(100), 100)
    }

    func testStepAnnouncesEachLevelOnce() {
        var mark = LimitLogic.Mark()
        let reset = now.addingTimeInterval(3600)
        var r = LimitLogic.step(mark, percent: 91, resetsAt: reset, skip90: false)
        XCTAssertEqual(r.fire, 90); mark = r.mark
        r = LimitLogic.step(mark, percent: 95, resetsAt: reset, skip90: false)
        XCTAssertNil(r.fire); mark = r.mark
        r = LimitLogic.step(mark, percent: 100, resetsAt: reset, skip90: false)
        XCTAssertEqual(r.fire, 100)
    }

    func testStepFiresAgainAfterTheWindowResets() {
        let first = LimitLogic.step(LimitLogic.Mark(), percent: 92, resetsAt: now, skip90: false).mark
        let next = LimitLogic.step(first, percent: 92, resetsAt: now.addingTimeInterval(5 * 3600), skip90: false)
        XCTAssertEqual(next.fire, 90)
    }

    func testStepFiresAgainAfterUsageDrops() {
        var mark = LimitLogic.step(LimitLogic.Mark(), percent: 92, resetsAt: now, skip90: false).mark
        mark = LimitLogic.step(mark, percent: 10, resetsAt: now, skip90: false).mark
        XCTAssertEqual(LimitLogic.step(mark, percent: 93, resetsAt: now, skip90: false).fire, 90)
    }

    func testSkip90StillRecordsTheLevel() {
        let r = LimitLogic.step(LimitLogic.Mark(), percent: 90, resetsAt: now, skip90: true)
        XCTAssertNil(r.fire)
        XCTAssertEqual(r.mark.level, 90)
    }

    func testPickTargetPrefersMostRoom() {
        let all = [
            LimitLogic.Candidate(id: "active", fetched: now, session: 5, week: 5),
            LimitLogic.Candidate(id: "busy", fetched: now, session: 50, week: 10),
            LimitLogic.Candidate(id: "roomy", fetched: now, session: 20, week: 30),
        ]
        XCTAssertEqual(LimitLogic.pickTarget(all, activeID: "active", now: now, blockedUntil: [:])?.id, "roomy")
    }

    func testPickTargetSkipsStaleFullBlockedAndSignedOut() {
        let all = [
            LimitLogic.Candidate(id: "stale", fetched: now.addingTimeInterval(-LimitLogic.freshFor - 1), session: 1, week: 1),
            LimitLogic.Candidate(id: "full", fetched: now, session: 80, week: 1),
            LimitLogic.Candidate(id: "blocked", fetched: now, session: 1, week: 1),
            LimitLogic.Candidate(id: "signedOut", fetched: now, session: 1, week: 1, needsLogin: true),
            LimitLogic.Candidate(id: "unknown", fetched: now, session: nil, week: nil),
        ]
        let blocked = ["blocked": now.addingTimeInterval(60)]
        XCTAssertNil(LimitLogic.pickTarget(all, activeID: nil, now: now, blockedUntil: blocked))
        XCTAssertEqual(LimitLogic.pickTarget(all, activeID: nil, now: now.addingTimeInterval(61),
                                             blockedUntil: blocked)?.id, "blocked",
                       "once its reset passes, the blocked account is the only one with no other fault")
    }

    func testBlockedAccountComesBackAfterItsReset() {
        let c = [LimitLogic.Candidate(id: "a", fetched: now, session: 1, week: 1)]
        XCTAssertNil(LimitLogic.pickTarget(c, activeID: nil, now: now, blockedUntil: ["a": now.addingTimeInterval(1)]))
        XCTAssertEqual(LimitLogic.pickTarget(c, activeID: nil, now: now, blockedUntil: ["a": now])?.id, "a")
    }

    func testMaySwitchAtMostEveryHalfHour() {
        XCTAssertTrue(LimitLogic.maySwitch(lastSwitch: nil, now: now))
        XCTAssertFalse(LimitLogic.maySwitch(lastSwitch: now.addingTimeInterval(-29 * 60), now: now))
        XCTAssertTrue(LimitLogic.maySwitch(lastSwitch: now.addingTimeInterval(-30 * 60), now: now))
    }

    func testBlockedUntilUsesLatestResetOverThreshold() {
        let session = UsageWindow(percent: 96, resetsAt: now.addingTimeInterval(3600))
        let week = UsageWindow(percent: 97, resetsAt: now.addingTimeInterval(86400))
        let low = UsageWindow(percent: 10, resetsAt: now.addingTimeInterval(999_999))
        XCTAssertEqual(LimitLogic.blockedUntil(session: session, week: week, other: [low], at: 95, now: now),
                       now.addingTimeInterval(86400))
        XCTAssertEqual(LimitLogic.blockedUntil(session: nil, week: nil, at: 95, now: now),
                       now.addingTimeInterval(5 * 3600), "no window over the line: wait five hours")
    }
}
