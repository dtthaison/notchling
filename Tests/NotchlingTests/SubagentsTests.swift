import XCTest
@testable import Notchling

/// Subagents beside a session transcript, in the shape Claude Code 2.1.318 writes them.
final class SubagentsTests: XCTestCase {
    var dir: URL!
    var transcript: String { dir.appendingPathComponent("sess.jsonl").path }

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("subagents-\(UUID().uuidString)")
        let subs = dir.appendingPathComponent("sess/subagents")
        try FileManager.default.createDirectory(at: subs, withIntermediateDirectories: true)
        func agent(_ id: String, _ meta: String, _ lines: [String]) throws {
            try meta.write(to: subs.appendingPathComponent("agent-\(id).meta.json"), atomically: true, encoding: .utf8)
            try lines.joined(separator: "\n").appending("\n").write(to: subs.appendingPathComponent("agent-\(id).jsonl"), atomically: true, encoding: .utf8)
        }
        try agent("abg", #"{"agentType":"Explore","description":"Survey the repo","toolUseId":"toolu_bg","requestShape":"background","model":"sonnet","effort":"high"}"#, [
            #"{"isSidechain":true,"agentId":"abg","type":"user","message":{"role":"user","content":"Read-only survey of the repo."},"uuid":"u1","timestamp":"2026-11-05T10:00:00.000Z"}"#,
            #"{"isSidechain":true,"agentId":"abg","type":"assistant","message":{"id":"m1","role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"text","text":"The repo has three apps."}]},"uuid":"u2","timestamp":"2026-11-05T10:01:00.000Z"}"#,
        ])
        try agent("afg", #"{"agentType":"general-purpose","description":"Fix the test","toolUseId":"toolu_fg"}"#, [])
        try "".write(toFile: transcript, atomically: true, encoding: .utf8)
    }

    override func tearDownWithError() throws { try? FileManager.default.removeItem(at: dir) }

    func testListsWithMetaAndStatus() {
        let now = Date()
        let subs = Subagents.list(forTranscript: transcript, finished: ["abg": "completed"], now: now)
        XCTAssertEqual(Set(subs.map(\.id)), ["abg", "afg"])
        let bg = subs.first { $0.id == "abg" }
        XCTAssertEqual(bg?.description, "Survey the repo")
        XCTAssertEqual(bg?.type, "Explore")
        XCTAssertEqual(bg?.model, "sonnet")
        XCTAssertEqual(bg?.background, true)
        XCTAssertEqual(bg?.status, .completed)
        XCTAssertEqual(subs.first { $0.id == "afg" }?.status, .running, "just written, no word it ended")
        XCTAssertEqual(Subagents.list(forTranscript: transcript, now: now.addingTimeInterval(3600)).first { $0.id == "afg" }?.status, .quiet)
    }

    func testBackgroundLaunchResultDoesNotEndIt() {
        let bg = Subagent(id: "abg", path: "", description: "", type: "", model: nil, effort: nil, background: true,
                          toolUseID: "toolu_bg", started: Date(), updated: Date())
        XCTAssertEqual(Subagents.status(of: bg, finished: ["toolu_bg": "completed"], now: Date()), .running)
        var fg = bg
        fg.background = false
        XCTAssertEqual(Subagents.status(of: fg, finished: ["toolu_bg": "failed"], now: Date()), .failed)
    }

    func testReadsNotificationsFromTheSessionTranscript() throws {
        let note = "<task-notification>\\n<task-id>abg</task-id>\\n<status>completed</status>\\n<summary>done</summary>\\n</task-notification>"
        let lines = [
            #"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T10:02:00.000Z","content":"\#(note)"}"#,
            #"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_fg","content":"fixed"}]},"toolUseResult":{"status":"completed","agentId":"afg"},"timestamp":"2026-11-05T10:03:00.000Z"}"#,
            #"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_bg","content":"launched"}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"abg"},"timestamp":"2026-11-05T09:59:00.000Z"}"#,
        ]
        try lines.joined(separator: "\n").appending("\n").write(toFile: transcript, atomically: true, encoding: .utf8)
        let finished = ConversationReader.read(transcript).finishedAgents
        XCTAssertEqual(finished["abg"], "completed")
        XCTAssertEqual(finished["afg"], "completed")
        XCTAssertEqual(finished["toolu_fg"], "completed")
        XCTAssertNil(finished["toolu_bg"], "a background launch isn't an end")
    }

    func testReadsTheSubagentsOwnSidechainTranscript() {
        let sub = Subagents.list(forTranscript: transcript).first { $0.id == "abg" }!
        let conversation = ConversationReader.readSubagent(sub.path)
        let texts = conversation.items.compactMap { item -> String? in
            switch item.kind {
            case .user(let t, _), .assistant(let t): return t
            default: return nil
            }
        }
        XCTAssertEqual(texts, ["Read-only survey of the repo.", "The repo has three apps."])
        XCTAssertTrue(ConversationReader.read(sub.path).items.isEmpty, "a session's reader still skips sidechain lines")
    }

    func testActiveCountForTheNotch() {
        XCTAssertEqual(Subagents.activeCount(forTranscript: transcript), 2)
        XCTAssertEqual(Subagents.activeCount(forTranscript: transcript, now: Date().addingTimeInterval(600)), 0)
    }
}
