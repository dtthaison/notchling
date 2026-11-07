import XCTest
@testable import ConsoleCore

final class TakeoverTests: XCTestCase {
    private let id = "7f1c2d3e-4a5b-4c6d-8e9f-0a1b2c3d4e5f"

    private struct Fixture {
        let paths: ClaudePaths
        let cwd: URL
        let root: URL
    }

    private func fixture(transcript: Bool = true, makeCwd: Bool = true) throws -> Fixture {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("takeover-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let claude = root.appendingPathComponent(".claude")
        let cwd = root.appendingPathComponent("work")
        try FileManager.default.createDirectory(at: claude.appendingPathComponent("sessions"), withIntermediateDirectories: true)
        if makeCwd { try FileManager.default.createDirectory(at: cwd, withIntermediateDirectories: true) }
        if transcript {
            let folder = claude.appendingPathComponent("projects/-tmp-work")
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try Data("{}\n".utf8).write(to: folder.appendingPathComponent("\(id).jsonl"))
        }
        return Fixture(paths: ClaudePaths(claudeDir: claude, requestsDir: root.appendingPathComponent("requests")), cwd: cwd, root: root)
    }

    private func session(_ f: Fixture, id: String? = nil, pid: Int32? = 4242, state: SessionState = .exited,
                         control: SessionControl = .viewOnly, entrypoint: Entrypoint = .cli, cwd: String? = nil) -> Session {
        Session(id: id ?? self.id, pid: pid, name: "old", project: "work", cwd: cwd ?? f.cwd.path, entrypoint: entrypoint, kind: "interactive",
                state: state, statusUpdatedAt: fixtureNow, startedAt: fixtureNow, transcriptURL: nil, control: control)
    }

    private func failure(_ result: Result<TakeoverPlan, TakeoverError>) -> TakeoverError? {
        if case .failure(let error) = result { return error } else { return nil }
    }

    // MARK: Eligibility

    func testAnExitedSessionWithATranscriptAndFolderCanBeTakenOver() throws {
        let f = try fixture()
        guard case .success(let plan) = Takeover.evaluate(session(f), paths: f.paths, isAlive: { _ in false }) else { return XCTFail() }
        XCTAssertEqual(plan.arguments, ["--resume", id])
        XCTAssertEqual(plan.cwd.path, f.cwd.path)
        XCTAssertTrue(plan.transcript.path.hasSuffix("\(id).jsonl"))
        XCTAssertEqual(Takeover.state(for: session(f), paths: f.paths, isAlive: { _ in false }), .available)
    }

    func testARunningOriginalIsBusyNoMatterWhat() throws {
        let f = try fixture()
        XCTAssertEqual(failure(Takeover.evaluate(session(f, pid: 4242), paths: f.paths, isAlive: { $0 == 4242 })), .alive)
        XCTAssertEqual(failure(Takeover.evaluate(session(f, pid: nil, state: .running), paths: f.paths, isAlive: { _ in false })), .alive)
        XCTAssertEqual(failure(Takeover.evaluate(session(f, state: .needsInput), paths: f.paths, isAlive: { _ in false })), .alive)
        XCTAssertEqual(Takeover.state(for: session(f), paths: f.paths, isAlive: { _ in true }), .unavailable(Takeover.aliveReason))
    }

    func testAnotherLiveProcessWritingTheSameSessionIdIsBusy() throws {
        let f = try fixture()
        let file = f.paths.sessionsDir.appendingPathComponent("999.json")
        try Data(#"{"pid":999,"sessionId":"\#(id)","cwd":"/x"}"#.utf8).write(to: file)
        XCTAssertEqual(failure(Takeover.evaluate(session(f), paths: f.paths, isAlive: { $0 == 999 })), .alive)
        XCTAssertNil(failure(Takeover.evaluate(session(f), paths: f.paths, isAlive: { _ in false })), "a stale file of a dead process does not block")
    }

    func testNoResumableTranscriptMeansNoTakeover() throws {
        let f = try fixture(transcript: false)
        XCTAssertEqual(failure(Takeover.evaluate(session(f), paths: f.paths, isAlive: { _ in false })), .noTranscript)
        // Desktop sessions keep their transcript elsewhere; the CLI cannot resume those.
        let elsewhere = f.paths.claudeDir.deletingLastPathComponent().appendingPathComponent("local-agent-mode-sessions/x")
        try FileManager.default.createDirectory(at: elsewhere, withIntermediateDirectories: true)
        try Data("{}".utf8).write(to: elsewhere.appendingPathComponent("\(id).jsonl"))
        XCTAssertEqual(failure(Takeover.evaluate(session(f), paths: f.paths, isAlive: { _ in false })), .noTranscript)
    }

    func testBadIdsNeverReachThePath() throws {
        let f = try fixture()
        for bad in ["../../etc/passwd", "short", "", "7f1c2d3e-4a5b-4c6d-8e9f-0a1b2c3d4e5f/../x"] {
            XCTAssertEqual(failure(Takeover.evaluate(session(f, id: bad), paths: f.paths, isAlive: { _ in false })), .noTranscript, bad)
        }
    }

    func testSdkMissingFolderAndAlreadyControllableAreRefused() throws {
        let f = try fixture()
        XCTAssertEqual(failure(Takeover.evaluate(session(f, entrypoint: .sdk), paths: f.paths, isAlive: { _ in false })), .sdk)
        XCTAssertEqual(failure(Takeover.evaluate(session(f, cwd: "/definitely/not/here"), paths: f.paths, isAlive: { _ in false })), .noWorkingFolder)
        XCTAssertEqual(failure(Takeover.evaluate(session(f, cwd: ""), paths: f.paths, isAlive: { _ in false })), .noWorkingFolder)
        XCTAssertEqual(failure(Takeover.evaluate(session(f, control: .consolePTY), paths: f.paths, isAlive: { _ in false })), .alreadyControllable)
        XCTAssertNil(Takeover.state(for: session(f, control: .consolePTY), paths: f.paths), "only view-only sessions carry a takeover state")
    }

    // MARK: Spawn

    func testSpawnRequestMatchesANewSessionWithResumeAndAScrubbedEnvironment() throws {
        let f = try fixture()
        guard case .success(let plan) = Takeover.evaluate(session(f), paths: f.paths, isAlive: { _ in false }) else { return XCTFail() }
        let request = Takeover.spawn(for: plan, executable: URL(fileURLWithPath: "/x/claude"),
                                     environment: ["PATH": "/bin", "CLAUDE_CODE_ENTRYPOINT": "cli", "CLAUDECODE": "1", "HOME": "/h"])
        XCTAssertEqual(request.executable.path, "/x/claude")
        XCTAssertEqual(request.arguments, ["--resume", id])
        XCTAssertEqual(request.cwd.path, f.cwd.path)
        XCTAssertNil(request.environment["CLAUDE_CODE_ENTRYPOINT"])
        XCTAssertNil(request.environment["CLAUDECODE"])
        XCTAssertEqual(request.environment["PATH"], "/bin")
        XCTAssertEqual(request.environment["TERM"], "xterm-256color")
        XCTAssertEqual(request.environment, SpawnEnvironment.scrubbed(["PATH": "/bin", "CLAUDE_CODE_ENTRYPOINT": "cli", "CLAUDECODE": "1", "HOME": "/h"]))
    }

    // MARK: pid to session

    func testTheResumedProcessMapsBackToItsSessionWhetherTheIdStaysOrChanges() throws {
        let f = try fixture()
        let parents: [Int32: Int32] = [501: 500, 500: 300]
        func write(_ sessionId: String) throws {
            try Data(#"{"pid":501,"sessionId":"\#(sessionId)"}"#.utf8).write(to: f.paths.sessionsDir.appendingPathComponent("501.json"))
        }
        try write(id)
        let same = SessionPidMapper.sessionId(forRootPid: 300, sessionsDir: f.paths.sessionsDir, parentOf: { parents[$0] })
        XCTAssertEqual(same, id)
        XCTAssertEqual(Takeover.resolution(original: id, resumed: same!), .sameSession)

        let changedId = "11111111-2222-4333-8444-555555555555"
        try write(changedId)
        let changed = SessionPidMapper.sessionId(forRootPid: 300, sessionsDir: f.paths.sessionsDir, parentOf: { parents[$0] })
        XCTAssertEqual(Takeover.resolution(original: id, resumed: changed!), .replaced(old: id, new: changedId))
        XCTAssertNil(SessionPidMapper.sessionId(forRootPid: 777, sessionsDir: f.paths.sessionsDir, parentOf: { parents[$0] }))
    }

    // MARK: Router op

    func testTakeoverOpValidatesAndAudits() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, _) = try await rig.login(name: "away-phone", from: "203.0.113.5")
        rig.backend.sessionList = [makeSession("view", control: .viewOnly), makeSession("mine", control: .consolePTY)]

        await rig.router.handle(#"{"id":2,"op":"session.takeover"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await rig.router.handle(#"{"id":3,"op":"session.takeover","sessionId":"nope"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
        await rig.router.handle(#"{"id":4,"op":"session.takeover","sessionId":"mine"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid", "already controllable")
        XCTAssertTrue(rig.backend.takeoverCalls.isEmpty, "nothing reached the backend")

        rig.backend.takeoverResult = .failure(BridgeFailure(.busy, Takeover.aliveReason))
        await rig.router.handle(#"{"id":5,"op":"session.takeover","sessionId":"view"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "busy")
        XCTAssertTrue(rig.core.audit.recent(limit: 5).allSatisfy { $0.op != "takeover" }, "a refused takeover is not audited as done")

        rig.backend.takeoverResult = .success(TakeoverOutcome(sessionId: "resumed-id"))
        await rig.router.handle(#"{"id":6,"op":"session.takeover","sessionId":"view"}"#, from: connection)
        let answer = try io.lastJSON()
        XCTAssertTrue(answer.isOK)
        XCTAssertEqual(answer.dataObject["sessionId"] as? String, "resumed-id")
        let entry = try XCTUnwrap(rig.core.audit.recent(limit: 1).first)
        XCTAssertEqual(entry.op, "takeover")
        XCTAssertEqual(entry.session, "view")
        XCTAssertEqual(entry.device, "away-phone")
    }

    func testTheTakeoverStateTravelsInTheSessionJSON() throws {
        var view = makeSession("v", control: .viewOnly)
        view.takeover = .unavailable(Takeover.aliveReason)
        let dto = SessionDTO(view)
        XCTAssertEqual(dto.takeover, TakeoverDTO(available: false, reason: Takeover.aliveReason, stoppable: false, busy: false))
        view.takeover = .available
        XCTAssertEqual(SessionDTO(view).takeover, TakeoverDTO(available: true, reason: nil, stoppable: false, busy: false))
        XCTAssertNil(SessionDTO(makeSession("c", control: .consolePTY)).takeover)
    }
}
