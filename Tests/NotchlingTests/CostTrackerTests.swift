import XCTest
@testable import Notchling

/// Token cost from transcripts in a temporary folder: the first scan walks everything, later ones read
/// only what FSEvents reported.
final class CostTrackerTests: XCTestCase {
    var dir: URL!
    var tracker: CostTracker!

    override func setUpWithError() throws {
        // Under /var/folders, which FSEvents reports as /private/var/folders: the symlink case for free.
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-cost-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir.appendingPathComponent("projects/-tmp-demo"), withIntermediateDirectories: true)
        tracker = CostTracker(roots: [(dir.appendingPathComponent("projects"), "Claude Code")],
                              cacheURL: dir.appendingPathComponent("cost-cache.json"))
    }

    override func tearDownWithError() throws {
        tracker = nil
        try? FileManager.default.removeItem(at: dir)
    }

    /// An assistant line costing `dollars` at Opus 5.5 prices ($20 per million output tokens).
    func usageLine(id: String, dollars: Double) -> String {
        let stamp = ISO8601DateFormatter().string(from: Date())
        let tokens = Int(dollars / 20 * 1_000_000)
        return #"{"type":"assistant","timestamp":"\#(stamp)","message":{"id":"\#(id)","model":"claude-opus-5-5","usage":{"output_tokens":\#(tokens)}}}"# + "\n"
    }

    func append(_ text: String, to name: String) throws {
        let url = dir.appendingPathComponent("projects/-tmp-demo/\(name)")
        if !FileManager.default.fileExists(atPath: url.path) { FileManager.default.createFile(atPath: url.path, contents: nil) }
        let handle = try FileHandle(forWritingTo: url)
        handle.seekToEndOfFile()
        handle.write(Data(text.utf8))
        try handle.close()
    }

    /// Runs one refresh and waits for it to finish on the main thread.
    func refresh() {
        let before = tracker.scanned
        tracker.refresh()
        let deadline = Date().addingTimeInterval(10)
        while tracker.scanned == before, Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        }
        XCTAssertNotEqual(tracker.scanned, before, "refresh did not finish")
    }

    /// FSEvents delivers after its latency (2 s); give it a little more.
    func waitForEvents() { RunLoop.main.run(until: Date().addingTimeInterval(3.5)) }

    func testFirstScanThenOnlyChangedFiles() throws {
        try append(usageLine(id: "m1", dollars: 20), to: "a.jsonl")
        refresh()
        XCTAssertEqual(tracker.summary.today, 20, accuracy: 0.001)

        // Appended after the first walk: only FSEvents can tell the next refresh about it.
        try append(usageLine(id: "m2", dollars: 4), to: "a.jsonl")
        try append(usageLine(id: "m3", dollars: 6), to: "b.jsonl")
        waitForEvents()
        refresh()
        XCTAssertEqual(tracker.summary.today, 30, accuracy: 0.001)
    }

    func testAMessageCopiedToAnotherTranscriptCountsOnce() throws {
        try append(usageLine(id: "m1", dollars: 20), to: "a.jsonl")
        refresh()
        // Claude Code copies messages into the new transcript when it compacts or resumes.
        try append(usageLine(id: "m1", dollars: 20), to: "resumed.jsonl")
        waitForEvents()
        refresh()
        XCTAssertEqual(tracker.summary.today, 20, accuracy: 0.001)
    }

    func testNothingChangedKeepsTheSummary() throws {
        try append(usageLine(id: "m1", dollars: 20), to: "a.jsonl")
        refresh()
        var changes = 0
        tracker.changed = { changes += 1 }
        refresh()
        XCTAssertEqual(changes, 0)
        XCTAssertEqual(tracker.summary.today, 20, accuracy: 0.001)
    }

    func testSaveNowWritesTheCacheAndANewTrackerReadsIt() throws {
        try append(usageLine(id: "m1", dollars: 20), to: "a.jsonl")
        refresh()
        tracker.saveNow()
        let cache = dir.appendingPathComponent("cost-cache.json")
        XCTAssertTrue(FileManager.default.fileExists(atPath: cache.path))

        let again = CostTracker(roots: [(dir.appendingPathComponent("projects"), "Claude Code")], cacheURL: cache)
        tracker = again
        refresh()
        XCTAssertEqual(again.summary.today, 20, accuracy: 0.001, "read from the cache, not counted twice")
    }
}

/// A summary is published only when it differs, so every part of it has to take part in `==`.
final class CostSummaryEqualityTests: XCTestCase {
    func base() -> CostTracker.Summary {
        var s = CostTracker.Summary()
        s.today = 1; s.week = 2; s.month = 3
        s.days = [(Date(timeIntervalSince1970: 0), 1)]
        s.models = [("Opus 5.5", 3)]
        s.modelDetail = [CostTracker.ModelStat(name: "claude-opus-5-5", cost: 3, percent: 100, input: 1, output: 2, cacheRead: 0, cacheWrite: 0)]
        s.projects = [("demo", 3)]
        s.accounts = [("a", 3)]
        s.channels = [("Claude Code", 3)]
        return s
    }

    func testEqualSummariesAreEqual() {
        XCTAssertEqual(base(), base())
    }

    func testEachFieldChangeShows() {
        var week = base(); week.week = 5
        var days = base(); days.days = [(Date(timeIntervalSince1970: 86400), 1)]
        var models = base(); models.models = [("Sonnet 5.5", 3)]
        var projects = base(); projects.projects = [("other", 3)]
        var accounts = base(); accounts.accounts = [("b", 3)]
        var channels = base(); channels.channels = [("Desktop", 3)]
        for (name, changed) in [("week", week), ("days", days), ("models", models), ("projects", projects),
                                ("accounts", accounts), ("channels", channels)] {
            XCTAssertNotEqual(base(), changed, name)
        }
    }
}
