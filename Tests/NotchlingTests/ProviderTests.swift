import SwiftUI
import XCTest
@testable import Notchling

final class UsageReportTests: XCTestCase {
    private let reset = Date(timeIntervalSince1970: 1_800_000_000)

    func testClaudeUsageMapsToOneUnnamedGroupInCardOrder() {
        let usage = AccountUsage(
            session: UsageWindow(percent: 12, resetsAt: reset),
            week: UsageWindow(percent: 40, resetsAt: nil),
            models: [(name: "Opus", window: UsageWindow(percent: 70, resetsAt: reset))],
            other: [(name: "Design", window: UsageWindow(percent: 5, resetsAt: nil))])
        let report = UsageReport(usage)
        XCTAssertEqual(report.groups.count, 1)
        XCTAssertNil(report.groups[0].name)
        // The card's order: 5 hours, week, limits the app doesn't know, then per-model weekly limits.
        XCTAssertEqual(report.bars.map(\.label), ["5 hours", "Week", "Design", "Opus"])
        XCTAssertEqual(report.bars.map(\.kind), [.session, .week, .other, .week])
        XCTAssertEqual(report.bars.map(\.percent), [12, 40, 5, 70])
        XCTAssertEqual(report.bars[0].resetsAt, reset)
        XCTAssertNil(report.bars[1].resetsAt)
        XCTAssertEqual(report.fetched, usage.fetched)
    }

    func testClaudeUsageWithNoLimitsIsEmpty() {
        XCTAssertTrue(UsageReport(AccountUsage()).isEmpty)
    }

    func testClaudeUsageWithOnlyAWeekHasNoSessionBar() {
        let report = UsageReport(AccountUsage(session: nil, week: UsageWindow(percent: 3, resetsAt: nil)))
        XCTAssertEqual(report.bars.map(\.kind), [.week])
    }

    func testGroupedUsageKeepsGroupsAndClassifiesBarsByLabel() {
        let window = UsageWindow(percent: 25, resetsAt: reset)
        let grouped = GroupedUsage(groups: [
            .init(name: "Codex", bars: [("5h", window), ("Week", window)]),
            .init(name: "Code Review", bars: [("Month", window), ("3h", window), ("Limit", window)]),
        ])
        let report = UsageReport(grouped)
        XCTAssertEqual(report.groups.map(\.name), ["Codex", "Code Review"])
        XCTAssertEqual(report.groups[0].bars.map(\.kind), [.session, .week])
        XCTAssertEqual(report.groups[1].bars.map(\.kind), [.other, .other, .other])
        XCTAssertEqual(report.groups[1].bars.map(\.label), ["Month", "3h", "Limit"])
        XCTAssertEqual(report.bars[0].window, window)
        XCTAssertEqual(report.fetched, grouped.fetched)
    }

    func testCodexUsageJSONReachesTheReport() throws {
        let json = """
        {"plan_type": "plus", "rate_limit": {
            "primary_window": {"used_percent": 30, "limit_window_seconds": 18000, "reset_at": 1800000000},
            "secondary_window": {"used_percent": 60, "limit_window_seconds": 604800, "reset_at": 1800500000}}}
        """
        let read = CodexLogin.usage(from: Data(json.utf8))
        let report = try XCTUnwrap(read.usage.map(UsageReport.init))
        XCTAssertEqual(report.groups.first?.name, "Codex")
        XCTAssertEqual(report.bars.map(\.kind), [.session, .week])
        XCTAssertEqual(report.bars.map(\.percent), [30, 60])
        XCTAssertEqual(report.bars[0].resetsAt, Date(timeIntervalSince1970: 1_800_000_000))
    }

    func testBarWindowRoundTrips() {
        let bar = UsageReport.Bar(label: "Week", kind: .week, percent: 9, resetsAt: reset)
        XCTAssertEqual(bar.window, UsageWindow(percent: 9, resetsAt: reset))
    }
}

final class ProviderRegistryTests: XCTestCase {
    private final class FakeProvider: AccountProvider {
        let id: ProviderID
        let title: String
        var syncs: [Bool] = []
        init(_ id: ProviderID, title: String = "Fake") { self.id = id; self.title = title }

        let capabilities: ProviderCapabilities = [.addAccount]
        var state = ProviderState()
        let menuStyle = ProviderMenuStyle(notSignedIn: "Not signed in", showsLimits: false)
        var changed: (() -> Void)?
        let switchFailureTitle = "Couldn't switch"
        var closed = 0
        var switched: [String] = []

        func sync(force: Bool) { syncs.append(force) }
        func switchTo(_ id: String) async throws { switched.append(id) }
        func beginAdd(done: @escaping (Error?) -> Void) { done(nil) }
        func cancelAdd() {}
        func remove(_ id: String) {}
        func makePage(cost: CostTracker) -> AnyView { AnyView(EmptyView()) }
        func accountsWindowClosed() { closed += 1 }
    }

    func testRegistryKeepsRegistrationOrderAndFindsById() {
        let registry = ProviderRegistry()
        let a = FakeProvider("a"), b = FakeProvider("b")
        registry.register(a)
        registry.register(b)
        XCTAssertEqual(registry.providers.map(\.id), ["a", "b"])
        XCTAssertTrue(registry["b"] === b)
        XCTAssertNil(registry["c"])
    }

    func testSecondProviderWithTheSameIdIsIgnored() {
        let registry = ProviderRegistry()
        let first = FakeProvider("a", title: "First")
        registry.register(first)
        registry.register(FakeProvider("a", title: "Second"))
        XCTAssertEqual(registry.providers.count, 1)
        XCTAssertTrue(registry["a"] === first)
    }

    func testSyncAllPassesForceToEveryProvider() {
        let registry = ProviderRegistry()
        let a = FakeProvider("a"), b = FakeProvider("b")
        registry.register(a)
        registry.register(b)
        registry.syncAll()
        registry.syncAll(force: true)
        XCTAssertEqual(a.syncs, [false, true])
        XCTAssertEqual(b.syncs, [false, true])
    }

    func testWindowClosedReachesEveryProvider() {
        let registry = ProviderRegistry()
        let a = FakeProvider("a"), b = FakeProvider("b")
        registry.register(a)
        registry.register(b)
        registry.accountsWindowClosed()
        XCTAssertEqual([a.closed, b.closed], [1, 1])
    }

    func testDefaultsAreNoConfirmationAndNoUsage() {
        let provider = FakeProvider("a")
        XCTAssertTrue(provider.isAvailable)
        XCTAssertNil(provider.switchConfirmation(to: "x"))
        XCTAssertNil(provider.addConfirmation())
        XCTAssertNil(provider.usageReport(for: "x"))
    }

    func testAwaitStoreTurnsTheCallbackIntoAResultOrAThrow() async throws {
        try await awaitStore { $0(nil) }
        struct Boom: Error {}
        do {
            try await awaitStore { $0(Boom()) }
            XCTFail("should have thrown")
        } catch {
            XCTAssertTrue(error is Boom)
        }
    }

    func testStateNamesTheActiveAccount() {
        var state = ProviderState(accounts: [ProviderAccount(id: "1", email: "a@x.com"), ProviderAccount(id: "2", email: "b@x.com")],
                                  activeID: "2")
        XCTAssertEqual(state.activeName, "b@x.com")
        state.activeID = "9"
        XCTAssertNil(state.activeName)
        state.unlistedActiveTitle = "Signed in"
        XCTAssertEqual(state.activeName, "Signed in")
        state.activeID = nil
        XCTAssertNil(state.activeName)
    }

    func testProviderAdaptersDescribeThemselves() {
        let accounts = AccountStore()
        let providers: [any AccountProvider] = [
            ClaudeCodeProvider(store: accounts),
            ClaudeDesktopProvider(store: ClaudeDesktopStore(), claude: accounts),
            CodexProvider(store: CodexStore()),
            AntigravityProvider(store: AntigravityStore()),
        ]
        XCTAssertEqual(providers.map(\.id), [.claudeCode, .claudeDesktop, .codex, .antigravity])
        XCTAssertEqual(providers.map(\.title), ["Claude Code", "Claude Desktop", "Codex", "Antigravity"])
        XCTAssertTrue(providers[1].capabilities.contains(.addFromMenu))
        XCTAssertFalse(providers[2].capabilities.contains(.addFromMenu))
    }
}
