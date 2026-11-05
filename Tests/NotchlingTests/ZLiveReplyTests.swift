import XCTest
@testable import Notchling

/// Types into a real background session. Skipped unless asked for, since it needs one:
/// `claude --bg -n probe "Reply with just OK."`, then
/// `NOTCHLING_LIVE_JOB=<id> NOTCHLING_LIVE_TEXT='!echo hi' swift test --filter ZLiveReplyTests`
final class ZLiveReplyTests: XCTestCase {
    func testSend() throws {
        let env = ProcessInfo.processInfo.environment
        guard let job = env["NOTCHLING_LIVE_JOB"], let text = env["NOTCHLING_LIVE_TEXT"] else {
            throw XCTSkip("set NOTCHLING_LIVE_JOB and NOTCHLING_LIVE_TEXT")
        }
        try ReplySender.send(text, toJob: job)
    }
}

extension ZLiveReplyTests {
    /// Opens the plugin menu in Chat's terminal on a real session, then resizes it:
    /// `NOTCHLING_LIVE_JOB=<id> swift test --filter ZLiveReplyTests/testTerminal`
    func testTerminal() throws {
        guard let job = ProcessInfo.processInfo.environment["NOTCHLING_LIVE_JOB"] else {
            throw XCTSkip("set NOTCHLING_LIVE_JOB")
        }
        let session = TerminalSession(jobID: job, title: "probe", rows: 30, cols: 100)
        try session.start(typing: "/plugin")
        func wait(_ seconds: TimeInterval, until done: () -> Bool) -> Bool {
            let end = Date().addingTimeInterval(seconds)
            while Date() < end {
                if done() { return true }
                RunLoop.main.run(until: Date().addingTimeInterval(0.1))
            }
            return done()
        }
        let text = { session.screen.lines.joined(separator: "\n") }
        XCTAssertTrue(wait(15) { text().contains("Discover plugins") }, "plugin menu opened:\n" + text())

        session.resize(rows: 24, cols: 70)
        // The search box is redrawn for 70 columns, right edge included.
        let boxed = { session.screen.lines.contains { $0.hasSuffix("╮") && $0.count <= 70 } }
        XCTAssertTrue(wait(5) { boxed() && text().contains("Discover plugins") }, "redrawn at 70 columns:\n" + text())
        print("TERMINAL\n" + text())

        session.send("\u{1b}")
        XCTAssertTrue(wait(5) { !text().contains("Discover plugins") }, "Esc closed the menu:\n" + text())
        session.close()
    }

    /// Prints how the Chat window would show a real transcript:
    /// `NOTCHLING_LIVE_TRANSCRIPT=<path.jsonl> swift test --filter ZLiveReplyTests`
    func testShowTranscript() throws {
        guard let path = ProcessInfo.processInfo.environment["NOTCHLING_LIVE_TRANSCRIPT"] else {
            throw XCTSkip("set NOTCHLING_LIVE_TRANSCRIPT")
        }
        for item in ConversationReader.read(path).items { print("ITEM", item.kind) }
    }
}
