import XCTest
@testable import ConsoleCore

/// Tool steps with their output and diff, `!` commands, and the compaction notice, as the phone shows them.
final class ChatRichTests: XCTestCase {
    private func parse(_ lines: [String]) -> [ChatMessage] {
        ChatTranscript.parse(lines: lines.map { Substring($0) }, isMain: true, path: "x").messages
    }

    private func assistant(_ id: String, blocks: String, time: String = "2026-11-04T08:00:01.000Z") -> String {
        #"{"type":"assistant","uuid":"\#(id)","timestamp":"\#(time)","message":{"role":"assistant","model":"m","content":[\#(blocks)]}}"#
    }

    private func user(_ id: String, content: String, time: String = "2026-11-04T08:00:02.000Z") -> String {
        #"{"type":"user","uuid":"\#(id)","timestamp":"\#(time)","message":{"role":"user","content":\#(content)}}"#
    }

    func testAnEditCarriesItsDiffAndABashCallItsOutput() {
        let messages = parse([
            assistant("a1", blocks: #"{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/p/lib/auth/guard.dart","old_string":"if (a) return 1;","new_string":"await ready;\nif (a) return 1;"}},{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"flutter test test/auth\nsecond line"}}"#),
            user("u1", content: #"[{"type":"tool_result","tool_use_id":"t2","content":"00:03 +14: All tests passed!"}]"#),
        ])
        XCTAssertEqual(messages.count, 1)
        let steps = messages[0].steps
        XCTAssertEqual(steps.map(\.name), ["Edit", "Bash"])
        XCTAssertEqual(steps[0].summary, "auth/guard.dart")
        XCTAssertEqual(steps[0].path, "/p/lib/auth/guard.dart")
        XCTAssertEqual(steps[0].diff.map(\.wire), ["-if (a) return 1;", "+await ready;", "+if (a) return 1;"])
        XCTAssertEqual(steps[1].summary, "flutter test test/auth", "only the first line of a command")
        XCTAssertEqual(steps[1].output, "00:03 +14: All tests passed!")
    }

    func testAResultThatArrivesAfterTheRunWasFoldedStillLandsOnItsStep() {
        let messages = parse([
            assistant("a1", blocks: #"{"type":"text","text":"Reading."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/a.swift"}}"#),
            user("u1", content: #"[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"import Foundation"}]}]"#),
        ])
        let tools = messages.first { $0.role == .tools }
        XCTAssertEqual(tools?.steps.first?.output, "import Foundation")
    }

    func testAWriteShowsItsFirstLinesAsAdded() {
        let messages = parse([assistant("a1", blocks: #"{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/p/n.md","content":"one\ntwo"}}"#)])
        XCTAssertEqual(messages[0].steps[0].diff.map(\.wire), ["+one", "+two"])
    }

    func testAShellCommandAndItsOutputBecomeOneMessage() {
        let messages = parse([
            user("u1", content: #""<bash-input>git status --short</bash-input>""#),
            user("u2", content: #""<bash-stdout> M a.dart\n?? b.md</bash-stdout><bash-stderr></bash-stderr>""#),
        ])
        XCTAssertEqual(messages.count, 1)
        XCTAssertEqual(messages[0].kind, .shell)
        XCTAssertEqual(messages[0].role, .user)
        XCTAssertEqual(messages[0].text, "!git status --short")
        XCTAssertEqual(messages[0].output, "M a.dart\n?? b.md")
    }

    func testCompactionIsOneNoticeAndItsSummaryIsNotAUserMessage() {
        let messages = parse([
            assistant("a1", blocks: #"{"type":"text","text":"before"}"#),
            #"{"type":"system","subtype":"compact_boundary","uuid":"c1","timestamp":"2026-11-04T08:00:03.000Z","compactMetadata":{"trigger":"auto","preTokens":168000}}"#,
            #"{"type":"user","uuid":"u9","isCompactSummary":true,"timestamp":"2026-11-04T08:00:04.000Z","message":{"role":"user","content":"This session is being continued from a previous conversation..."}}"#,
            assistant("a2", blocks: #"{"type":"text","text":"after"}"#),
        ])
        XCTAssertEqual(messages.map(\.kind), [.text, .compaction, .text])
        XCTAssertEqual(messages[1].text, "168k tokens")
        XCTAssertFalse(messages.contains { $0.text.contains("being continued") })
    }

    func testLongOutputIsCutAndNoStepsMeansNoField() {
        let big = String(repeating: "x", count: 5000)
        let messages = parse([
            assistant("a1", blocks: #"{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cat big"}}"#),
            user("u1", content: #"[{"type":"tool_result","tool_use_id":"t1","content":"\#(big)"}]"#),
        ])
        XCTAssertLessThanOrEqual(messages[0].steps[0].output?.count ?? 0, 3000)
        XCTAssertNil(ChatMessageDTO(ChatMessage(id: UUID(), role: .assistant, text: "hi", timestamp: Date(), toolSummary: nil)).steps, "absent, not an empty list")
    }

    func testTheWireCarriesTheNewFieldsOnlyWhenThereAreAny() throws {
        let edit = ToolStep(id: "t", name: "Edit", summary: "a.dart", path: "/a.dart", diff: [DiffRow(.removed, "x"), DiffRow(.added, "y")])
        let message = ChatMessage(id: UUID(), role: .tools, text: "Edited 1 file", timestamp: Date(), toolSummary: "Edited 1 file", steps: [edit])
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(ChatMessageDTO(message))) as? [String: Any]
        let step = (json?["steps"] as? [[String: Any]])?.first
        XCTAssertEqual(step?["diff"] as? [String], ["-x", "+y"])
        XCTAssertNil(json?["kind"])
        XCTAssertNil(json?["output"])
    }

    func testASessionCarriesAMarkerOfClaudesNewestReplyOnlyWhenThereIsOne() throws {
        var session = makeSession("s")
        XCTAssertNil(SessionDTO(session).lastMessage)
        session.lastMessage = LastMessageInfo(id: "item-42", text: "Done. The tests pass.")
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(SessionDTO(session))) as? [String: Any]
        let marker = json?["lastMessage"] as? [String: Any]
        XCTAssertEqual(marker?["id"] as? String, "item-42")
        XCTAssertEqual(marker?["text"] as? String, "Done. The tests pass.")
        XCTAssertNil((try JSONSerialization.jsonObject(with: JSONEncoder().encode(SessionDTO(makeSession("t")))) as? [String: Any])?["lastMessage"])
    }
}
