import ConsoleCore
import XCTest
@testable import Notchling

/// Codex and Antigravity sessions mapped onto the phone bridge's session and chat shapes.
final class ForeignSessionsTests: XCTestCase {
    private func agent(_ id: String, since: Date = Date(), group: AgentGroup = .completed, target: AgentTarget? = nil) -> Agent {
        let parsed = ForeignSessions.parse(id)
        let target = target ?? (parsed?.tool == .agy ? .antigravity(sessionID: parsed?.sessionID ?? "") : .codex(sessionID: parsed?.sessionID ?? ""))
        return Agent(id: id, name: "Title \(id)", project: "proj", cwd: "/tmp/proj", group: group, label: "", detail: nil,
                     since: since, target: target, transcriptPath: id.hasPrefix("codex:") ? "/tmp/rollout-x.jsonl" : nil, host: "Codex")
    }

    func testParseRowIDs() {
        XCTAssertEqual(ForeignSessions.parse("codex:abc")?.tool, .codex)
        XCTAssertEqual(ForeignSessions.parse("codex:abc")?.sessionID, "abc")
        XCTAssertEqual(ForeignSessions.parse("agy:123-4")?.tool, .agy)
        XCTAssertNil(ForeignSessions.parse("agy:"))
        XCTAssertNil(ForeignSessions.parse("3f2a9c1e-1111-2222-3333-444444444444"), "a Claude Code session is not foreign")
    }

    func testSessionMapping() throws {
        let since = Date(timeIntervalSince1970: 1_800_000_000)
        let running = try XCTUnwrap(ForeignSessions.session(from: agent("codex:abc", since: since), working: true))
        XCTAssertEqual(running.tool, "codex")
        XCTAssertEqual(running.state, .running)
        XCTAssertEqual(running.control, .job("codex:abc"))
        XCTAssertEqual(running.transcriptURL?.path, "/tmp/rollout-x.jsonl")
        XCTAssertEqual(running.project, "proj")
        let idle = try XCTUnwrap(ForeignSessions.session(from: agent("agy:9"), working: false))
        XCTAssertEqual(idle.tool, "agy")
        XCTAssertEqual(idle.state, .done)
        XCTAssertNil(ForeignSessions.session(from: agent("not-foreign", target: .job("x")), working: false))
        // The wire: `tool` is additive and Claude Code sessions say so too.
        XCTAssertEqual(SessionDTO(running).tool, "codex")
        let claude = Session(id: "c", pid: nil, name: "n", project: "p", cwd: "/", entrypoint: .cli, kind: "interactive",
                             state: .done, statusUpdatedAt: Date(), startedAt: Date(), transcriptURL: nil, control: .viewOnly)
        XCTAssertEqual(SessionDTO(claude).tool, "claude")
    }

    func testPermissionMakesItNeedInput() throws {
        let permission = ACPPermission(rpcID: "7", rpcIDIsNumber: true,
                                       call: ToolCall(id: "t", name: "run_command", kind: .shell, title: "Run command", input: "rm -rf build\nsecond"),
                                       options: [.init(id: "a1", name: "Allow", kind: "allow_once"), .init(id: "r1", name: "Reject", kind: "reject_once")])
        let prompt = ForeignSessions.prompt(for: permission, rowID: "agy:S1", created: Date())
        XCTAssertEqual(prompt.id, "agy-perm:agy:S1:7")
        XCTAssertEqual(prompt.sessionId, "agy:S1")
        XCTAssertEqual(prompt.kind, .permission)
        XCTAssertEqual(prompt.summary, "Run command: rm -rf build")
        let parsed = try XCTUnwrap(ForeignSessions.parsePrompt(prompt.id))
        XCTAssertEqual(parsed.rowID, "agy:S1")
        XCTAssertEqual(parsed.rpcID, "7")
        let session = try XCTUnwrap(ForeignSessions.session(from: agent("agy:S1"), working: true, pending: prompt))
        XCTAssertEqual(session.state, .needsInput)
        XCTAssertEqual(session.pending, prompt)
        XCTAssertNil(ForeignSessions.parsePrompt("something-else"))
        // A string rpc id may hold colons.
        XCTAssertEqual(ForeignSessions.parsePrompt("agy-perm:agy:S1:a:b")?.rpcID, "a:b")
    }

    func testPermissionAnswers() {
        let call = ToolCall(id: "t", name: "x", kind: .other, title: "x", input: "")
        let permission = ACPPermission(rpcID: "1", rpcIDIsNumber: false, call: call,
                                       options: [.init(id: "always", name: "Always", kind: "allow_always"), .init(id: "once", name: "Once", kind: "allow_once"),
                                                 .init(id: "no", name: "No", kind: "reject_once")])
        XCTAssertEqual(ForeignSessions.option(for: .allow, in: permission), "once", "the one-time allow, never the standing one")
        XCTAssertEqual(ForeignSessions.option(for: .deny, in: permission), "no")
        let onlyAlways = ACPPermission(rpcID: "1", rpcIDIsNumber: false, call: call, options: [.init(id: "always", name: "Always", kind: "allow_always")])
        XCTAssertEqual(ForeignSessions.option(for: .allow, in: onlyAlways), "always")
        XCTAssertNil(ForeignSessions.option(for: .deny, in: onlyAlways), "no reject option: the turn is cancelled")
    }

    func testVisibleHidesClosedOldAndExcess() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let fresh = agent("codex:a", since: now.addingTimeInterval(-60))
        let closed = agent("codex:b", since: now.addingTimeInterval(-120))
        let reopened = agent("codex:c", since: now.addingTimeInterval(-30))
        let old = agent("agy:old", since: now.addingTimeInterval(-9 * 86_400))
        let list = ForeignSessions.visible([closed, old, fresh, reopened], closed: [
            "codex:b": closed.since.timeIntervalSince1970 + 1,      // closed after its last use: hidden
            "codex:c": reopened.since.timeIntervalSince1970 - 100,  // used again since: back
        ], now: now)
        XCTAssertEqual(list.map(\.id), ["codex:c", "codex:a"], "newest first")
        let many = (0..<5).map { agent("agy:\($0)", since: now.addingTimeInterval(Double(-$0))) }
        XCTAssertEqual(ForeignSessions.visible(many, closed: [:], now: now, perTool: 2).map(\.id), ["agy:0", "agy:1"])
    }

    func testMessagesFromConversation() throws {
        let date = Date(timeIntervalSince1970: 1_800_000_000)
        var conversation = Conversation()
        conversation.items = [
            ChatItem(id: "u1", kind: .user(text: "Fix it", images: 0), date: date),
            ChatItem(id: "th", kind: .thinking(seconds: 0, text: "hmm"), date: nil),
            ChatItem(id: "t1", kind: .tools([
                ToolCall(id: "c1", name: "Bash", kind: .shell, title: "swift test", input: "swift test", output: "1 failure", finished: true),
                ToolCall(id: "c2", name: "Edit", kind: .edit, title: "Edit parser.swift", input: "/tmp/proj/parser.swift",
                         diff: [DiffLine(kind: .hunk, text: "@@", number: nil), DiffLine(kind: .remove, text: "a", number: nil),
                                DiffLine(kind: .add, text: "b", number: nil)]),
            ]), date: nil),
            ChatItem(id: "a1", kind: .assistant(text: "Done"), date: nil),
            ChatItem(id: "end", kind: .turnEnd(seconds: 3, model: nil), date: nil),
        ]
        let messages = ForeignSessions.messages(from: conversation, sessionID: "codex:s", fallbackDate: date, holdTail: false)
        XCTAssertEqual(messages.map(\.role), [.user, .tools, .assistant], "thinking and turn ends are not sent")
        XCTAssertEqual(messages[1].toolSummary, "Ran 1 shell command, edited 1 file")
        XCTAssertEqual(messages[1].steps.map(\.name), ["Bash", "Edit"])
        XCTAssertEqual(messages[1].steps[0].output, "1 failure")
        XCTAssertEqual(messages[1].steps[1].diff, [DiffRow(.removed, "a"), DiffRow(.added, "b")], "hunk headers are dropped")
        XCTAssertEqual(messages[1].steps[1].path, "/tmp/proj/parser.swift")
        XCTAssertEqual(messages[2].timestamp, date, "an item with no date takes the one before it")
        // Same ids on a second read, and different ids for another session.
        let again = ForeignSessions.messages(from: conversation, sessionID: "codex:s", fallbackDate: date, holdTail: false)
        XCTAssertEqual(messages.map(\.id), again.map(\.id))
        let other = ForeignSessions.messages(from: conversation, sessionID: "codex:t", fallbackDate: date, holdTail: false)
        XCTAssertNotEqual(messages.map(\.id), other.map(\.id))
        // The wire shape the bridge already sends.
        let dto = ChatMessageDTO(messages[1])
        XCTAssertEqual(dto.role, "tools")
        XCTAssertEqual(dto.steps?.count, 2)
    }

    func testHoldTailKeepsAGrowingReplyBack() {
        var conversation = Conversation()
        conversation.items = [ChatItem(id: "u", kind: .user(text: "q", images: 0), date: nil),
                              ChatItem(id: "a", kind: .assistant(text: "partial"), date: nil)]
        XCTAssertEqual(ForeignSessions.messages(from: conversation, sessionID: "agy:s", fallbackDate: Date(), holdTail: true).map(\.role), [.user])
        XCTAssertEqual(ForeignSessions.messages(from: conversation, sessionID: "agy:s", fallbackDate: Date(), holdTail: false).map(\.role), [.user, .assistant])
        conversation.items.append(ChatItem(id: "e", kind: .turnEnd(seconds: 1, model: nil), date: nil))
        XCTAssertEqual(ForeignSessions.messages(from: conversation, sessionID: "agy:s", fallbackDate: Date(), holdTail: true).count, 2,
                       "once the turn has ended the reply is whole")
    }

    func testEnrichTakesLastReplyAndContext() throws {
        var conversation = Conversation()
        conversation.items = [ChatItem(id: "a1", kind: .assistant(text: "  first "), date: nil),
                              ChatItem(id: "a2", kind: .assistant(text: " latest\n"), date: nil)]
        conversation.facts.contextTokens = 1234
        conversation.facts.model = "gpt-6"
        var session = try XCTUnwrap(ForeignSessions.session(from: agent("codex:s"), working: false))
        ForeignSessions.enrich(&session, with: conversation)
        XCTAssertEqual(session.lastMessage?.text, "latest")
        XCTAssertEqual(session.facts?.contextTokens, 1234)
        XCTAssertEqual(session.facts?.model, "gpt-6")
        XCTAssertNotNil(UUID(uuidString: session.lastMessage?.id ?? ""))
    }
}
