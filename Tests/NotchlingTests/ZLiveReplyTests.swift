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
    /// Prints how the Chat window would show a real transcript:
    /// `NOTCHLING_LIVE_TRANSCRIPT=<path.jsonl> swift test --filter ZLiveReplyTests`
    func testShowTranscript() throws {
        guard let path = ProcessInfo.processInfo.environment["NOTCHLING_LIVE_TRANSCRIPT"] else {
            throw XCTSkip("set NOTCHLING_LIVE_TRANSCRIPT")
        }
        for item in ConversationReader.read(path).items { print("ITEM", item.kind) }
    }
}
