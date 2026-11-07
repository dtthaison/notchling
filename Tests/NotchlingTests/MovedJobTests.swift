import XCTest
@testable import Notchling

/// A session moved to another account is resumed there under the same job id; the stopped copy it left
/// must not win over the new one.
final class MovedJobTests: XCTestCase {
    func testLiveCopyFirst() {
        let old: [String: Any] = ["createdAt": "2026-10-28T08:20:11.786Z", "updatedAt": "2026-11-02T08:21:06.159Z", "state": "stopped"]
        let moved: [String: Any] = ["createdAt": "2026-11-02T08:21:08.279Z", "updatedAt": "2026-11-03T08:00:00.000Z", "state": "working"]
        XCTAssertEqual(AgentMonitor.newestFirst([(nil, old), ("/lanes/b", moved)]), ["/lanes/b", nil])
        // Moved back: the old main copy is woken (created in October, still) and the lane's is stopped.
        let woken: [String: Any] = ["createdAt": "2026-10-28T08:20:11.786Z", "updatedAt": "2026-11-04T03:22:20.219Z", "state": "running"]
        let left: [String: Any] = ["createdAt": "2026-11-02T08:21:08.279Z", "updatedAt": "2026-11-04T03:15:40.662Z", "state": "stopped"]
        XCTAssertEqual(AgentMonitor.newestFirst([(nil, woken), ("/lanes/b", left)]), [nil, "/lanes/b"])
    }

    func testBothStoppedNewestUpdateFirst() {
        let a: [String: Any] = ["updatedAt": "2026-11-02T08:21:06.159Z", "state": "stopped"]
        let b: [String: Any] = ["updatedAt": "2026-11-04T03:15:40.662Z", "state": "stopped"]
        XCTAssertEqual(AgentMonitor.newestFirst([(nil, a), ("/lanes/b", b)]), ["/lanes/b", nil])
        XCTAssertEqual(AgentMonitor.newestFirst([(nil, [:]), ("/lanes/b", b)]), [nil, "/lanes/b"])
    }
}
