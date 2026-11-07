import XCTest
@testable import Notchling

/// Claude Code's JSONL transcripts, read from fixtures. The format is Claude Code's, not ours, so these
/// pin what the chat shows for each kind of line.
final class TranscriptTests: XCTestCase {
    func fixture(_ name: String) throws -> String {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "jsonl", subdirectory: "Fixtures"))
        return url.path
    }

    func testConversationItems() throws {
        let c = ConversationReader.read(try fixture("conversation"))
        let kinds = c.items.map(\.kind)
        guard kinds.count == 8 else { return XCTFail("items: \(kinds)") }

        XCTAssertEqual(kinds[0], .user(text: "Fix the bug", images: 0))
        XCTAssertEqual(kinds[1], .thinking(seconds: 3, text: "Check the folder first."))
        XCTAssertEqual(kinds[2], .assistant(text: "Looking."))

        guard case .tools(let calls) = kinds[3] else { return XCTFail("expected tools, got \(kinds[3])") }
        XCTAssertEqual(calls.map(\.name), ["Bash", "Edit"])
        XCTAssertEqual(calls[0].title, "List files")
        XCTAssertEqual(calls[0].kind, .shell)
        XCTAssertTrue(calls[0].finished)
        XCTAssertEqual(calls[0].output, "a.txt\nb.txt")
        XCTAssertEqual(calls[1].title, "Edited a.txt")
        XCTAssertNil(calls[1].output, "edits show their diff, not the tool's text")
        XCTAssertEqual(calls[1].diff.map(\.kind), [.hunk, .context, .remove, .add])
        XCTAssertEqual(calls[1].added, 1)
        XCTAssertEqual(calls[1].removed, 1)

        XCTAssertEqual(kinds[4], .assistant(text: "Done."))
        XCTAssertEqual(kinds[5], .turnEnd(seconds: 12, model: "claude-opus-5-5"))
        XCTAssertEqual(kinds[6], .user(text: "/cost", images: 0), "a local_command line shows as typed")
        XCTAssertEqual(kinds[7], .notice(text: "Interrupted", symbol: "stop.circle"))
    }

    func testConversationFacts() throws {
        let c = ConversationReader.read(try fixture("conversation"))
        XCTAssertEqual(c.facts.branch, "main")
        XCTAssertEqual(c.facts.model, "claude-opus-5-5")
        XCTAssertEqual(c.facts.turns, 1)
        XCTAssertEqual(c.facts.commands, 1)
        XCTAssertEqual(c.facts.files, ["/repo/a.txt"])
        XCTAssertEqual(c.facts.inputTokens, 100 + 5 + 1, "a message's usage counts once though its lines repeat it")
        XCTAssertNil(c.activity, "nothing is still running")
    }

    func testMissingFileIsEmpty() {
        XCTAssertEqual(ConversationReader.read("/nonexistent/x.jsonl"), Conversation())
    }

    func testLimitHit() throws {
        let hit = try XCTUnwrap(TranscriptReader.limitHit(inFile: try fixture("limit-hit")))
        XCTAssertEqual(hit.id, "u-limit")
        XCTAssertTrue(hit.text.hasPrefix("You've hit your session limit"))
        XCTAssertNotNil(hit.at)
    }

    func testA429IsNotALimitHit() throws {
        XCTAssertNil(TranscriptReader.limitHit(inFile: try fixture("rate-limited-429")))
    }

    func testOneCompactionShowsOnce() throws {
        // Claude Code writes a compact_boundary line and then the summary; older builds only the summary.
        let compacted = ChatItem.Kind.notice(text: "Conversation compacted", symbol: "rectangle.compress.vertical")
        XCTAssertEqual(ConversationReader.read(try fixture("compact")).items.map(\.kind),
                       [.user(text: "Before", images: 0), compacted, .user(text: "After", images: 0), compacted])
    }

    func testContextIsTheLatestRequest() throws {
        // The last message's input (with cache) and output, not the session's total.
        XCTAssertEqual(ConversationReader.read(try fixture("conversation")).facts.contextTokens, 1 + 3)
        // Right after a compaction, what it left.
        XCTAssertEqual(ConversationReader.read(try fixture("compact")).facts.contextTokens, 28610)
    }

    func testTokenTextAndContextWindow() {
        XCTAssertEqual(ChatFormat.tokens(950), "950")
        XCTAssertEqual(ChatFormat.tokens(96_400), "96.4k")
        XCTAssertEqual(ChatFormat.tokens(400_000), "400k")
        XCTAssertEqual(ChatFormat.tokens(1_250_000), "1.2M")
        XCTAssertEqual(ChatFormat.contextWindow(model: "claude-opus-5-5"), 400_000)
        XCTAssertNil(ChatFormat.contextWindow(model: "claude-sonnet-5-5"), "unknown: no guessed percentage")
        XCTAssertNil(ChatFormat.contextWindow(model: nil))
    }

    func testCollapsePastes() {
        XCTAssertEqual(TranscriptReader.collapsePastes("see <pasted_content id=\"1\">\na\nb\nc\n</pasted_content> ok"),
                       "see [Pasted text +3 lines] ok")
        XCTAssertEqual(TranscriptReader.collapsePastes("<pasted_content>one</pasted_content>"), "[Pasted text]")
        XCTAssertEqual(TranscriptReader.collapsePastes("plain"), "plain")
    }
}

/// "!" shell commands, as Claude Code 2.1.316 records them (checked against a real session).
final class ShellCommandTests: XCTestCase {
    func testShellCommandsAndOutput() throws {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "bash-mode", withExtension: "jsonl", subdirectory: "Fixtures"))
        let kinds = ConversationReader.read(url.path).items.map(\.kind)
        XCTAssertEqual(kinds, [
            .user(text: "! git branch", images: 0),
            .commandOutput(text: "  feat/x\n* main"), // indentation kept
            .user(text: "! ped", images: 0),
            .commandOutput(text: "(eval):1: command not found: ped"),
            .user(text: "! true", images: 0),
            // Printed nothing: no output row.
            .commandOutput(text: "green"),
        ])
    }
}
