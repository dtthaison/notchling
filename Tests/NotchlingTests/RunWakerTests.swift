import XCTest
@testable import Notchling

/// Who hears that a run ended: the background job that started it, or the user.
final class RunWakerTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func agent(_ id: String, _ group: AgentGroup, job: String? = nil, command: String? = nil) -> Agent {
        Agent(id: id, name: id, project: "p", cwd: "/repo", group: group, label: "", detail: nil, since: t0,
              target: .job(job ?? "short-\(id)"), command: command)
    }

    func end(session: String? = "s1", log: String = "/l/a.log", rc: Int = 0, at: Date? = nil) -> RunEnd {
        let finish = RunFinish(key: "just perf", seconds: 1920, rc: rc, log: log, at: at ?? t0.addingTimeInterval(100))
        let start = RunStart(key: "just perf", log: log, cwd: "/repo", pid: 1, session: session, limit: 7200, startedAt: t0)
        return RunEnd(start: start, finish: finish)
    }

    func waker() -> RunWaker { RunWaker(startedAt: t0) }
    let line: (String) -> String? = { _ in "PASS 12 tests" }

    func plan(_ w: inout RunWaker, _ ends: [RunEnd], _ agents: [Agent], enabled: Bool = true, now: Date? = nil) -> [RunWaker.Action] {
        w.plan(ends: ends, agents: agents, enabled: enabled, now: now ?? t0.addingTimeInterval(200), lastLine: line)
    }

    func testSendsToTheSessionsBackgroundJob() {
        var w = waker()
        let actions = plan(&w, [end()], [agent("s1", .completed, job: "abc123")])
        XCTAssertEqual(actions.count, 1)
        guard case .send(let job, let text, _) = actions[0] else { return XCTFail("\(actions)") }
        XCTAssertEqual(job, "abc123")
        XCTAssertEqual(text, "Run finished: just perf — exit 0 after 32m. Log: /l/a.log. Last line: PASS 12 tests.")
    }

    func testMessageWithoutALastLine() {
        var w = waker()
        let actions = w.plan(ends: [end(rc: 124)], agents: [agent("s1", .completed)], enabled: true, now: t0, lastLine: { _ in nil })
        guard case .send(_, let text, _) = actions.first else { return XCTFail() }
        XCTAssertEqual(text, "Run finished: just perf — exit 124 after 32m. Log: /l/a.log.")
    }

    func testAWorkingAgentWaitsThenGetsOneMessage() {
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .working)]).isEmpty)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .working)]).isEmpty)
        let actions = plan(&w, [end()], [agent("s1", .completed)])
        XCTAssertEqual(actions.count, 1)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)]).isEmpty, "only once")
    }

    func testAQueuedRunSurvivesLeavingTheFinishedWindow() {
        var w = waker()
        _ = plan(&w, [end()], [agent("s1", .working)])
        let actions = plan(&w, [], [agent("s1", .completed)])
        XCTAssertEqual(actions.count, 1)
    }

    func testAnAgentWaitingOnAPromptIsNotTypedInto() {
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .needsInput)]).isEmpty)
    }

    func testNoMatchingJobNotifies() {
        var w = waker()
        for agents in [[Agent](), [agent("other", .completed)]] {
            var w = waker()
            let actions = plan(&w, [end()], agents)
            guard case .notify(_, let body, _)? = actions.first else { return XCTFail("\(actions)") }
            XCTAssertTrue(body.contains("perf"))
        }
        _ = w
    }

    func testTerminalSessionsAndCommandJobsNotify() {
        var w = waker()
        let terminal = Agent(id: "s1", name: "t", project: "p", cwd: "/", group: .completed, label: "", detail: nil, since: t0, target: .process(123))
        guard case .notify? = plan(&w, [end()], [terminal]).first else { return XCTFail() }
        var w2 = waker()
        guard case .notify? = plan(&w2, [end()], [agent("s1", .completed, command: "sd lane")]).first else { return XCTFail() }
    }

    func testNoSessionNotifies() {
        var w = waker()
        guard case .notify? = plan(&w, [end(session: nil)], [agent("s1", .completed)]).first else { return XCTFail() }
    }

    func testAQueuedRunNotifiesWhenItsAgentIsGone() {
        var w = waker()
        _ = plan(&w, [end()], [agent("s1", .working)])
        guard case .notify? = plan(&w, [], []).first else { return XCTFail() }
        XCTAssertTrue(plan(&w, [], []).isEmpty)
    }

    func testFinishesFromBeforeNotchlingStartedAreSkipped() {
        var w = waker()
        let old = end(at: t0.addingTimeInterval(-60))
        XCTAssertTrue(plan(&w, [old], [agent("s1", .completed)]).isEmpty)
        XCTAssertTrue(plan(&w, [old], []).isEmpty)
    }

    func testEachRunIsHandledOnce() {
        var w = waker()
        XCTAssertEqual(plan(&w, [end()], []).count, 1)
        XCTAssertTrue(plan(&w, [end()], []).isEmpty)
    }

    func testTwoRunsOfOneSessionBothArrive() {
        var w = waker()
        let actions = plan(&w, [end(log: "/l/a.log"), end(log: "/l/b.log")], [agent("s1", .completed)])
        XCTAssertEqual(actions.count, 2)
    }

    func testAReusedLogWakesAgainForItsNextRun() {
        var w = waker()
        XCTAssertEqual(plan(&w, [end(at: t0.addingTimeInterval(100))], []).count, 1)
        XCTAssertEqual(plan(&w, [end(at: t0.addingTimeInterval(900))], []).count, 1)
    }

    func testSettingOffSendsNothingAndForgetsTheRun() {
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)], enabled: false).isEmpty)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)], enabled: true).isEmpty, "switching on later doesn't replay it")
    }

    func testAQueuedRunIsGivenUpAfterAWhile() {
        var w = waker()
        _ = plan(&w, [end()], [agent("s1", .working)])
        let actions = plan(&w, [], [agent("s1", .working)], now: t0.addingTimeInterval(200 + RunWaker.queueLimit + 1))
        guard case .notify? = actions.first else { return XCTFail("\(actions)") }
    }

    func testMessageHasNoLineBreaks() {
        var w = waker()
        let e = end()
        var long = e
        long.finish.key = "x"
        long.start?.key = "line one\nline two"
        guard case .send(_, let text, _)? = w.plan(ends: [long], agents: [agent("s1", .completed)], enabled: true, now: t0, lastLine: { _ in "a\nb" }).first else { return XCTFail() }
        XCTAssertFalse(text.contains("\n"))
    }
}
