import XCTest
@testable import Notchling

/// The run records sd appends to `<git common dir>/sd/runs.jsonl`, and what Notchling makes of them.
final class RunLedgerTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func iso(_ date: Date) -> String { ISO8601DateFormatter().string(from: date) }

    func start(_ log: String, at date: Date? = nil, pid: Int = 4242, session: String? = "sess-1", limit: String = "7200",
               key: String = "just perf") -> String {
        let s = session.map { "\"\($0)\"" } ?? "null"
        return #"{"event":"start","key":"\#(key)","log":"\#(log)","cwd":"/repo","pid":\#(pid),"session":\#(s),"limit":\#(limit),"started_at":"\#(iso(date ?? t0))","extra":1}"#
    }

    func finish(_ log: String, at date: Date, rc: Int = 0, key: String = "just perf") -> String {
        #"{"key":"\#(key)","cmd":"just perf","seconds":12.5,"rc":\#(rc),"head":"abc","clean":false,"log":"\#(log)","at":"\#(iso(date))"}"#
    }

    // MARK: Parsing

    func testParsesAStartRecord() {
        guard case .start(let s)? = RunLine.parse(start("/l/a.log")) else { return XCTFail("not a start") }
        XCTAssertEqual(s.key, "just perf")
        XCTAssertEqual(s.log, "/l/a.log")
        XCTAssertEqual(s.cwd, "/repo")
        XCTAssertEqual(s.pid, 4242)
        XCTAssertEqual(s.session, "sess-1")
        XCTAssertEqual(s.limit, 7200)
        XCTAssertEqual(s.startedAt, t0)
    }

    func testStartWithNullLimitAndSession() {
        guard case .start(let s)? = RunLine.parse(start("/l/a.log", session: nil, limit: "null")) else { return XCTFail() }
        XCTAssertNil(s.limit)
        XCTAssertNil(s.session)
    }

    func testParsesAFinishRecordWithoutAnEventKey() {
        guard case .finish(let f)? = RunLine.parse(finish("/l/a.log", at: t0.addingTimeInterval(60), rc: 124)) else { return XCTFail() }
        XCTAssertEqual(f.rc, 124)
        XCTAssertEqual(f.log, "/l/a.log")
        XCTAssertEqual(f.seconds, 12.5)
        XCTAssertEqual(f.at, t0.addingTimeInterval(60))
    }

    func testBadLinesAreIgnored() {
        XCTAssertNil(RunLine.parse(""))
        XCTAssertNil(RunLine.parse("not json"))
        XCTAssertNil(RunLine.parse("[1,2]"))
        XCTAssertNil(RunLine.parse(#"{"event":"start","key":"k"}"#), "a start needs a log and a time")
        XCTAssertNil(RunLine.parse(#"{"key":"k","rc":0}"#), "a finish needs a log")
        XCTAssertNil(RunLine.parse(#"{"event":"heartbeat","log":"/l"}"#), "unknown events are not finishes")
    }

    func testFractionalSecondsInTimes() {
        let line = #"{"event":"start","key":"k","log":"/l","pid":1,"started_at":"2026-11-07T05:00:15.250Z"}"#
        guard case .start(let s)? = RunLine.parse(line) else { return XCTFail() }
        XCTAssertEqual(s.startedAt.timeIntervalSince1970.truncatingRemainder(dividingBy: 1), 0.25, accuracy: 0.001)
    }

    func testTheRealFileShapeParsesAsFinishRecords() {
        let line = #"{"key": "caffeinate -i mise exec -- just --justfile apps/ios/justfile perf suite=coaching build=no", "cmd": "x", "seconds": 24.7, "rc": 0, "head": "b27a", "clean": false, "log": "/r/apps/ios/.build/logs/n-2.log", "at": "2026-11-07T05:00:15Z"}"#
        guard case .finish(let f)? = RunLine.parse(line) else { return XCTFail() }
        XCTAssertEqual(f.log, "/r/apps/ios/.build/logs/n-2.log")
        XCTAssertEqual(f.rc, 0)
    }

    // MARK: Ledger

    func testARunIsActiveUntilItsFinishRecord() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log")])
        XCTAssertEqual(ledger.active(now: t0, pidAlive: { _ in true }).map(\.log), ["/l/a.log"])
        ledger.apply(lines: [finish("/l/a.log", at: t0.addingTimeInterval(30))])
        XCTAssertTrue(ledger.active(now: t0, pidAlive: { _ in true }).isEmpty)
    }

    func testADeadPidIsNotActive() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log", pid: 77)])
        XCTAssertTrue(ledger.active(now: t0, pidAlive: { $0.pid != 77 }).isEmpty)
    }

    func testARunPastItsLimitIsNotActiveEvenIfItsPidIsAlive() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log", limit: "7200"), start("/l/b.log", limit: "null")])
        let justInside = t0.addingTimeInterval(7200 + RunLedger.limitSlack - 1)
        XCTAssertEqual(ledger.active(now: justInside, pidAlive: { _ in true }).count, 2)
        let past = t0.addingTimeInterval(7200 + RunLedger.limitSlack + 1)
        XCTAssertEqual(ledger.active(now: past, pidAlive: { _ in true }).map(\.log), ["/l/b.log"], "a run with no limit stays until it is stale")
    }

    func testTheAliveCheckGetsTheWholeStartRecord() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log", at: t0.addingTimeInterval(5), pid: 77)])
        var seen: [RunStart] = []
        _ = ledger.active(now: t0.addingTimeInterval(10), pidAlive: { seen.append($0); return true })
        XCTAssertEqual(seen.map(\.pid), [77])
        XCTAssertEqual(seen.map(\.startedAt), [t0.addingTimeInterval(5)])
    }

    // MARK: Runs that vanish

    func testARunSeenAliveThatLosesItsProcessHasEndedWithoutAResult() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log", pid: 77, session: "s-a")])
        XCTAssertTrue(ledger.disappeared(now: t0.addingTimeInterval(10), pidAlive: { _ in true }).isEmpty)
        let gone = ledger.disappeared(now: t0.addingTimeInterval(20), pidAlive: { _ in false })
        XCTAssertEqual(gone.map(\.log), ["/l/a.log"])
        XCTAssertTrue(ledger.disappeared(now: t0.addingTimeInterval(30), pidAlive: { _ in false }).isEmpty, "reported once")
        ledger.markLost(gone, at: t0.addingTimeInterval(20))
        let ends = ledger.finished(since: t0)
        XCTAssertEqual(ends.count, 1)
        XCTAssertNil(ends[0].finish.rc)
        XCTAssertEqual(ends[0].start?.session, "s-a")
        XCTAssertEqual(ends[0].finish.at, t0.addingTimeInterval(20))
        XCTAssertEqual(ends[0].finish.seconds, 20)
        XCTAssertTrue(ends[0].id.hasPrefix("lost:"), "its own dedupe id: \(ends[0].id)")
        XCTAssertTrue(ledger.active(now: t0.addingTimeInterval(30), pidAlive: { _ in true }).isEmpty, "and the run is over")
    }

    func testARunNeverSeenAliveIsNotReportedLost() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log")])
        XCTAssertTrue(ledger.disappeared(now: t0, pidAlive: { _ in false }).isEmpty)
        XCTAssertTrue(ledger.disappeared(now: t0, pidAlive: { _ in false }).isEmpty)
    }

    func testARunThatFinishedIsNotLost() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log")])
        _ = ledger.disappeared(now: t0, pidAlive: { _ in true })
        ledger.apply(lines: [finish("/l/a.log", at: t0.addingTimeInterval(5))])
        XCTAssertTrue(ledger.disappeared(now: t0.addingTimeInterval(6), pidAlive: { _ in false }).isEmpty)
    }

    func testAFinishThatArrivesAfterTheRunWasMarkedLostReplacesIt() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log")])
        _ = ledger.disappeared(now: t0, pidAlive: { _ in true })
        ledger.markLost(ledger.disappeared(now: t0.addingTimeInterval(10), pidAlive: { _ in false }), at: t0.addingTimeInterval(10))
        XCTAssertEqual(ledger.finished(since: t0).count, 1)
        ledger.apply(lines: [finish("/l/a.log", at: t0.addingTimeInterval(11), rc: 3)])
        let ends = ledger.finished(since: t0)
        XCTAssertEqual(ends.count, 1)
        XCTAssertEqual(ends[0].finish.rc, 3)
    }

    func testMarkLostSkipsARunWhoseFinishHasArrived() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log")])
        _ = ledger.disappeared(now: t0, pidAlive: { _ in true })
        let gone = ledger.disappeared(now: t0.addingTimeInterval(10), pidAlive: { _ in false })
        ledger.apply(lines: [finish("/l/a.log", at: t0.addingTimeInterval(9))])   // read again just before
        ledger.markLost(gone, at: t0.addingTimeInterval(10))
        XCTAssertEqual(ledger.finished(since: t0).map(\.finish.rc), [0])
    }

    func testARunPastItsLimitIsNotLostWhileItsProcessLives() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log", limit: "60")])
        _ = ledger.disappeared(now: t0, pidAlive: { _ in true })
        let past = t0.addingTimeInterval(60 + RunLedger.limitSlack + 1)
        XCTAssertTrue(ledger.active(now: past, pidAlive: { _ in true }).isEmpty, "no longer shown or holding the Mac awake")
        XCTAssertTrue(ledger.disappeared(now: past, pidAlive: { _ in true }).isEmpty, "but nothing says it ended")
        XCTAssertEqual(ledger.disappeared(now: past.addingTimeInterval(5), pidAlive: { _ in false }).map(\.log), ["/l/a.log"])
    }

    func testFinishMatchesItsStartByLog() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log", session: "s-a"), start("/l/b.log", session: "s-b"),
                             finish("/l/b.log", at: t0.addingTimeInterval(5), rc: 2)])
        let ends = ledger.finished(since: t0)
        XCTAssertEqual(ends.count, 1)
        XCTAssertEqual(ends[0].start?.session, "s-b")
        XCTAssertEqual(ends[0].finish.rc, 2)
        XCTAssertEqual(ledger.active(now: t0, pidAlive: { _ in true }).map(\.log), ["/l/a.log"])
    }

    func testAFinishWithoutAStartStillCounts() {
        var ledger = RunLedger()
        ledger.apply(lines: [finish("/l/old.log", at: t0)])
        let ends = ledger.finished(since: t0.addingTimeInterval(-1))
        XCTAssertEqual(ends.count, 1)
        XCTAssertNil(ends[0].start)
    }

    func testAReusedLogIsActiveAgainAndKeepsTheEarlierFinish() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log"), finish("/l/a.log", at: t0.addingTimeInterval(10)),
                             start("/l/a.log", at: t0.addingTimeInterval(100))])
        XCTAssertEqual(ledger.active(now: t0, pidAlive: { _ in true }).count, 1)
        XCTAssertEqual(ledger.finished(since: t0).count, 1)
        ledger.apply(lines: [finish("/l/a.log", at: t0.addingTimeInterval(150))])
        XCTAssertTrue(ledger.active(now: t0, pidAlive: { _ in true }).isEmpty)
        XCTAssertEqual(Set(ledger.finished(since: t0).map(\.id)).count, 2)
    }

    func testFinishedHonoursTheCutoff() {
        var ledger = RunLedger()
        ledger.apply(lines: [finish("/l/a.log", at: t0), finish("/l/b.log", at: t0.addingTimeInterval(1000))])
        XCTAssertEqual(ledger.finished(since: t0.addingTimeInterval(500)).map(\.finish.log), ["/l/b.log"])
    }

    func testOldStartsWithoutFinishAreNotActive() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log", at: t0)])
        XCTAssertTrue(ledger.active(now: t0.addingTimeInterval(3 * 86400), pidAlive: { _ in true }).isEmpty)
    }

    func testPruneDropsOldEnds() {
        var ledger = RunLedger()
        ledger.apply(lines: [start("/l/a.log"), finish("/l/a.log", at: t0.addingTimeInterval(5))])
        ledger.prune(now: t0.addingTimeInterval(2 * 3600))
        XCTAssertTrue(ledger.finished(since: .distantPast).isEmpty)
    }

    // MARK: Incremental reading

    var dir: URL!

    override func setUp() {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("runledger-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDown() { try? FileManager.default.removeItem(at: dir) }

    func write(_ text: String, to name: String = "runs.jsonl") -> URL {
        let url = dir.appendingPathComponent(name)
        try? text.write(to: url, atomically: false, encoding: .utf8)
        return url
    }

    func append(_ text: String, to url: URL) {
        let h = try! FileHandle(forWritingTo: url)
        h.seekToEndOfFile()
        h.write(text.data(using: .utf8)!)
        try? h.close()
    }

    func testTailReadsOnlyWhatWasAppended() {
        let url = write("a\nb\n")
        var tail = JSONLTail()
        XCTAssertEqual(tail.read(url).lines, ["a", "b"])
        XCTAssertEqual(tail.read(url).lines, [])
        append("c\n", to: url)
        XCTAssertEqual(tail.read(url).lines, ["c"])
    }

    func testTailWaitsForTheEndOfAPartialLine() {
        let url = write("a\nhal")
        var tail = JSONLTail()
        XCTAssertEqual(tail.read(url).lines, ["a"])
        append("f\n", to: url)
        XCTAssertEqual(tail.read(url).lines, ["half"])
    }

    func testTailStartsOverAfterTruncation() {
        let url = write("one\ntwo\nthree\n")
        var tail = JSONLTail()
        _ = tail.read(url)
        _ = write("x\n")
        let r = tail.read(url)
        XCTAssertTrue(r.reset)
        XCTAssertEqual(r.lines, ["x"])
    }

    func testTailCapsTheFirstReadAndDropsTheCutLine() {
        let url = write((0..<100).map { "line-\($0)" }.joined(separator: "\n") + "\n")
        var tail = JSONLTail()
        tail.firstReadCap = 50
        let lines = tail.read(url).lines
        XCTAssertEqual(lines.last, "line-99")
        XCTAssertTrue(lines.count < 10)
        XCTAssertTrue(lines.allSatisfy { $0.hasPrefix("line-") && Int($0.dropFirst(5)) != nil }, "no half line: \(lines)")
    }

    func testTailOnAMissingFileIsEmpty() {
        var tail = JSONLTail()
        XCTAssertEqual(tail.read(dir.appendingPathComponent("nope")).lines, [])
    }

    func testTailThenLedgerEndToEnd() {
        let url = write(start("/l/a.log") + "\n" + "garbage\n")
        var tail = JSONLTail()
        var ledger = RunLedger()
        ledger.apply(lines: tail.read(url).lines)
        XCTAssertEqual(ledger.active(now: t0, pidAlive: { _ in true }).count, 1)
        append(finish("/l/a.log", at: t0.addingTimeInterval(9)) + "\n", to: url)
        ledger.apply(lines: tail.read(url).lines)
        XCTAssertEqual(ledger.finished(since: t0).count, 1)
    }
}
