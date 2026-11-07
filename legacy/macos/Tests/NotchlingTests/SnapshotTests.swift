import XCTest
@testable import Notchling

/// What the notch shows for one transcript. The expectations were taken from the full-tail parser
/// before reading became incremental, so both must agree.
final class SnapshotTests: XCTestCase {
    func fixturePath(_ name: String) throws -> String {
        try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "jsonl", subdirectory: "Fixtures")).path
    }

    func testNotchSnapshot() throws {
        let s = TranscriptReader.snapshot(atPath: try fixturePath("notch"))
        // A message's tools are tallied before its text is added, so they show above it.
        XCTAssertEqual(s.lines.map(\.kind), [.user, .tools, .assistant, .tools, .assistant, .user, .tools])
        XCTAssertEqual(s.lines.map(\.text), [
            "Find the config [Pasted text +2 lines]",
            "Read 2 files",
            "I'll look.",
            "Searched for 1 pattern, used search",
            "Found it in b.swift.\n\nSecond paragraph.",
            "Now fix it",
            "Edited 1 file, asked you",
        ])
        XCTAssertEqual(s.lines.map(\.id), Array(0..<7))
        XCTAssertEqual(s.lastPrompt, "Now fix it")
        XCTAssertNotNil(s.activity)
        let q = try XCTUnwrap(s.question)
        XCTAssertEqual(q.toolUseID, "q1")
        XCTAssertEqual(q.items.first?.options, ["Yes", "No"])
        XCTAssertEqual(q.items.first?.details, ["both files", ""])
        XCTAssertTrue(q.answerable)
    }
}

/// A temporary folder and file helpers for the incremental reader tests; no tests of its own.
class TranscriptFileTestCase: XCTestCase {
    var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-cursor-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws { try? FileManager.default.removeItem(at: dir) }

    func append(_ text: String, to path: String) throws {
        let handle = try XCTUnwrap(FileHandle(forWritingAtPath: path))
        handle.seekToEndOfFile()
        handle.write(Data(text.utf8))
        try handle.close()
    }

}

/// The cursor must always agree with reading the whole file fresh.
final class IncrementalSnapshotTests: TranscriptFileTestCase {
    func fixtureLines() throws -> [String] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "notch", withExtension: "jsonl", subdirectory: "Fixtures"))
        return try String(contentsOf: url, encoding: .utf8).split(separator: "\n").map(String.init)
    }

    /// A fresh read of the same bytes, through a path no cursor has seen.
    func fresh(_ path: String) throws -> Snapshot {
        let copy = dir.appendingPathComponent("fresh-\(UUID().uuidString).jsonl").path
        try FileManager.default.copyItem(atPath: path, toPath: copy)
        return TranscriptReader.snapshot(atPath: copy)
    }

    func testAppendingLineByLineMatchesAFreshRead() throws {
        let path = dir.appendingPathComponent("live.jsonl").path
        FileManager.default.createFile(atPath: path, contents: nil)
        for line in try fixtureLines() {
            // Half a line first, as when Claude Code is mid-write: it must wait for the rest.
            let half = line.index(line.startIndex, offsetBy: line.count / 2)
            try append(String(line[..<half]), to: path)
            _ = TranscriptReader.snapshot(atPath: path)
            try append(String(line[half...]) + "\n", to: path)
            XCTAssertEqual(TranscriptReader.snapshot(atPath: path), try fresh(path), "after: \(line.prefix(60))")
        }
        XCTAssertEqual(TranscriptReader.snapshot(atPath: path).lines.count, 7)
    }

    func testReplacedOrTruncatedFileIsReadAgain() throws {
        let lines = try fixtureLines()
        let path = dir.appendingPathComponent("t.jsonl").path
        try (lines.joined(separator: "\n") + "\n").write(toFile: path, atomically: false, encoding: .utf8)
        XCTAssertEqual(TranscriptReader.snapshot(atPath: path).lines.count, 7)

        // Truncated in place to the first line.
        try (lines[0] + "\n").write(toFile: path, atomically: false, encoding: .utf8)
        XCTAssertEqual(TranscriptReader.snapshot(atPath: path).lines.map(\.kind), [.user])

        // Replaced by a new file (new inode) with different content.
        try (lines[9] + "\n").write(toFile: path, atomically: true, encoding: .utf8)
        XCTAssertEqual(TranscriptReader.snapshot(atPath: path).lastPrompt, "Now fix it")
    }

    func testLongAssistantRunsStayBounded() throws {
        let path = dir.appendingPathComponent("long.jsonl").path
        FileManager.default.createFile(atPath: path, contents: nil)
        let chunk = String(repeating: "x", count: 900)
        for i in 0..<40 {
            try append(#"{"type":"assistant","message":{"content":[{"type":"text","text":"\#(i) \#(chunk)"}]}}"# + "\n", to: path)
            _ = TranscriptReader.snapshot(atPath: path)
        }
        let live = TranscriptReader.snapshot(atPath: path)
        XCTAssertEqual(live, try fresh(path))
        XCTAssertEqual(live.lines.first?.text.count, 3001, "shown as … and the last 3000 characters")
    }
}

/// The Chat window's reader, appended to line by line, must match a fresh read of the same bytes.
final class IncrementalConversationTests: TranscriptFileTestCase {
    func conversationLines() throws -> [String] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "conversation", withExtension: "jsonl", subdirectory: "Fixtures"))
        return try String(contentsOf: url, encoding: .utf8).split(separator: "\n").map(String.init)
    }

    func freshConversation(_ path: String) throws -> Conversation {
        let copy = dir.appendingPathComponent("fresh-\(UUID().uuidString).jsonl").path
        try FileManager.default.copyItem(atPath: path, toPath: copy)
        return ConversationReader.read(copy)
    }

    /// Item ids come from a counter, so a fresh read numbers them the same only if nothing was dropped.
    func testConversationAppendMatchesAFreshRead() throws {
        let path = dir.appendingPathComponent("chat.jsonl").path
        FileManager.default.createFile(atPath: path, contents: nil)
        for line in try conversationLines() {
            let half = line.index(line.startIndex, offsetBy: line.count / 2)
            try append(String(line[..<half]), to: path)
            _ = ConversationReader.read(path)
            try append(String(line[half...]) + "\n", to: path)
            XCTAssertEqual(ConversationReader.read(path), try freshConversation(path), "after: \(line.prefix(60))")
        }
        XCTAssertEqual(ConversationReader.read(path).items.count, 8)
    }

    func testLongSessionsKeepTheLast500Items() throws {
        let path = dir.appendingPathComponent("long.jsonl").path
        FileManager.default.createFile(atPath: path, contents: nil)
        var text = ""
        for i in 0..<1300 {
            text += #"{"type":"user","message":{"content":"prompt \#(i)"}}"# + "\n"
            text += #"{"type":"assistant","message":{"id":"m\#(i)","content":[{"type":"tool_use","id":"t\#(i)","name":"Bash","input":{"command":"ls"}}]}}"# + "\n"
            text += #"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t\#(i)","content":"ok \#(i)"}]}}"# + "\n"
            if i % 100 == 99 { try append(text, to: path); text = ""; _ = ConversationReader.read(path) }
        }
        let c = ConversationReader.read(path)
        XCTAssertEqual(c.items.count, 500)
        guard case .tools(let calls) = try XCTUnwrap(c.items.last).kind else { return XCTFail("last item is not tools") }
        XCTAssertEqual(calls.first?.output, "ok 1299", "results still land on their calls after older items were dropped")
        XCTAssertEqual(c.facts.commands, 1300)
    }
}
