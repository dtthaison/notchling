import XCTest
@testable import Notchling

/// Issue #7: a sync that took its snapshot before a removal must not bring the account back.
final class AccountRemoveRaceTests: XCTestCase {
    func testRemovalAfterTheSnapshotWins() {
        // snapshot 3 taken, then "a" removed at 4: writing snapshot 3 back drops "a"
        XCTAssertEqual(AccountStore.removedSince(3, ["a": 4]), ["a"])
    }

    func testRemovalBeforeTheSnapshotDoesNotHideAReadd() {
        // "a" removed at 2, then added again by work whose snapshot is 5: it stays
        XCTAssertEqual(AccountStore.removedSince(5, ["a": 2]), [])
    }

    func testOnlyLaterRemovalsCount() {
        XCTAssertEqual(AccountStore.removedSince(4, ["a": 2, "b": 5, "c": 6]), ["b", "c"])
    }
}
