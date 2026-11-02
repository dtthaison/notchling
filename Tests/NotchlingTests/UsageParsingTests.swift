import XCTest
@testable import Notchling

/// `/api/oauth/usage` responses into `AccountUsage`. The endpoint changes without notice, so both the
/// fixed keys and the `limits` list are pinned here.
final class UsageParsingTests: XCTestCase {
    func testFixedKeys() {
        let usage = AccountStore.parseUsage([
            "five_hour": ["utilization": 42.0, "resets_at": "2026-11-01T15:00:00Z"],
            "seven_day": ["utilization": 10.0],
            "seven_day_opus": ["utilization": 3.0],
        ])
        XCTAssertEqual(usage.session?.percent, 42)
        XCTAssertNotNil(usage.session?.resetsAt)
        XCTAssertEqual(usage.week?.percent, 10)
        XCTAssertEqual(usage.models.map(\.name), ["Opus"])
    }

    /// Max 20x from October 2026: `five_hour` is null and everything comes from `limits`.
    func testLimitsListFillsMissingSession() {
        let usage = AccountStore.parseUsage([
            "five_hour": NSNull(),
            "limits": [
                ["kind": "five_hour", "group": "session", "percent": 61.0, "resets_at": "2026-11-01T15:00:00Z"],
                ["kind": "seven_day", "group": "weekly", "percent": 30.0],
                ["kind": "seven_day", "group": "weekly", "percent": 12.0],
                ["kind": "model_week", "group": "weekly", "percent": 70.0,
                 "scope": ["model": ["display_name": "Opus"]]],
                ["kind": "daily_requests", "group": "daily", "percent": 5.0],
                ["kind": "no_percent", "group": "weekly"],
            ],
        ])
        XCTAssertEqual(usage.session?.percent, 61)
        XCTAssertEqual(usage.week?.percent, 30, "the higher of two weekly limits wins")
        XCTAssertEqual(usage.models.map(\.name), ["Opus"])
        XCTAssertEqual(usage.models.first?.window.percent, 70)
        XCTAssertEqual(usage.other.map(\.name), ["Daily Requests"])
    }

    func testScopedLimitReplacesFixedModelKey() {
        let usage = AccountStore.parseUsage([
            "seven_day_opus": ["utilization": 3.0],
            "limits": [["kind": "x", "percent": 50.0, "scope": ["model": ["display_name": "Opus"]]]],
        ])
        XCTAssertEqual(usage.models.count, 1)
        XCTAssertEqual(usage.models.first?.window.percent, 50)
    }

    func testEmptyResponse() {
        let usage = AccountStore.parseUsage([:])
        XCTAssertNil(usage.session)
        XCTAssertNil(usage.week)
        XCTAssertTrue(usage.models.isEmpty && usage.other.isEmpty)
    }
}
