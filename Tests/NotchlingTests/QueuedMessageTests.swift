import XCTest
@testable import Notchling

/// Messages sent while a session works, as Claude Code 2.1.318 logs them (shapes from real sessions).
final class QueuedMessageTests: XCTestCase {
    private func read(_ lines: [String]) throws -> Conversation {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("queued-\(UUID().uuidString).jsonl")
        try (lines.joined(separator: "\n") + "\n").write(to: url, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: url) }
        return ConversationReader.read(url.path)
    }

    private let user = #"{"type":"user","message":{"role":"user","content":"Write a story."},"timestamp":"2026-11-04T04:26:51.000Z"}"#

    /// The effort on the latest reply is the session's (Claude Code 2.1.318 writes it on assistant lines).
    func testEffortFromTheLatestReply() throws {
        let c = try read([
            user,
            #"{"type":"assistant","effort":"medium","message":{"id":"m1","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"Once"}]},"timestamp":"2026-11-04T04:26:52.000Z"}"#,
            #"{"type":"assistant","effort":"xhigh","message":{"id":"m2","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"upon"}]},"timestamp":"2026-11-04T04:26:53.000Z"}"#,
        ])
        XCTAssertEqual(c.facts.effort, "xhigh")
    }

    /// Waiting behind the turn: queued, not yet a message.
    func testEnqueuedIsQueued() throws {
        let c = try read([
            user,
            #"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:27:12.000Z","content":"After the story, say fifth."}"#,
            #"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:27:13.000Z","content":"<task-notification> <task-id>b1</task-id></task-notification>"}"#,
        ])
        XCTAssertEqual(c.queued, ["After the story, say fifth."])
        XCTAssertEqual(c.items.count, 1)
        XCTAssertTrue(ChatModel.Pending(text: "After the story,  say fifth.", at: Date(), sending: false).landed(in: c))
    }

    /// Taken in mid-turn: an attachment, no user line. It used to vanish from Chat.
    func testAbsorbedMidTurnShowsAsYourMessage() throws {
        let c = try read([
            user,
            #"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:14:02.751Z","content":"support /model"}"#,
            #"{"type":"queue-operation","operation":"remove","timestamp":"2026-11-04T04:15:26.108Z","content":"support /model","reason":"absorbed_mid_turn"}"#,
            #"{"type":"attachment","attachment":{"type":"queued_command","prompt":"support /model","commandMode":"prompt","origin":{"kind":"human"},"humanTurn":true},"timestamp":"2026-11-04T04:14:02.641Z"}"#,
            #"{"type":"attachment","attachment":{"type":"queued_command","prompt":"<task-notification>done</task-notification>","commandMode":"task-notification"},"timestamp":"2026-11-04T04:14:03.000Z"}"#,
        ])
        XCTAssertEqual(c.queued, [])
        XCTAssertEqual(c.items.map(\.kind).last, .user(text: "support /model", images: 0))
        XCTAssertEqual(c.items.count, 2)
    }

    /// Run at the turn's end: dequeued, then an ordinary user line.
    func testDequeuedLeavesTheQueue() throws {
        let c = try read([
            user,
            #"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:27:12.000Z","content":"fifth"}"#,
            #"{"type":"queue-operation","operation":"dequeue","timestamp":"2026-11-04T04:27:32.000Z"}"#,
            #"{"type":"user","message":{"role":"user","content":"fifth"},"timestamp":"2026-11-04T04:27:32.100Z"}"#,
        ])
        XCTAssertEqual(c.queued, [])
        XCTAssertEqual(c.items.map(\.kind).last, .user(text: "fifth", images: 0))
    }

    /// A message that starts with a paste was hidden as if it were an injected wrapper.
    func testMessageStartingWithAPasteShows() throws {
        let c = try read([#"{"type":"user","message":{"role":"user","content":"<pasted_content id=\"a1\">x\ny</pasted_content> fix this"},"timestamp":"2026-11-04T04:00:00.000Z"}"#])
        XCTAssertEqual(c.items.map(\.kind), [.user(text: "[Pasted text +2 lines] fix this", images: 0)])
    }

    /// Clearing the box never sends Esc or Ctrl+C, which would interrupt a busy session, nor Ctrl+K,
    /// which a burst turns into a literal line break.
    func testClearInputNeverInterrupts() {
        XCTAssertFalse(ReplySender.clearInput.contains("\u{1b}"))
        XCTAssertFalse(ReplySender.clearInput.contains("\u{03}"))
        XCTAssertFalse(ReplySender.clearInput.contains("\u{0B}"))
    }
}
