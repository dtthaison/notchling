import XCTest
@testable import Notchling

/// A session moved to another account is resumed there under the same job id; the stopped copy it left
/// must not win over the new one.
final class MovedJobTests: XCTestCase {
    func testNewestCopyFirst() {
        let old: [String: Any] = ["createdAt": "2026-10-28T08:20:11.786Z", "state": "stopped"]
        let moved: [String: Any] = ["createdAt": "2026-11-02T08:21:08.279Z", "state": "working"]
        XCTAssertEqual(AgentMonitor.newestFirst([(nil, old), ("/lanes/b", moved)]), ["/lanes/b", nil])
        // Moved back to the main login: main's copy is the newest again.
        let back: [String: Any] = ["createdAt": "2026-11-03T10:00:00.000Z"]
        XCTAssertEqual(AgentMonitor.newestFirst([(nil, back), ("/lanes/b", moved)]), [nil, "/lanes/b"])
    }

    func testMissingDateSortsLast() {
        let dated: [String: Any] = ["createdAt": "2026-11-02T08:21:08.279Z"]
        XCTAssertEqual(AgentMonitor.newestFirst([(nil, [:]), ("/lanes/b", dated)]), ["/lanes/b", nil])
    }
}
