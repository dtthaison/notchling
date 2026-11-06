import SwiftUI
import XCTest
@testable import Notchling

/// The limit rules for Codex and Antigravity. Nothing here touches a real login: the watcher runs
/// against a fake provider that only records the switch it is asked for.
final class ProviderLimitsTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private var reset: Date { now.addingTimeInterval(3600) }

    private func report(session: Double, week: Double, fetched: Date? = nil, extra: [UsageReport.Group] = []) -> UsageReport {
        UsageReport(groups: [UsageReport.Group(name: "Codex", bars: [
            .init(label: "5h", kind: .session, percent: session, resetsAt: reset),
            .init(label: "Week", kind: .week, percent: week, resetsAt: reset.addingTimeInterval(86400)),
        ])] + extra, fetched: fetched ?? now)
    }

    // MARK: Windows and candidates

    func testCodexWatchesOnlyItsMainGroup() {
        let review = UsageReport.Group(name: "Code Review", bars: [.init(label: "Week", kind: .week, percent: 99, resetsAt: nil)])
        let windows = LimitLogic.watchedWindows(report(session: 10, week: 20, extra: [review]), primaryGroup: "Codex")
        XCTAssertEqual(windows.map(\.name), ["5-hour", "weekly"])
        XCTAssertEqual(windows.map(\.window.percent), [10, 20])
    }

    func testAntigravityWatchesEveryGroupAndNamesIt() {
        let r = UsageReport(groups: [
            .init(name: "Gemini Pro", bars: [.init(label: "5h", kind: .session, percent: 50, resetsAt: nil)]),
            .init(name: "Flash", bars: [.init(label: "Week", kind: .week, percent: 70, resetsAt: nil)]),
        ])
        XCTAssertEqual(LimitLogic.watchedWindows(r, primaryGroup: nil).map(\.name), ["Gemini Pro 5-hour", "Flash weekly"])
    }

    func testCandidateFromReportTakesTheHighestShortAndWeeklyBars() {
        let c = LimitLogic.Candidate(id: "a", report: report(session: 30, week: 60), primaryGroup: "Codex")
        XCTAssertEqual(c.session, 30)
        XCTAssertEqual(c.week, 60)
        XCTAssertEqual(c.fetched, now)
        let none = LimitLogic.Candidate(id: "b", report: nil, primaryGroup: "Codex")
        XCTAssertNil(none.session)
        XCTAssertNil(LimitLogic.pickTarget([none], activeID: nil, now: now, blockedUntil: [:]))
    }

    func testReportCandidateFollowsTheSameRoomAndFreshnessRulesAsClaude() {
        let all = [
            LimitLogic.Candidate(id: "full", report: report(session: 80, week: 1), primaryGroup: "Codex"),
            LimitLogic.Candidate(id: "stale", report: report(session: 1, week: 1, fetched: now.addingTimeInterval(-LimitLogic.freshFor - 1)),
                                 primaryGroup: "Codex"),
            LimitLogic.Candidate(id: "roomy", report: report(session: 20, week: 30), primaryGroup: "Codex"),
            LimitLogic.Candidate(id: "roomier", report: report(session: 5, week: 10), primaryGroup: "Codex"),
        ]
        XCTAssertEqual(LimitLogic.pickTarget(all, activeID: nil, now: now, blockedUntil: [:])?.id, "roomier")
    }

    // MARK: Announcements

    func testAnnouncementFiresOncePerWindowAndKeepsTheHighestLevel() {
        var marks: [String: LimitLogic.Mark] = [:]
        let w = [("5-hour", Optional(UsageWindow(percent: 92, resetsAt: reset))), ("weekly", Optional(UsageWindow(percent: 100, resetsAt: reset)))]
        let first = LimitLogic.announcement(marks: &marks, keyPrefix: "a", windows: w, skip90: false)
        XCTAssertEqual(first?.level, 100)
        XCTAssertEqual(first?.name, "weekly")
        XCTAssertNil(LimitLogic.announcement(marks: &marks, keyPrefix: "a", windows: w, skip90: false))
        // Another account has its own marks.
        XCTAssertNotNil(LimitLogic.announcement(marks: &marks, keyPrefix: "b", windows: w, skip90: false))
    }

    // MARK: Switch decision

    private func decide(active: Double = 96, candidates: [LimitLogic.Candidate]? = nil, blocked: [String: Date] = [:],
                        last: Date? = nil, idle: Bool = true, idleCalls: UnsafeMutablePointer<Int>? = nil) -> LimitLogic.SwitchDecision {
        let windows: [LimitLogic.NamedWindow] = [("5-hour", UsageWindow(percent: active, resetsAt: reset))]
        let pool = candidates ?? [LimitLogic.Candidate(id: "other", report: report(session: 10, week: 10), primaryGroup: "Codex")]
        return LimitLogic.decideSwitch(windows: windows, switchAt: 95, activeID: "me", candidates: pool, blockedUntil: blocked,
                                       lastSwitch: last, now: now) { idleCalls?.pointee += 1; return idle }
    }

    func testSwitchesToTheAccountWithRoom() {
        XCTAssertEqual(decide(), .switchTo("other"))
    }

    func testStaysBelowTheSwitchPoint() {
        XCTAssertEqual(decide(active: 94), .stay)
    }

    func testNoRoomWhenEveryOtherAccountIsFullStaleOrBlocked() {
        let full = LimitLogic.Candidate(id: "full", report: report(session: 85, week: 10), primaryGroup: "Codex")
        XCTAssertEqual(decide(candidates: [full]), .noRoom)
        XCTAssertEqual(decide(blocked: ["other": now.addingTimeInterval(60)]), .noRoom)
        XCTAssertEqual(decide(blocked: ["other": now]), .switchTo("other"))
    }

    func testAtMostOneSwitchPerHalfHour() {
        XCTAssertEqual(decide(last: now.addingTimeInterval(-29 * 60)), .stay)
        XCTAssertEqual(decide(last: now.addingTimeInterval(-30 * 60)), .switchTo("other"))
    }

    func testBusyProviderBlocksTheSwitchAndIsAskedOnlyWhenATargetExists() {
        var calls = 0
        XCTAssertEqual(decide(idle: false, idleCalls: &calls), .busy)
        XCTAssertEqual(calls, 1)
        calls = 0
        XCTAssertEqual(decide(active: 50, idle: false, idleCalls: &calls), .stay)
        let full = LimitLogic.Candidate(id: "full", report: report(session: 85, week: 10), primaryGroup: "Codex")
        XCTAssertEqual(decide(candidates: [full], idle: false, idleCalls: &calls), .noRoom)
        XCTAssertEqual(calls, 0)
    }

    func testCodexAutoSwitchWaitsForTurnsAndTheApp() {
        XCTAssertTrue(CodexStore.idleForAutoSwitch((app: false, server: false, sessions: 0)))
        XCTAssertTrue(CodexStore.idleForAutoSwitch((app: false, server: true, sessions: 0)))
        XCTAssertFalse(CodexStore.idleForAutoSwitch((app: false, server: false, sessions: 1)))
        XCTAssertFalse(CodexStore.idleForAutoSwitch((app: true, server: false, sessions: 0)))
    }

    // MARK: Chat header

    func testChatHeaderShowsTheMostUsedAntigravityGroup() {
        let r = UsageReport(groups: [
            .init(name: "Flash", bars: [.init(label: "5h", kind: .session, percent: 10, resetsAt: nil),
                                        .init(label: "Week", kind: .week, percent: 20, resetsAt: nil)]),
            .init(name: "Gemini Pro", bars: [.init(label: "5h", kind: .session, percent: 40, resetsAt: nil),
                                             .init(label: "Week", kind: .week, percent: 75, resetsAt: nil)]),
        ])
        let limits = SessionLimit.mostUsedGroup(r)
        XCTAssertEqual(limits.map(\.label), ["5h", "Week"])
        XCTAssertEqual(limits.map(\.window.percent), [40, 75])
        XCTAssertEqual(limits.first?.name, "Gemini Pro 5-hour limit")
        XCTAssertTrue(SessionLimit.mostUsedGroup(UsageReport(groups: [])).isEmpty)
    }

    // MARK: Watcher

    private final class FakeProvider: AccountProvider {
        let id: ProviderID = "fake"
        let title = "Fake"
        let capabilities: ProviderCapabilities = [.usage]
        var state = ProviderState(accounts: [ProviderAccount(id: "me", email: "me@x.com"), ProviderAccount(id: "other", email: "other@x.com")],
                                  activeID: "me")
        let menuStyle = ProviderMenuStyle(notSignedIn: "", showsLimits: true)
        var changed: (() -> Void)?
        let switchFailureTitle = ""
        var reports: [String: UsageReport] = [:]
        var switched: [String] = []
        var onSwitch: (() -> Void)?

        func sync(force: Bool) {}
        func usageReport(for accountID: String) -> UsageReport? { reports[accountID] }
        func switchTo(_ id: String) async throws { switched.append(id); onSwitch?() }
        func beginAdd(done: @escaping (Error?) -> Void) { done(nil) }
        func cancelAdd() {}
        func remove(_ id: String) {}
        func makePage(cost: CostTracker) -> AnyView { AnyView(EmptyView()) }
    }

    private func watcher(_ provider: FakeProvider, mode: LimitMode, modes: [LimitMode] = LimitMode.allCases,
                         idle: Bool = true, said: @escaping (String) -> Void) -> ProviderLimitWatcher {
        let key = "test.limitMode.\(UUID().uuidString)"
        addTeardownBlock { UserDefaults.standard.removeObject(forKey: key) }
        let w = ProviderLimitWatcher(
            provider: provider,
            setup: .init(label: "Fake", modeKey: key, modes: modes, defaultMode: .notify, primaryGroup: "Codex", busyText: "Busy."),
            switchAt: { 95 }, canSwitchNow: { idle }, announce: said)
        w.mode = mode
        return w
    }

    func testNotifyAnnouncesEachLevelOnceAndNeverSwitches() {
        let provider = FakeProvider()
        provider.reports = ["me": report(session: 96, week: 10), "other": report(session: 1, week: 1)]
        var said: [String] = []
        let w = watcher(provider, mode: .notify) { said.append($0) }
        w.check(now: now)
        w.check(now: now.addingTimeInterval(3))
        XCTAssertEqual(said.count, 1)
        XCTAssertTrue(said[0].contains("at 96% of its 5-hour limit"), said[0])
        provider.reports["me"] = report(session: 100, week: 10)
        w.check(now: now.addingTimeInterval(6))
        XCTAssertEqual(said.count, 2)
        XCTAssertTrue(said[1].contains("has used its 5-hour limit"), said[1])
        XCTAssertTrue(provider.switched.isEmpty)
    }

    func testOffAndStaleReadingsSayNothing() {
        let provider = FakeProvider()
        provider.reports = ["me": report(session: 99, week: 99)]
        var said: [String] = []
        watcher(provider, mode: .off) { said.append($0) }.check(now: now)
        let stale = report(session: 99, week: 99, fetched: now.addingTimeInterval(-LimitLogic.inUseFreshFor - 1))
        provider.reports = ["me": stale]
        watcher(provider, mode: .notify) { said.append($0) }.check(now: now)
        XCTAssertTrue(said.isEmpty)
    }

    func testModeFallsBackToWhatTheProviderAllows() {
        let w = watcher(FakeProvider(), mode: .notify, modes: [.off, .notify]) { _ in }
        w.mode = .notifyAndSwitch
        XCTAssertEqual(w.mode, .notify)
        w.mode = .off
        XCTAssertEqual(w.mode, .off)
    }

    func testNotifyAndSwitchAsksTheProviderToSwitchWhenIdle() {
        let provider = FakeProvider()
        provider.reports = ["me": report(session: 96, week: 10), "other": report(session: 10, week: 10)]
        let switched = expectation(description: "switch")
        provider.onSwitch = { switched.fulfill() }
        var said: [String] = []
        let w = watcher(provider, mode: .notifyAndSwitch) { said.append($0) }
        w.check(now: now)
        wait(for: [switched], timeout: 2)
        XCTAssertEqual(provider.switched, ["other"])
        XCTAssertTrue(said.isEmpty, "the switch announcement comes after it, not the climb: \(said)")
    }

    func testNotifyAndSwitchOnlyNotifiesWhileBusyAndSaysSo() {
        let provider = FakeProvider()
        provider.reports = ["me": report(session: 96, week: 10), "other": report(session: 10, week: 10)]
        var said: [String] = []
        let w = watcher(provider, mode: .notifyAndSwitch, idle: false) { said.append($0) }
        w.check(now: now)
        w.check(now: now.addingTimeInterval(3))
        XCTAssertTrue(provider.switched.isEmpty)
        XCTAssertEqual(said.count, 1)
        XCTAssertTrue(said[0].contains("Busy."), said[0])
    }

    func testNotifyAndSwitchSaysWhenNoOtherAccountHasRoom() {
        let provider = FakeProvider()
        provider.reports = ["me": report(session: 96, week: 10), "other": report(session: 90, week: 10)]
        var said: [String] = []
        let w = watcher(provider, mode: .notifyAndSwitch) { said.append($0) }
        w.check(now: now)
        XCTAssertTrue(provider.switched.isEmpty)
        XCTAssertEqual(said.count, 1)
        XCTAssertTrue(said[0].contains("No other Fake account has room"), said[0])
    }
}
