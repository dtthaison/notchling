import XCTest
@testable import Notchling

/// `acp-agy.jsonl` is a real `agy acp` exchange (Antigravity CLI 1.5.1): `>` lines Notchling sent, `<`
/// lines it got. A `session/load` replays an earlier turn (ls, read a file, answer), then a live turn
/// edits the file after a permission request.
final class ACPTranscriptTests: XCTestCase {
    private func replay() throws -> (ACPTranscript, [String: Any]?) {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "acp-agy", withExtension: "jsonl", subdirectory: "Fixtures"))
        var t = ACPTranscript()
        var permission: [String: Any]?
        for line in try String(contentsOf: url, encoding: .utf8).split(separator: "\n") {
            let sent = line.hasPrefix(">")
            let json = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(line.dropFirst(2).utf8)) as? [String: Any])
            let method = json["method"] as? String
            if sent {
                if method == "session/prompt" { t.beginTurn() }
                continue
            }
            let params = json["params"] as? [String: Any]
            if method == "session/update", let update = params?["update"] as? [String: Any] {
                t.apply(update)
            } else if method == "session/request_permission", let call = params?["toolCall"] as? [String: Any] {
                permission = params
                t.apply(call.merging(["sessionUpdate": "tool_call_update"]) { _, new in new })
            } else if let result = json["result"] as? [String: Any], result["stopReason"] != nil {
                t.endTurn(stopReason: result["stopReason"] as? String, usage: result["usage"] as? [String: Any])
            }
        }
        return (t, permission)
    }

    func testReplayAndLiveTurn() throws {
        let (t, _) = try replay()
        let kinds = t.items.map { item -> String in
            switch item.kind {
            case .user: return "user"
            case .assistant: return "assistant"
            case .thinking: return "thinking"
            case .tools(let calls): return "tools(\(calls.count))"
            case .turnEnd: return "end"
            default: return "other"
            }
        }
        XCTAssertEqual(kinds, ["user", "tools(2)", "assistant", "user", "tools(1)", "assistant", "end"])
        XCTAssertEqual(t.title, "Execute Command And Read File")

        guard case .tools(let first) = t.items[1].kind else { return XCTFail() }
        XCTAssertEqual(first[0].kind, .shell)
        XCTAssertEqual(first[0].input, "ls")
        XCTAssertTrue(first[0].finished)
        XCTAssertEqual(first[0].output?.contains("hello.txt"), true)
        XCTAssertEqual(first[1].kind, .read)

        guard case .tools(let edit) = t.items[4].kind else { return XCTFail() }
        XCTAssertEqual(edit[0].kind, .edit)
        XCTAssertTrue(edit[0].finished)
        XCTAssertEqual(edit[0].diff.filter { $0.kind == .remove }.map(\.text), ["hello from the scratch folder"])
        XCTAssertEqual(edit[0].diff.filter { $0.kind == .add }.map(\.text), ["hello from Notchling"])

        guard case .assistant(let reply) = t.items[5].kind else { return XCTFail() }
        XCTAssertEqual(reply, "ok")

        let c = t.conversation()
        XCTAssertNil(c.turnStart)
        XCTAssertEqual(c.facts.turns, 1)
        XCTAssertEqual(c.facts.files.count, 1)
        XCTAssertGreaterThan(c.facts.contextTokens, 0)
    }

    func testPermissionRequest() throws {
        let (_, params) = try replay()
        let p = try XCTUnwrap(params)
        let call = ACPTranscript.toolCall(try XCTUnwrap(p["toolCall"] as? [String: Any]), merging: nil)
        XCTAssertEqual(call.kind, .edit)
        XCTAssertEqual(call.title, "File hello.txt")
        XCTAssertFalse(call.diff.isEmpty)
        let kinds = (p["options"] as? [[String: Any]] ?? []).compactMap { $0["kind"] as? String }
        XCTAssertEqual(kinds, ["allow_once", "reject_once"])
    }

    func testChunksJoinUntilAnotherKindOrANewTurn() {
        var t = ACPTranscript()
        t.apply(["sessionUpdate": "agent_message_chunk", "content": ["type": "text", "text": "Hel"]])
        t.apply(["sessionUpdate": "agent_message_chunk", "content": ["type": "text", "text": "lo"]])
        t.apply(["sessionUpdate": "agent_thought_chunk", "content": ["type": "text", "text": "hmm"]])
        t.apply(["sessionUpdate": "agent_message_chunk", "content": ["type": "text", "text": "Bye"]])
        t.beginTurn()
        t.apply(["sessionUpdate": "user_message_chunk", "content": ["type": "text", "text": "one"]])
        t.beginTurn()
        t.apply(["sessionUpdate": "user_message_chunk", "content": ["type": "text", "text": "two"]])
        XCTAssertEqual(t.items.count, 5)
        if case .assistant(let text) = t.items[0].kind { XCTAssertEqual(text, "Hello") } else { XCTFail() }
        XCTAssertNotNil(t.conversation().turnStart)
        t.endTurn(stopReason: "cancelled", usage: nil)
        if case .notice(let text, _) = t.items[5].kind { XCTAssertEqual(text, "Stopped") } else { XCTFail() }
    }

    func testDiffTrimsSharedLines() {
        let d = ACPTranscript.diff(old: "a\nb\nc\nd\ne\nf\ng\nh\n", new: "a\nb\nc\nd\nX\nf\ng\nh\n")
        XCTAssertEqual(d.map(\.kind), [.context, .context, .context, .remove, .add, .context, .context, .context])
        XCTAssertEqual(ACPTranscript.diff(old: nil, new: "new\n").map(\.kind), [.add])
    }

    func testSummaryHelpers() {
        XCTAssertEqual(AntigravitySessions.folder(#"["file:///Users/me/proj%20a"]"#), "/Users/me/proj a")
        XCTAssertNotNil(AntigravitySessions.date("2026-11-05 16:57:03.335872+00:00"))
        let options = ACPConfigOption.parse([["id": "model", "name": "Model", "type": "select", "currentValue": "g",
                                              "options": [["value": "g", "name": "Gemini"], ["value": "c", "name": "Claude"]]]])
        XCTAssertEqual(options.first?.currentName, "Gemini")
    }
}
