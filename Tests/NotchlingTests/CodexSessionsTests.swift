import XCTest
@testable import Notchling

/// Codex session files (`rollout-*.jsonl`, CLI 0.166) read as Chat conversations.
final class CodexSessionsTests: XCTestCase {
    var file: URL!

    private let lines = [
        #"{"timestamp":"2026-11-05T08:44:11.961Z","ordinal":0,"type":"session_meta","payload":{"id":"01a1aae1-432d","cwd":"/tmp/proj","originator":"codex_exec"}}"#,
        #"{"timestamp":"2026-11-05T08:44:12.000Z","ordinal":1,"type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#,
        #"{"timestamp":"2026-11-05T08:44:12.001Z","ordinal":2,"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"You are Codex"}]}}"#,
        #"{"timestamp":"2026-11-05T08:44:12.002Z","ordinal":3,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>\n<cwd>/tmp/proj</cwd>\n</environment_context>"}]}}"#,
        #"{"timestamp":"2026-11-05T08:44:12.003Z","ordinal":4,"type":"turn_context","payload":{"turn_id":"t1","model":"gpt-6-luna"}}"#,
        #"{"timestamp":"2026-11-05T08:44:12.004Z","ordinal":5,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Fix the failing test\nin parser.swift"}]}}"#,
        #"{"timestamp":"2026-11-05T08:44:13.000Z","ordinal":6,"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Looking at the test first."}]}}"#,
        #"{"timestamp":"2026-11-05T08:44:14.000Z","ordinal":7,"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"c1","arguments":"{\"cmd\":\"swift test\"}"}}"#,
        #"{"timestamp":"2026-11-05T08:44:15.000Z","ordinal":8,"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"Process exited with code 1\nOutput:\n1 failure"}}"#,
        #"{"timestamp":"2026-11-05T08:44:16.000Z","ordinal":9,"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c2","input":"*** Begin Patch\n*** Update File: Sources/parser.swift\n@@\n-let a = 1\n+let a = 2\n*** End Patch"}}"#,
        #"{"timestamp":"2026-11-05T08:44:17.000Z","ordinal":10,"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c2","output":"Success. Updated the following files:\nM Sources/parser.swift"}}"#,
        #"{"timestamp":"2026-11-05T08:44:18.000Z","ordinal":11,"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Fixed it."}]}}"#,
        #"{"timestamp":"2026-11-05T08:44:18.100Z","ordinal":12,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"output_tokens":50},"last_token_usage":{"input_tokens":900,"output_tokens":40}}}}"#,
        #"{"timestamp":"2026-11-05T08:44:18.200Z","ordinal":13,"type":"event_msg","payload":{"type":"task_complete","turn_id":"t1","duration_ms":6200}}"#,
    ]

    override func setUpWithError() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("codex-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        file = dir.appendingPathComponent("rollout-2026-11-05T15-44-11-01a1aae1-432d.jsonl")
        try write(lines)
    }

    override func tearDownWithError() throws { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }

    private func write(_ lines: [String]) throws {
        try lines.joined(separator: "\n").appending("\n").write(to: file, atomically: true, encoding: .utf8)
    }

    func testMetaAndTitleSkipInjectedMessages() {
        let meta = CodexSessions.meta(file.path)
        XCTAssertEqual(meta?.id, "01a1aae1-432d")
        XCTAssertEqual(meta?.cwd, "/tmp/proj")
        XCTAssertEqual(meta?.title, "Fix the failing test", "first line of what you typed, not the environment context")
    }

    func testConversation() {
        let c = CodexReader.read(file.path)
        let kinds = c.items.map { item -> String in
            switch item.kind {
            case .user(let t, _): return "user:" + t
            case .assistant(let t): return "assistant:" + t
            case .thinking(_, let t): return "thinking:" + t
            case .tools(let calls): return "tools:" + calls.map(\.title).joined(separator: ",")
            case .turnEnd(let s, let m): return "end:\(s):\(m ?? "")"
            default: return "other"
            }
        }
        XCTAssertEqual(kinds, ["user:Fix the failing test\nin parser.swift", "thinking:Looking at the test first.",
                               "tools:swift test,Edit parser.swift", "assistant:Fixed it.", "end:6.2:gpt-6-luna"])
        guard case .tools(let calls) = c.items[2].kind else { return XCTFail() }
        XCTAssertEqual(calls[0].kind, .shell)
        XCTAssertTrue(calls[0].isError, "exit code 1")
        XCTAssertEqual(calls[1].kind, .edit)
        XCTAssertEqual(calls[1].added, 1)
        XCTAssertEqual(calls[1].removed, 1)
        XCTAssertTrue(calls.allSatisfy(\.finished))
        XCTAssertEqual(c.facts.model, "gpt-6-luna")
        XCTAssertEqual(c.facts.contextTokens, 940)
        XCTAssertEqual(c.facts.files, ["Sources/parser.swift"])
        XCTAssertNil(c.turnStart, "the turn ended")
        XCTAssertFalse(CodexSessions.turnOpen(file.path))
    }

    func testAnOpenTurnShowsWhatItsDoing() throws {
        try write(Array(lines.prefix(8)))
        let c = CodexReader.read(file.path)
        XCTAssertNotNil(c.turnStart)
        XCTAssertEqual(c.activity, "swift test")
        XCTAssertTrue(CodexSessions.turnOpen(file.path))
    }

    func testExecArguments() {
        XCTAssertEqual(CodexRunner.arguments(prompt: "hi", session: nil, folder: "/p"),
                       ["exec", "--json", "--skip-git-repo-check", "-c", "sandbox_mode=\"workspace-write\"", "-C", "/p", "hi"])
        XCTAssertEqual(CodexRunner.arguments(prompt: "more", session: "abc", folder: "/p"),
                       ["exec", "resume", "--json", "--skip-git-repo-check", "-c", "sandbox_mode=\"workspace-write\"", "abc", "more"])
    }

    func testSessionChoiceKeys() {
        for choice in [SessionChoice.claude(accountID: nil), .claude(accountID: "a1"), .codex(accountID: "u:w")] {
            XCTAssertEqual(SessionChoice(key: choice.key), choice)
        }
    }
}
