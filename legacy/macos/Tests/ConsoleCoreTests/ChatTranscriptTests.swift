import XCTest
@testable import ConsoleCore

final class ChatTranscriptTests: XCTestCase {
    private func writeLines(_ lines: [String], in tree: FixtureTree, name: String) throws -> URL {
        let url = tree.chatDir.appendingPathComponent(name)
        try Data((lines.joined(separator: "\n") + "\n").utf8).write(to: url)
        return url
    }

    private func assistantLine(_ text: String, uuid: String) -> String {
        #"{"type":"assistant","uuid":"\#(uuid)","message":{"role":"assistant","content":[{"type":"text","text":"\#(text)"}]}}"#
    }

    func testMainTranscriptSkipsSidechainMetaAndInjectedWrappers() throws {
        let tree = try makeFixtures()
        let result = ChatTranscript.read(url: tree.chatDir.appendingPathComponent("main.jsonl"), isMain: true)
        let text = result.messages.map(\.text).joined(separator: "|")
        XCTAssertFalse(text.contains("meta line"))
        XCTAssertFalse(text.contains("sidechain"))
        XCTAssertFalse(text.contains("system-reminder"))
        XCTAssertFalse(text.contains("file body"), "tool results are not chat")
        XCTAssertEqual(result.messages.map(\.role), [.user, .tools, .assistant, .user])
        XCTAssertEqual(result.messages.map(\.text),
                       ["Please add a unit test", "Read 2 files, ran 1 shell command", "Added the test.", "Thanks"])
    }

    func testToolUseBlocksCollapseIntoOneSummaryMessage() throws {
        let tree = try makeFixtures()
        let result = ChatTranscript.read(url: tree.chatDir.appendingPathComponent("main.jsonl"), isMain: true)
        let tools = try XCTUnwrap(result.messages.first { $0.role == .tools })
        XCTAssertEqual(tools.toolSummary, "Read 2 files, ran 1 shell command")
        XCTAssertEqual(result.messages.filter { $0.role == .tools }.count, 1)
        XCTAssertNil(result.messages.first { $0.role == .user }?.toolSummary)
    }

    func testMalformedLineIsCountedAndSkipped() throws {
        let tree = try makeFixtures()
        let result = ChatTranscript.read(url: tree.chatDir.appendingPathComponent("main.jsonl"), isMain: true)
        XCTAssertEqual(result.malformedLines, 1)
        XCTAssertEqual(result.messages.count, 4, "the lines around it still parse")
    }

    func testSubagentTranscriptKeepsItsSidechainLines() throws {
        let tree = try makeFixtures()
        let url = tree.chatDir.appendingPathComponent("subagent.jsonl")
        XCTAssertEqual(ChatTranscript.read(url: url, isMain: false).messages.map(\.text), ["Scan the repo", "Scanning now"])
        XCTAssertTrue(ChatTranscript.read(url: url, isMain: true).messages.isEmpty)
    }

    func testLongTextIsCappedAt3000Characters() throws {
        let tree = try makeFixtures()
        let url = try writeLines([assistantLine(String(repeating: "x", count: 3500), uuid: "long")], in: tree, name: "long.jsonl")
        let message = try XCTUnwrap(ChatTranscript.read(url: url, isMain: true).messages.first)
        XCTAssertEqual(message.text.count, 3000)
        XCTAssertTrue(message.text.hasSuffix("…"))

        let short = try writeLines([assistantLine(String(repeating: "y", count: 3000), uuid: "exact")], in: tree, name: "exact.jsonl")
        XCTAssertEqual(ChatTranscript.read(url: short, isMain: true).messages.first?.text.count, 3000)
    }

    func testLinesOver100KBAreSkipped() throws {
        let tree = try makeFixtures()
        let url = try writeLines([
            assistantLine("before", uuid: "u1"),
            assistantLine(String(repeating: "z", count: 150_000), uuid: "huge"),
            #"{"type":"user","uuid":"u3","message":{"role":"user","content":"after"}}"#,
        ], in: tree, name: "huge.jsonl")
        XCTAssertEqual(ChatTranscript.read(url: url, isMain: true).messages.map(\.text), ["before", "after"])
    }

    func testReturnsOnlyTheLast200Messages() throws {
        let tree = try makeFixtures()
        let lines = (0..<250).map { index -> String in
            let role = index.isMultiple(of: 2) ? "user" : "assistant"
            return #"{"type":"\#(role)","uuid":"n\#(index)","message":{"role":"\#(role)","content":"message \#(index)"}}"#
        }
        let url = try writeLines(lines, in: tree, name: "many.jsonl")
        let messages = ChatTranscript.read(url: url, isMain: true).messages
        XCTAssertEqual(messages.count, 200)
        XCTAssertEqual(messages.first?.text, "message 50")
        XCTAssertEqual(messages.last?.text, "message 249")
    }

    func testOnlyTheTailIsReadAndTheCutLineIsDropped() throws {
        let tree = try makeFixtures()
        let lines = (0..<50).map { #"{"type":"user","uuid":"t\#($0)","message":{"role":"user","content":"line \#($0)"}}"# }
        let url = try writeLines(lines, in: tree, name: "tail.jsonl")
        let tailLines = try XCTUnwrap(ChatTranscript.tail(of: url, bytes: 400))
        XCTAssertLessThan(tailLines.count, 50)
        let result = ChatTranscript.parse(lines: tailLines, isMain: true, path: url.path)
        XCTAssertEqual(result.malformedLines, 0, "a half line at the cut is not reported as malformed")
        // Adjacent user lines merge into one message.
        XCTAssertTrue(result.messages.last?.text.hasSuffix("line 49") == true)
        XCTAssertFalse(result.messages.contains { $0.text.contains("line 0\n") || $0.text.hasPrefix("line 0") })
    }

    func testMessageIdsAreStableAcrossReads() throws {
        let tree = try makeFixtures()
        let url = tree.chatDir.appendingPathComponent("main.jsonl")
        let first = ChatTranscript.read(url: url, isMain: true).messages.map(\.id)
        let second = ChatTranscript.read(url: url, isMain: true).messages.map(\.id)
        XCTAssertEqual(first, second)
        XCTAssertEqual(Set(first).count, first.count)
    }

    func testCacheReparsesOnlyWhenTheFileChanges() throws {
        let tree = try makeFixtures()
        let url = try writeLines([assistantLine("one", uuid: "c1")], in: tree, name: "cache.jsonl")
        let cache = ChatTranscriptCache()
        XCTAssertEqual(cache.messages(at: url, isMain: true).count, 1)
        try Data((assistantLine("one", uuid: "c1") + "\n" + assistantLine("two", uuid: "c2") + "\n").utf8).write(to: url)
        // Adjacent assistant lines merge into one message, so compare the text.
        XCTAssertEqual(cache.messages(at: url, isMain: true).first?.text, "one\n\ntwo")
    }
}
