import XCTest
@testable import Notchling

/// Who hears that a run ended: the background job that started it, or the user.
final class RunWakerTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    /// `since` is when the agent last changed state (before the runs' finishes, which are at t0 + 100, unless a test says otherwise).
    func agent(_ id: String, _ group: AgentGroup, job: String? = nil, command: String? = nil, since: Date? = nil) -> Agent {
        Agent(id: id, name: id, project: "p", cwd: "/repo", group: group, label: "", detail: nil, since: since ?? t0,
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
        let actions = w.plan(ends: [end(rc: 124)], agents: [agent("s1", .completed)], enabled: true, now: t0.addingTimeInterval(200), lastLine: { _ in nil })
        guard case .send(_, let text, _) = actions.first else { return XCTFail() }
        XCTAssertEqual(text, "Run finished: just perf — exit 124 after 32m. Log: /l/a.log.")
    }

    func testAnAgentThatStoppedWorkingAfterTheFinishIsNotWoken() {
        // A session that waits in Monitor, a background command or `sd.py wait` wakes itself when the run ends.
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .working, since: t0.addingTimeInterval(90))], now: t0.addingTimeInterval(110)).isEmpty)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .working, since: t0.addingTimeInterval(90))], now: t0.addingTimeInterval(200)).isEmpty)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed, since: t0.addingTimeInterval(230))], now: t0.addingTimeInterval(240)).isEmpty,
                      "it worked after the finish, so nobody types into it")
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)], now: t0.addingTimeInterval(300)).isEmpty, "and it stays handled")
    }

    func testAnAgentThatWorkedAfterTheFinishAndIsIdleAgainIsNotWoken() {
        var w = waker()
        let idleAgain = agent("s1", .completed, since: t0.addingTimeInterval(130))
        XCTAssertTrue(plan(&w, [end()], [idleAgain], now: t0.addingTimeInterval(200)).isEmpty)
    }

    func testNothingIsSentBeforeTheGraceHasPassed() {
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)], now: t0.addingTimeInterval(100 + 30)).isEmpty)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)], now: t0.addingTimeInterval(100 + RunWaker.grace - 1)).isEmpty)
        XCTAssertEqual(plan(&w, [end()], [agent("s1", .completed)], now: t0.addingTimeInterval(100 + RunWaker.grace)).count, 1)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)], now: t0.addingTimeInterval(400)).isEmpty, "only once")
    }

    func testAnIdleAgentIsWokenOnlyIfItHasBeenIdleSinceBeforeTheFinish() {
        var w = waker()
        let actions = plan(&w, [end()], [agent("s1", .completed, since: t0.addingTimeInterval(100))])
        XCTAssertEqual(actions.count, 1, "idle since exactly the finish counts as before it")
    }

    func testAGraceInProgressSurvivesLeavingTheFinishedWindow() {
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed)], now: t0.addingTimeInterval(110)).isEmpty)
        XCTAssertEqual(plan(&w, [], [agent("s1", .completed)], now: t0.addingTimeInterval(200)).count, 1)
    }

    func testWithoutAStateTimeTheWatcherRemembersWhenItFirstSawTheAgentIdle() {
        let unknown = Date.distantPast
        var w = waker()
        // seen idle at t0 + 50, before the finish: it has been idle since before it
        XCTAssertTrue(plan(&w, [], [agent("s1", .completed, since: unknown)], now: t0.addingTimeInterval(50)).isEmpty)
        XCTAssertEqual(plan(&w, [end()], [agent("s1", .completed, since: unknown)], now: t0.addingTimeInterval(200)).count, 1)

        // first seen idle only after the finish: it may have worked in between
        var late = waker()
        XCTAssertTrue(plan(&late, [end()], [agent("s1", .completed, since: unknown)], now: t0.addingTimeInterval(200)).isEmpty)
        XCTAssertTrue(plan(&late, [end()], [agent("s1", .completed, since: unknown)], now: t0.addingTimeInterval(400)).isEmpty)
    }

    func testGoingBusyForgetsWhenTheAgentWentIdle() {
        let unknown = Date.distantPast
        var w = waker()
        _ = plan(&w, [], [agent("s1", .completed, since: unknown)], now: t0.addingTimeInterval(50))
        _ = plan(&w, [], [agent("s1", .working, since: unknown)], now: t0.addingTimeInterval(120))
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed, since: unknown)], now: t0.addingTimeInterval(200)).isEmpty)
    }

    func testSeveralEndsDueForOneJobAreOneMessageWithALinePerRun() {
        var w = waker()
        let actions = plan(&w, [end(log: "/l/a.log", at: t0.addingTimeInterval(100)), end(log: "/l/b.log", rc: 2, at: t0.addingTimeInterval(110))],
                           [agent("s1", .completed, job: "abc123")])
        XCTAssertEqual(actions.count, 1)
        guard case .send(let job, let text, let ids) = actions[0] else { return XCTFail("\(actions)") }
        XCTAssertEqual(job, "abc123")
        XCTAssertEqual(ids.count, 2)
        let lines = text.split(separator: "\n").map(String.init)
        XCTAssertEqual(lines.count, 2)
        XCTAssertTrue(lines[0].contains("/l/a.log") && lines[0].contains("exit 0"))
        XCTAssertTrue(lines[1].contains("/l/b.log") && lines[1].contains("exit 2"))
    }

    func testEndsOfTwoJobsAreTwoMessages() {
        var w = waker()
        let e1 = end(session: "s1", log: "/l/a.log"), e2 = end(session: "s2", log: "/l/b.log")
        let actions = plan(&w, [e1, e2], [agent("s1", .completed, job: "j1"), agent("s2", .completed, job: "j2")])
        XCTAssertEqual(actions.count, 2)
    }

    func testAnAgentWaitingOnAPromptIsNotTypedInto() {
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .needsInput)]).isEmpty)
    }

    func testAnAgentThatWasWaitingOnAPromptIsNotWokenOnceItHasMovedOn() {
        var w = waker()
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .needsInput)]).isEmpty)
        XCTAssertTrue(plan(&w, [end()], [agent("s1", .completed, since: t0.addingTimeInterval(250))], now: t0.addingTimeInterval(300)).isEmpty,
                      "the user answered it after the run ended")
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

    func testAFinishWithoutAStartNeitherWakesNorNotifies() {
        // sd writes `{"key":"precheck: ...","log":"<log>.precheck"}` before each main run, and older lines have no start either.
        var w = waker()
        var orphan = end(log: "/l/a.log.precheck")
        orphan.start = nil
        orphan.finish.key = "precheck: just check"
        XCTAssertTrue(plan(&w, [orphan], [agent("s1", .completed)]).isEmpty)
        XCTAssertTrue(plan(&w, [orphan], []).isEmpty)
        var old = end(log: "/l/old.log")
        old.start = nil
        XCTAssertTrue(plan(&w, [old], []).isEmpty)
    }

    func testNoSessionNotifies() {
        var w = waker()
        guard case .notify? = plan(&w, [end(session: nil)], [agent("s1", .completed)]).first else { return XCTFail() }
    }

    func testAQueuedRunNotifiesWhenItsAgentIsGone() {
        var w = waker()
        _ = plan(&w, [end()], [agent("s1", .needsInput)])
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
        _ = plan(&w, [end()], [agent("s1", .needsInput)])
        let actions = plan(&w, [], [agent("s1", .needsInput)], now: t0.addingTimeInterval(200 + RunWaker.queueLimit + 1))
        guard case .notify? = actions.first else { return XCTFail("\(actions)") }
    }

    func testGivingUpNotifiesOnlyForAnOwnerStillWaitingOnAPrompt() {
        var w = waker()
        _ = plan(&w, [end()], [agent("s1", .needsInput)])
        let late = t0.addingTimeInterval(200 + RunWaker.queueLimit + 1)
        XCTAssertTrue(plan(&w, [], [agent("s1", .working, since: late)], now: late).isEmpty, "it moved on, and the run is not its news any more")
        XCTAssertTrue(plan(&w, [], [agent("s1", .needsInput)], now: late.addingTimeInterval(60)).isEmpty, "handled")
    }

    func testMessageHasNoLineBreaks() {
        var w = waker()
        let e = end()
        var long = e
        long.finish.key = "x"
        long.start?.key = "line one\nline two"
        guard case .send(_, let text, _)? = w.plan(ends: [long], agents: [agent("s1", .completed)], enabled: true, now: t0.addingTimeInterval(200), lastLine: { _ in "a\nb" }).first else { return XCTFail() }
        XCTAssertFalse(text.contains("\n"))
    }
}
