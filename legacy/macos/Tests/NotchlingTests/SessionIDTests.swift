import XCTest
@testable import Notchling

final class SessionIDTests: XCTestCase {
    private func agent(_ target: AgentTarget, transcript: String? = nil) -> Agent {
        var a = Agent(id: "x", name: "n", project: "p", cwd: "/tmp", group: .completed, label: "Idle",
                      detail: nil, since: Date(), target: target)
        a.transcriptPath = transcript
        return a
    }

    func testIDs() {
        XCTAssertEqual(agent(.job("ab12"), transcript: "/h/.claude/projects/p/1f2e-uuid.jsonl").sessionID, "1f2e-uuid")
        XCTAssertNil(agent(.job("ab12")).sessionID)
        XCTAssertEqual(agent(.resume(sessionID: "s1", cwd: "/tmp")).sessionID, "s1")
        XCTAssertEqual(agent(.codex(sessionID: "c1")).sessionID, "c1")
        XCTAssertNil(agent(.codex(sessionID: "")).sessionID)
        XCTAssertNil(agent(.antigravity(sessionID: "")).sessionID)
    }
}
