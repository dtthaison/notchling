import XCTest
@testable import Notchling

final class MachinesStatusTests: XCTestCase {
    private func fixture(_ name: String) throws -> Data {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures"))
        return try Data(contentsOf: url)
    }

    private func status(_ json: String) throws -> WorkerStatus {
        try XCTUnwrap(WorkerStatus.decode(Data(json.utf8)))
    }

    private let now = Date(timeIntervalSince1970: 1_800_000_000)

    private func ok(_ s: WorkerStatus) -> LastReport { LastReport(status: s, at: now, error: nil) }
    private func failed() -> LastReport { LastReport(status: nil, at: now, error: "ssh: timed out") }

    private func busy(current: Bool = true, ready: Int = 0, needsYou: Int = 0) -> WorkerStatus {
        var s = WorkerStatus()
        s.machine = "m1"
        if current { s.current = .init(repo: "o/r", issue: 1, stage: "implementation") }
        s.ready = Array(repeating: .init(repo: "o/r", issue: 2), count: ready)
        s.needsYou = Array(repeating: .init(repo: "o/r", issue: 3), count: needsYou)
        return s
    }

    // MARK: decoding

    func testDecodesFullStatus() throws {
        let s = try XCTUnwrap(WorkerStatus.decode(try fixture("worker-status")))
        XCTAssertEqual(s.machine, "vps-sing-1")
        XCTAssertEqual(s.version, "1.19.1")
        XCTAssertNil(s.paused)
        XCTAssertEqual(s.health["github"], "error: 1 outbox items failed")
        XCTAssertEqual(s.health["claude"], "ok")
        XCTAssertEqual(s.current?.issue, 214)
        XCTAssertEqual(s.current?.stage, "implementation")
        XCTAssertEqual(s.current?.startedAt, StatusDates.parse("2026-11-06T01:02:03Z"))
        XCTAssertEqual(s.ready.count, 1)
        XCTAssertEqual(s.ready[0].commit, "9f3c2a1")
        XCTAssertEqual(s.ready[0].evidenceDir, "/home/agent/evidence/201/verify")
        XCTAssertEqual(s.ready[0].readyAt, StatusDates.parse("2026-11-06T00:15:09Z"))
        XCTAssertEqual(s.needsYou.count, 2)
        XCTAssertEqual(s.needsYou[0].question, "Cap at 50 or 100?")
        XCTAssertEqual(s.needsYou[0].recommend, "50")
        XCTAssertEqual(s.needsYou[0].options, ["50", "100"])
        XCTAssertEqual(s.stuck.count, 1)
        XCTAssertEqual(s.stuck[0].reportTail, "line one\nline two")
        XCTAssertEqual(s.jobsTonight, 3)
        XCTAssertEqual(s.weeklyPct, 42.5)
        XCTAssertEqual(s.tokenExpiresInDays, 14)
        XCTAssertEqual(s.bootedAt, StatusDates.parse("2026-11-05T22:10:31Z"))
        XCTAssertEqual(s.since, StatusDates.parse("2026-11-04T18:00:00Z"))
        XCTAssertEqual(s.unrouted, ["ios"])
        XCTAssertEqual(s.testflight?.failed?.count, 1)
        XCTAssertEqual(s.testflight?.failed?.newestId, "sub1")
        XCTAssertFalse(s.testflight?.quiet ?? true)
    }

    func testDecodesTriageNeedsYouWithoutBranchOrWorktree() throws {
        let s = try XCTUnwrap(WorkerStatus.decode(try fixture("worker-status")))
        let triage = s.needsYou[1]
        XCTAssertEqual(triage.issue, 230)
        XCTAssertNil(triage.branch)
        XCTAssertNil(triage.worktree)
        XCTAssertNil(triage.question)
        XCTAssertEqual(triage.recommend, "split it")
        XCTAssertEqual(triage.options, [])
    }

    func testDecodesMissingAndExtraFields() throws {
        let s = try XCTUnwrap(WorkerStatus.decode(try fixture("worker-status-old")))
        XCTAssertEqual(s.machine, "m1")
        XCTAssertEqual(s.version, "1.13.0")
        XCTAssertNil(s.current)
        XCTAssertEqual(s.ready.count, 1)
        XCTAssertEqual(s.ready[0].evidenceDir, "")
        XCTAssertNil(s.ready[0].readyAt)
        XCTAssertEqual(s.needsYou[0].reason, "verify stopped")
        XCTAssertNil(s.needsYou[0].question)
        XCTAssertEqual(s.needsYou[0].options, [])
        XCTAssertEqual(s.stuck, [])
        XCTAssertEqual(s.jobsTonight, 1)
        XCTAssertNil(s.weeklyPct)
        XCTAssertNil(s.tokenExpiresInDays)
        XCTAssertNil(s.bootedAt)
        XCTAssertNil(s.since)
        XCTAssertEqual(s.outboxFailed, 0)
        XCTAssertEqual(s.unrouted, [])
        XCTAssertNil(s.testflight)
    }

    func testEmptyObjectIsAnEmptyStatus() throws {
        XCTAssertEqual(try status("{}"), WorkerStatus())
    }

    func testWrongTypesBecomeDefaultsAndBadEntriesAreDropped() throws {
        let s = try status("""
        {"machine": 5, "jobs_tonight": "many", "paused": null, "ready": "none",
         "needs_you": [{"repo": "o/r", "issue": 7, "options": "x"}, 42, {"repo": "o/r", "issue": 8}],
         "booted_at": "yesterday", "token_expires_in_days": 3.5}
        """)
        XCTAssertEqual(s.machine, "")
        XCTAssertEqual(s.jobsTonight, 0)
        XCTAssertEqual(s.ready, [])
        XCTAssertEqual(s.needsYou.map(\.issue), [7, 8])
        XCTAssertEqual(s.needsYou[0].options, [])
        XCTAssertNil(s.bootedAt)
        XCTAssertNil(s.tokenExpiresInDays)
    }

    func testNotAnObjectDoesNotDecode() {
        XCTAssertNil(WorkerStatus.decode(Data("[1, 2]".utf8)))
        XCTAssertNil(WorkerStatus.decode(Data("not json".utf8)))
    }

    func testDatesWithFractionalSeconds() throws {
        let s = try status(#"{"booted_at": "2026-11-05T22:10:31.250Z", "since": "2026-11-05T22:10:31Z"}"#)
        let booted = try XCTUnwrap(s.bootedAt)
        let since = try XCTUnwrap(s.since)
        XCTAssertEqual(booted.timeIntervalSince(since), 0.25, accuracy: 0.001)
    }

    func testOutboxFailedCountsAListOrTakesANumber() throws {
        XCTAssertEqual(try status(#"{"outbox_failed": [{"item": {}, "error": "x"}, {"item": {}}]}"#).outboxFailed, 2)
        XCTAssertEqual(try status(#"{"outbox_failed": 3}"#).outboxFailed, 3)
        XCTAssertEqual(try status(#"{"outbox_failed": null}"#).outboxFailed, 0)
    }

    func testTestFlightErrorAndQuiet() throws {
        let s = try status(#"{"testflight": {"quiet": true, "last_new_at": null, "error": "cannot read state"}}"#)
        XCTAssertEqual(s.testflight?.quiet, true)
        XCTAssertNil(s.testflight?.lastNewAt)
        XCTAssertEqual(s.testflight?.error, "cannot read state")
        XCTAssertNil(s.testflight?.failed)
    }

    func testPausedFixture() throws {
        let s = try XCTUnwrap(WorkerStatus.decode(try fixture("worker-status-paused")))
        XCTAssertEqual(s.paused, "load5 6.4 is above 5.0")
        XCTAssertEqual(s.health["host"], "paused: load5 6.4 is above 5.0")
    }

    // MARK: health

    func testHealthOnline() throws {
        let s = try XCTUnwrap(WorkerStatus.decode(try fixture("worker-status")))
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: ok(s), previous: nil, now: now), "online")
        XCTAssertEqual(MachineHealth.word(fileVault: false, last: ok(s), previous: ok(s), now: now), "online")
    }

    func testHealthOfflineNoJobs() {
        let idle = busy(current: false)
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: failed(), previous: ok(idle), now: now), "offline")
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: failed(), previous: nil, now: now), "offline")
        // never reached, nothing known before it
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: failed(), previous: failed(), now: now), "offline")
    }

    func testHealthRestartedWhenUnreachableWithJobs() {
        for before in [busy(current: true), busy(current: false, ready: 1), busy(current: false, needsYou: 1)] {
            XCTAssertEqual(MachineHealth.word(fileVault: true, last: failed(), previous: ok(before), now: now),
                           "restarted, waiting for unlock")
        }
    }

    func testHealthOfflineForNonFileVault() {
        XCTAssertEqual(MachineHealth.word(fileVault: false, last: failed(), previous: ok(busy()), now: now), "offline")
    }

    func testHealthRestartedWhenBootedAtMovesWhileWorking() {
        var before = busy(); before.bootedAt = Date(timeIntervalSince1970: 1_000)
        var after = busy(); after.bootedAt = Date(timeIntervalSince1970: 2_000)
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: ok(after), previous: ok(before), now: now),
                       "restarted, waiting for unlock")
        XCTAssertEqual(MachineHealth.word(fileVault: false, last: ok(after), previous: ok(before), now: now), "online")
        // same boot, an idle worker, or nothing to compare with: online
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: ok(before), previous: ok(before), now: now), "online")
        after.current = nil
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: ok(after), previous: ok(before), now: now), "online")
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: ok(busy()), previous: ok(before), now: now), "online")
    }

    func testHealthPaused() throws {
        let s = try XCTUnwrap(WorkerStatus.decode(try fixture("worker-status-paused")))
        XCTAssertEqual(MachineHealth.word(fileVault: true, last: ok(s), previous: nil, now: now), "production busy, paused")
        XCTAssertEqual(MachineHealth.word(fileVault: false, last: ok(s), previous: nil, now: now), "production busy, paused")
    }

    func testAFailedPollIsNotOnlineEvenWithAStatusAttached() {
        let both = LastReport(status: busy(), at: now, error: "bad json")
        XCTAssertEqual(MachineHealth.word(fileVault: false, last: both, previous: nil, now: now), "offline")
    }

    // MARK: token warning

    func testTokenWarningAt14NotAt15() {
        var s = WorkerStatus(); s.machine = "m1"
        s.tokenExpiresInDays = 14
        XCTAssertEqual(MachineHealth.tokenWarning(s), "GitHub token on m1 expires in 14 days")
        s.tokenExpiresInDays = 15
        XCTAssertNil(MachineHealth.tokenWarning(s))
        s.tokenExpiresInDays = nil
        XCTAssertNil(MachineHealth.tokenWarning(s))
        s.tokenExpiresInDays = 1
        XCTAssertEqual(MachineHealth.tokenWarning(s), "GitHub token on m1 expires in 1 day")
        s.tokenExpiresInDays = -2
        XCTAssertEqual(MachineHealth.tokenWarning(s), "GitHub token on m1 has expired")
    }
}
