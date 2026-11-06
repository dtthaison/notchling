import XCTest
@testable import Notchling

/// Token use from Codex rollouts and Antigravity CLI transcripts. The fixtures have the real files' shape
/// with made-up numbers.
final class CostSourcesTests: XCTestCase {
    func fixture(_ name: String) throws -> URL {
        try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "jsonl", subdirectory: "Fixtures"))
    }

    // MARK: Parsers

    private func objects(_ name: String) throws -> [[String: Any]] {
        try String(contentsOf: fixture(name)).split(separator: "\n").compactMap {
            try JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any]
        }
    }

    private func iso(_ s: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f.date(from: s) ?? ISO8601DateFormatter().date(from: s)
    }

    func testCodexEvents() throws {
        let events = try objects("codex-rollout").compactMap { CodexUsage.parse($0, date: iso) }
        XCTAssertEqual(events.count, 5, "session, turn and three token_count lines with info; the null-info one and agent_message are skipped")
        XCTAssertEqual(events.first, .session(account: "user-AAA:acct-111"))
        XCTAssertEqual(events[1], .turn(model: "gpt-5.1", folder: "/Users/me/work/demo"))
        guard case .tokens(let second) = events[3] else { return XCTFail("not tokens") }
        XCTAssertEqual(second.input, 25000)
        XCTAssertEqual(second.cached, 20000)
        XCTAssertEqual(second.output, 200, "the per-request number, not the running total of 300")
        XCTAssertEqual(second.total, 45300)
        XCTAssertEqual(CodexUsage.counts(second), [5000, 200, 20000, 0], "input is counted without its cached part")
    }

    func testAntigravityEvents() throws {
        let events = try objects("antigravity-transcript").compactMap { AntigravityUsage.parse($0, date: iso) }
        XCTAssertEqual(events.count, 3, "the tool output step is skipped")
        XCTAssertEqual(events.first, .model("Gemini 4 Argon"))
        guard case .tokens(let t) = events[2] else { return XCTFail("not tokens") }
        XCTAssertEqual(AntigravityUsage.counts(t), [2567, 365, 6418, 0])
    }

    func testModelSelectionChange() {
        XCTAssertEqual(AntigravityUsage.model(in: "x `Model Selection` from Gemini 4 Argon (Medium) to Gemini 3.8 Flash (Low). No need to comment"),
                       "Gemini 3.8 Flash")
        XCTAssertNil(AntigravityUsage.model(in: "just a prompt"))
    }

    func testPrices() {
        // gpt-5.1: $1.25 in, $0.125 cached, $10 out per million.
        XCTAssertEqual(OtherPrices.cost(model: "gpt-5.1", counts: [1_000_000, 100_000, 2_000_000]), 1.25 + 1 + 0.25, accuracy: 1e-9)
        XCTAssertEqual(OtherPrices.cost(model: "gpt-5.1-2026-01-01", counts: [1_000_000, 0, 0]), 1.25, accuracy: 1e-9)
        XCTAssertEqual(OtherPrices.cost(model: "gpt-6.1-sol", counts: [1_000_000, 1_000_000, 0]), 0, "unknown model: no cost")
        XCTAssertEqual(OtherPrices.cost(model: "gpt-5.6-sol", counts: [1_000_000, 0, 0]), 0, "a longer id doesn't take gpt-5's price")
    }

    func testWhichFilesAreRead() {
        XCTAssertTrue(CostTracker.accepts("/x/2026/11/06/rollout-2026-11-06T07-49-29-abc.jsonl", channel: "Codex"))
        XCTAssertFalse(CostTracker.accepts("/x/2026/11/06/rollout-2026-11-06T07-49-29-abc.jsonl.zst", channel: "Codex"))
        XCTAssertFalse(CostTracker.accepts("/x/history.jsonl", channel: "Codex"))
        XCTAssertTrue(CostTracker.accepts("/b/1/.system_generated/logs/transcript.jsonl", channel: "Antigravity CLI"))
        XCTAssertFalse(CostTracker.accepts("/b/1/.system_generated/logs/transcript_full.jsonl", channel: "Antigravity CLI"))
        XCTAssertTrue(CostTracker.accepts("/p/a.jsonl", channel: "Claude Code"))
    }

    // MARK: Through the tracker

    var dir: URL!
    var tracker: CostTracker!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-cost-\(UUID().uuidString)")
        let fm = FileManager.default
        try fm.createDirectory(at: dir.appendingPathComponent("codex/2026/11/06"), withIntermediateDirectories: true)
        try fm.createDirectory(at: dir.appendingPathComponent("brain/c1/.system_generated/logs"), withIntermediateDirectories: true)
        try fm.copyItem(at: fixture("codex-rollout"), to: dir.appendingPathComponent("codex/2026/11/06/rollout-2026-11-06T07-49-29-abc.jsonl"))
        try Data("not json".utf8).write(to: dir.appendingPathComponent("codex/2026/11/06/rollout-old.jsonl.zst"))
        try fm.copyItem(at: fixture("antigravity-transcript"), to: dir.appendingPathComponent("brain/c1/.system_generated/logs/transcript.jsonl"))
        tracker = CostTracker(roots: [(dir.appendingPathComponent("codex"), "Codex"), (dir.appendingPathComponent("brain"), "Antigravity CLI")],
                              cacheURL: dir.appendingPathComponent("cost-cache.json"))
    }

    override func tearDownWithError() throws {
        tracker = nil
        try? FileManager.default.removeItem(at: dir)
    }

    func refresh() {
        let before = tracker.scanned
        tracker.refresh()
        let deadline = Date().addingTimeInterval(10)
        while tracker.scanned == before, Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.05)) }
        XCTAssertNotEqual(tracker.scanned, before, "refresh did not finish")
    }

    func testCodexAndAntigravityChannels() throws {
        refresh()
        let all = try XCTUnwrap(tracker.ranged[Int.max])
        // Two requests of gpt-5.1: (20000 in, 100 out) and (5000 in, 20000 cached, 200 out); the repeat is dropped.
        let codex = try XCTUnwrap(all.channels.first { $0.name == "Codex" })
        XCTAssertEqual(codex.cost, 0.026 + 0.01075, accuracy: 1e-9)
        XCTAssertEqual(all.channelTokens["Codex"], 20100 + 25200)
        // Gemini 4 Argon has no price: tokens only.
        let agy = try XCTUnwrap(all.channels.first { $0.name == "Antigravity CLI" })
        XCTAssertEqual(agy.cost, 0)
        XCTAssertEqual(all.channelTokens["Antigravity CLI"], 8472 + 248 + 2567 + 6418 + 365)

        let gemini = try XCTUnwrap(all.modelDetail.first { $0.name == "Gemini 4 Argon" })
        XCTAssertEqual(gemini.input, 8472 + 2567)
        XCTAssertEqual(gemini.cacheRead, 6418)
        XCTAssertEqual(all.projects.first?.name, "demo")

        let owner = try XCTUnwrap(tracker.rangedByAccount[Int.max]?["user-AAA:acct-111"])
        XCTAssertEqual(owner.month, 0.026 + 0.01075, accuracy: 1e-9, "the rollout's creator is the account")
    }

    func testAppendedLinesKeepTheModelAndSkipARepeat() throws {
        refresh()
        let url = dir.appendingPathComponent("codex/2026/11/06/rollout-2026-11-06T07-49-29-abc.jsonl")
        let again = #"{"timestamp":"2026-11-06T00:51:00.000Z","ordinal":30,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":45300},"last_token_usage":{"input_tokens":25000,"cached_input_tokens":20000,"output_tokens":200,"total_tokens":25200}}}}"#
        let next = #"{"timestamp":"2026-11-06T00:52:00.000Z","ordinal":31,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":55300},"last_token_usage":{"input_tokens":10000,"cached_input_tokens":0,"output_tokens":0,"total_tokens":10000}}}}"#
        let handle = try FileHandle(forWritingTo: url)
        handle.seekToEndOfFile()
        handle.write(Data((again + "\n" + next + "\n").utf8))
        try handle.close()
        RunLoop.main.run(until: Date().addingTimeInterval(3.5))
        refresh()
        let codex = try XCTUnwrap(tracker.ranged[Int.max]?.channels.first { $0.name == "Codex" })
        // Only `next` is new, priced as gpt-5.1 from the turn_context read in the first pass.
        XCTAssertEqual(codex.cost, 0.026 + 0.01075 + 0.0125, accuracy: 1e-9)
    }
}
