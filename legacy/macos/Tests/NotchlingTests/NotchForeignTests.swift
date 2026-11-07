import XCTest
@testable import Notchling

/// Codex and Antigravity sessions in the notch: which rows show, in which tab, and how a conversation reads there.
final class NotchForeignTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)

    private func row(_ id: String, minutesAgo: Double = 1, group: AgentGroup = .completed) -> Agent {
        let target: AgentTarget = id.hasPrefix("agy:") ? .antigravity(sessionID: String(id.dropFirst(4)))
                                                       : .codex(sessionID: String(id.dropFirst(6)))
        return Agent(id: id, name: "Title \(id)", project: "proj", cwd: "/tmp/proj", group: group, label: "x", detail: nil,
                     since: now.addingTimeInterval(-minutesAgo * 60), target: target, host: "Codex")
    }

    private func permission(_ options: [(String, String, String)] = [("a", "Allow", "allow_once"), ("r", "Reject", "reject_once")]) -> ACPPermission {
        ACPPermission(rpcID: "1", rpcIDIsNumber: true,
                      call: ToolCall(id: "t", name: "run_command", kind: .shell, title: "Run command", input: "rm -rf build\nsecond"),
                      options: options.map { .init(id: $0.0, name: $0.1, kind: $0.2) })
    }

    // MARK: Grouping

    func testIsForeign() {
        XCTAssertTrue(NotchForeign.isForeign(row("codex:a")))
        XCTAssertTrue(NotchForeign.isForeign(row("agy:a")))
        let claude = Agent(id: "j", name: "n", project: "p", cwd: "/", group: .working, label: "", detail: nil, since: now, target: .job("j"))
        XCTAssertFalse(NotchForeign.isForeign(claude))
    }

    func testMergeSortsTabs() {
        let rows = [row("codex:idle", minutesAgo: 5), row("codex:busy", minutesAgo: 2), row("agy:idle", minutesAgo: 3),
                    row("agy:busy", minutesAgo: 4), row("agy:ask", minutesAgo: 1)]
        let merged = NotchForeign.merge(rows, closed: [:], codexRunning: ["busy"], agyWorking: ["busy"],
                                        permissions: ["agy:ask": permission()], now: now)
        let byID = Dictionary(uniqueKeysWithValues: merged.map { ($0.id, $0) })
        XCTAssertEqual(byID["codex:idle"]?.group, .completed)
        XCTAssertEqual(byID["codex:idle"]?.label, "Done")
        XCTAssertEqual(byID["codex:busy"]?.group, .working)
        XCTAssertEqual(byID["codex:busy"]?.label, "Working")
        XCTAssertEqual(byID["agy:idle"]?.group, .completed)
        XCTAssertEqual(byID["agy:busy"]?.group, .working)
        XCTAssertEqual(byID["agy:ask"]?.group, .needsInput, "a waiting permission puts it under Input")
        XCTAssertEqual(byID["agy:ask"]?.detail, "Run command")
        XCTAssertEqual(merged.map(\.id).first, "agy:ask", "newest first")
    }

    func testCodexRowFromFileKeepsWorking() {
        // The file says a turn is open (another process runs it): the row stays Working without our runner.
        let merged = NotchForeign.merge([row("codex:other", group: .working)], closed: [:], codexRunning: [],
                                        agyWorking: [], permissions: [:], now: now)
        XCTAssertEqual(merged.first?.group, .working)
    }

    func testAntigravityListedWorkingEndsWhenRunnerStops() {
        let merged = NotchForeign.merge([row("agy:a", group: .working)], closed: [:], codexRunning: [], agyWorking: [],
                                        permissions: [:], now: now)
        XCTAssertEqual(merged.first?.group, .completed)
    }

    func testClosedInChatStaysOutUntilUsedAgain() {
        let rows = [row("codex:a", minutesAgo: 30), row("agy:b", minutesAgo: 10)]
        // Closed after codex:a's last use, before agy:b was used again.
        let closed = ["codex:a": now.addingTimeInterval(-60 * 5).timeIntervalSince1970,
                      "agy:b": now.addingTimeInterval(-60 * 20).timeIntervalSince1970]
        let merged = NotchForeign.merge(rows, closed: closed, codexRunning: [], agyWorking: [], permissions: [:], now: now)
        XCTAssertEqual(merged.map(\.id), ["agy:b"])
    }

    func testOldAndExcessRowsAreLeftOut() {
        var rows = [row("codex:old", minutesAgo: 13 * 60)]
        rows += (0..<12).map { row("codex:\($0)", minutesAgo: Double($0 + 1)) }
        let merged = NotchForeign.merge(rows, closed: [:], codexRunning: [], agyWorking: [], permissions: [:], now: now)
        XCTAssertFalse(merged.contains { $0.id == "codex:old" })
        XCTAssertEqual(merged.count, NotchForeign.perTool)
    }

    // MARK: Conversation

    func testLinesFromConversation() {
        var conversation = Conversation()
        let read = ToolCall(id: "1", name: "read_file", kind: .read, title: "Read a.swift", input: "/a.swift")
        let shell = ToolCall(id: "2", name: "run", kind: .shell, title: "Run ls", input: "ls")
        conversation.items = [
            ChatItem(id: "a", kind: .user(text: "fix it", images: 0), date: nil),
            ChatItem(id: "b", kind: .thinking(seconds: 3, text: "hmm"), date: nil),
            ChatItem(id: "c", kind: .tools([read, shell]), date: nil),
            ChatItem(id: "d", kind: .assistant(text: "done"), date: nil),
            ChatItem(id: "e", kind: .turnEnd(seconds: 4, model: nil), date: nil),
            ChatItem(id: "f", kind: .notice(text: "limit", symbol: "x"), date: nil),
        ]
        conversation.activity = "Running ls"
        let lines = NotchForeign.lines(from: conversation)
        XCTAssertEqual(lines.map(\.kind), [.user, .tools, .assistant])
        XCTAssertEqual(lines.map(\.text), ["fix it", "Read 1 file, ran 1 shell command", "done"])
        let snapshot = NotchForeign.snapshot(from: conversation)
        XCTAssertEqual(snapshot.lastPrompt, "fix it")
        XCTAssertEqual(snapshot.lastReply, "done")
        XCTAssertEqual(snapshot.activity, "Running ls")
    }

    func testLinesKeepTheTail() {
        var conversation = Conversation()
        conversation.items = (0..<10).map { ChatItem(id: "\($0)", kind: .assistant(text: "m\($0)"), date: nil) }
        let lines = NotchForeign.lines(from: conversation, limit: 3)
        XCTAssertEqual(lines.map(\.text), ["m7", "m8", "m9"])
        XCTAssertEqual(lines.map(\.id), [7, 8, 9])
    }

    func testEmptyConversationHasNoLines() {
        XCTAssertTrue(NotchForeign.snapshot(from: Conversation()).lines.isEmpty)
    }

    func testCodexRowSnapshotComesFromItsFile() throws {
        let lines = [
            #"{"timestamp":"2026-11-05T08:44:11.961Z","ordinal":0,"type":"session_meta","payload":{"id":"s1","cwd":"/tmp/proj","originator":"codex_exec"}}"#,
            #"{"timestamp":"2026-11-05T08:44:12.004Z","ordinal":1,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Fix the test"}]}}"#,
            #"{"timestamp":"2026-11-05T08:44:14.000Z","ordinal":2,"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"c1","arguments":"{\"cmd\":\"swift test\"}"}}"#,
            #"{"timestamp":"2026-11-05T08:44:15.000Z","ordinal":3,"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#,
            #"{"timestamp":"2026-11-05T08:44:18.000Z","ordinal":4,"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Fixed it."}]}}"#,
        ]
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("notch-foreign-\(UUID().uuidString).jsonl")
        try (lines.joined(separator: "\n") + "\n").write(to: file, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: file) }
        var agent = row("codex:s1")
        agent.transcriptPath = file.path
        let snapshot = NotchForeign.snapshot(for: agent, agy: [:])
        XCTAssertEqual(snapshot.lines.map(\.kind), [.user, .tools, .assistant])
        XCTAssertEqual(snapshot.lastReply, "Fixed it.")
        XCTAssertEqual(snapshot.lastPrompt, "Fix the test")
        // An Antigravity row reads the conversation the runner holds, none while it isn't open.
        let agy = row("agy:z")
        XCTAssertTrue(NotchForeign.snapshot(for: agy, agy: [:]).lines.isEmpty)
        var held = Conversation()
        held.items = [ChatItem(id: "a", kind: .assistant(text: "hi"), date: nil)]
        XCTAssertEqual(NotchForeign.snapshot(for: agy, agy: ["agy:z": held]).lastReply, "hi")
    }

    // MARK: Permission

    func testOptionsListSafeChoiceFirst() {
        let p = permission([("rej", "Reject always", "reject_always"), ("rej1", "Reject", "reject_once"),
                            ("alw", "Allow always", "allow_always"), ("one", "Allow once", "allow_once")])
        XCTAssertEqual(NotchForeign.orderedOptions(p).map(\.id), ["one", "alw", "rej1", "rej"])
    }

    func testPreviewShowsDiffOrInput() {
        let plain = permission()
        XCTAssertEqual(NotchForeign.preview(of: plain).map(\.text), ["rm -rf build", "second"])
        let edit = ACPPermission(rpcID: "2", rpcIDIsNumber: false,
                                 call: ToolCall(id: "t", name: "edit", kind: .edit, title: "Edit a.swift", input: "/a.swift",
                                                diff: [DiffLine(kind: .remove, text: "old", number: 1), DiffLine(kind: .add, text: "new", number: 1)]),
                                 options: [])
        let preview = NotchForeign.preview(of: edit)
        XCTAssertEqual(preview.map(\.text), ["old", "new"])
        XCTAssertEqual(preview.map(\.kind), [.remove, .add])
        let long = ACPPermission(rpcID: "3", rpcIDIsNumber: false,
                                 call: ToolCall(id: "t", name: "run", kind: .shell, title: "Run", input: (0..<30).map(String.init).joined(separator: "\n")),
                                 options: [])
        XCTAssertEqual(NotchForeign.preview(of: long, maxLines: 8).count, 8)
    }

    func testPermissionHeightGrowsWithOptions() {
        let two = permission()
        let four = permission([("a", "A", "allow_once"), ("b", "B", "allow_always"), ("c", "C", "reject_once"), ("d", "D", "reject_always")])
        XCTAssertGreaterThan(NotchForeign.permissionHeight(four), NotchForeign.permissionHeight(two))
    }

    // MARK: Model

    func testModelCountsAndAnswersPermission() {
        let model = NotchModel()
        let claude = Agent(id: "j", name: "n", project: "p", cwd: "/", group: .working, label: "", detail: nil, since: now, target: .job("j"))
        let ask = Agent(id: "agy:ask", name: "ask", project: "p", cwd: "/", group: .needsInput, label: "Needs permission", detail: nil,
                        since: now, target: .antigravity(sessionID: "ask"), host: "Antigravity")
        model.agents = [claude]
        model.foreignAgents = [ask, row("codex:a")]
        model.foreignPermissions = ["agy:ask": permission()]
        XCTAssertEqual(model.listed.count, 3)
        XCTAssertEqual(model.workingCount, 1)
        XCTAssertEqual(model.needsCount, 1)
        XCTAssertNotNil(model.agent("agy:ask"))
        XCTAssertNotNil(model.agyPermission("agy:ask"))
        XCTAssertTrue(model.completions(for: ask).isEmpty, "Claude's slash commands don't apply to Antigravity")
        XCTAssertFalse(model.canReplyInPeek("agy:ask"), "a permission is answered, not replied to")
        // Taller than a plain peek, and the claude-only path untouched.
        model.notchSize = CGSize(width: 185, height: 32)
        XCTAssertGreaterThan(model.size(for: .peek("agy:ask")).height, model.size(for: .peek("j")).height)
    }
}
