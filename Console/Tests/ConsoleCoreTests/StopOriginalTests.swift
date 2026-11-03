import XCTest
@testable import ConsoleCore

/// A stand-in for the system: which pids exist, what they run, and what happens on SIGTERM.
private final class FakeProcesses: ProcessControl, @unchecked Sendable {
    private let lock = NSLock()
    var alive: Set<Int32> = []
    var paths: [Int32: String] = [:]
    /// pids that exit as soon as they get SIGTERM; others ignore it.
    var exitsOnTerm: Set<Int32> = []
    /// pids that exit after this many liveness checks following SIGTERM.
    var exitsAfterChecks: [Int32: Int] = [:]
    private(set) var terminated: [Int32] = []
    private var termChecks: [Int32: Int] = [:]

    func isAlive(_ pid: Int32) -> Bool {
        lock.withLock {
            if let after = exitsAfterChecks[pid], terminated.contains(pid) {
                termChecks[pid, default: 0] += 1
                if termChecks[pid]! > after { alive.remove(pid) }
            }
            return alive.contains(pid)
        }
    }
    func executablePath(of pid: Int32) -> String? { lock.withLock { paths[pid] } }
    func terminate(_ pid: Int32) -> Bool {
        lock.withLock {
            terminated.append(pid)
            if exitsOnTerm.contains(pid) { alive.remove(pid) }
            return true
        }
    }
}

final class StopOriginalTests: XCTestCase {
    private let id = "7f1c2d3e-4a5b-4c6d-8e9f-0a1b2c3d4e5f"
    private let claudePath = "/Users/x/.local/share/claude/versions/2.1.316"

    private func sessionsDir(files: [Int32: String]) throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("stop-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        for (pid, sessionId) in files {
            try Data(#"{"pid":\#(pid),"sessionId":"\#(sessionId)"}"#.utf8).write(to: root.appendingPathComponent("\(pid).json"))
        }
        return root
    }

    private func instant(_ seconds: TimeInterval) async {}

    func testWhatCountsAsClaude() {
        XCTAssertTrue(OriginalStopper.isClaudeExecutable("/Users/x/.local/share/claude/versions/2.1.316"))
        XCTAssertTrue(OriginalStopper.isClaudeExecutable("/Users/x/Library/Application Support/Claude/claude-code/2.1.309/h/claude.app/Contents/MacOS/claude"))
        XCTAssertTrue(OriginalStopper.isClaudeExecutable("/usr/local/bin/claude"))
        XCTAssertFalse(OriginalStopper.isClaudeExecutable("/bin/zsh"))
        XCTAssertFalse(OriginalStopper.isClaudeExecutable("/Applications/Safari.app/Contents/MacOS/Safari"))
        XCTAssertFalse(OriginalStopper.isClaudeExecutable("/usr/bin/claudette"))
    }

    func testAVerifiedClaudeIsTermedAndWaitedFor() async throws {
        let dir = try sessionsDir(files: [900: id])
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = claudePath; control.exitsOnTerm = [900]
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, sleep: { _ in })
        XCTAssertEqual(result, .success([900]))
        XCTAssertEqual(control.terminated, [900], "exactly that pid, once")
    }

    func testAProcessThatNeedsAMomentIsWaitedFor() async throws {
        let dir = try sessionsDir(files: [900: id])
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = claudePath; control.exitsAfterChecks = [900: 3]
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, timeout: 5, poll: 0.1, sleep: { _ in })
        XCTAssertEqual(result, .success([900]))
    }

    func testAProcessThatDoesNotExitIsBusyAndNeverKilled() async throws {
        let dir = try sessionsDir(files: [900: id])
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = claudePath
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, timeout: 5, poll: 0.1, sleep: { _ in })
        XCTAssertEqual(result, .failure(.stillRunning(pid: 900)))
        XCTAssertEqual(control.terminated, [900], "one SIGTERM and nothing stronger (the control has no other signal)")
        XCTAssertEqual(OriginalStopError.stillRunning(pid: 900).localizedDescription, "Mac chưa dừng được session")
    }

    func testTheWaitIsFiveSecondsOfPolling() async throws {
        let dir = try sessionsDir(files: [900: id])
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = claudePath
        let waited = Waited()
        _ = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, timeout: 5, poll: 0.1, sleep: { await waited.add($0) })
        let total = await waited.total
        XCTAssertEqual(total, 5, accuracy: 0.2)
    }

    func testAProcessThatIsNotClaudeIsNeverSignalled() async throws {
        let dir = try sessionsDir(files: [900: id])
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = "/bin/zsh"
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, sleep: { _ in })
        XCTAssertEqual(result, .failure(.notClaude(pid: 900)))
        XCTAssertTrue(control.terminated.isEmpty)
    }

    func testASessionFileNamingAnotherSessionIsNeverSignalled() async throws {
        let dir = try sessionsDir(files: [900: "11111111-2222-4333-8444-555555555555"])
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = claudePath
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, sleep: { _ in })
        XCTAssertEqual(result, .failure(.sessionMismatch(pid: 900)))
        XCTAssertTrue(control.terminated.isEmpty)
    }

    func testAKnownPidWithNoSessionFileIsNotTrusted() async throws {
        let dir = try sessionsDir(files: [:])
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = claudePath
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, sleep: { _ in })
        XCTAssertEqual(result, .failure(.sessionMismatch(pid: 900)))
        XCTAssertTrue(control.terminated.isEmpty)
    }

    func testADeadPidMeansNothingToStop() async throws {
        let dir = try sessionsDir(files: [900: id])
        let control = FakeProcesses()
        control.paths[900] = claudePath   // not alive
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, sleep: { _ in })
        XCTAssertEqual(result, .success([]))
        XCTAssertTrue(control.terminated.isEmpty)
    }

    func testOnlyTheSessionsOwnProcessesAreSignalled() async throws {
        // 901 belongs to another session and must not be touched; the session's own file names 900 and 902.
        let dir = try sessionsDir(files: [900: id, 901: "11111111-2222-4333-8444-555555555555", 902: id])
        let control = FakeProcesses()
        control.alive = [900, 901, 902]
        for pid: Int32 in [900, 901, 902] { control.paths[pid] = claudePath }
        control.exitsOnTerm = [900, 901, 902]
        let result = await OriginalStopper.stop(sessionId: id, knownPid: 900, sessionsDir: dir, control: control, sleep: { _ in })
        XCTAssertEqual(result, .success([900, 902]))
        XCTAssertFalse(control.terminated.contains(901))
    }

    func testStoppableStateForARunningVerifiedOriginal() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("stoppable-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let claude = root.appendingPathComponent(".claude")
        let cwd = root.appendingPathComponent("work")
        try FileManager.default.createDirectory(at: claude.appendingPathComponent("sessions"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: cwd, withIntermediateDirectories: true)
        let folder = claude.appendingPathComponent("projects/-tmp-work")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("{}\n".utf8).write(to: folder.appendingPathComponent("\(id).jsonl"))
        try Data(#"{"pid":900,"sessionId":"\#(id)"}"#.utf8).write(to: claude.appendingPathComponent("sessions/900.json"))
        let paths = ClaudePaths(claudeDir: claude, requestsDir: root.appendingPathComponent("requests"))
        let control = FakeProcesses()
        control.alive = [900]; control.paths[900] = claudePath

        func session(state: SessionState) -> Session {
            Session(id: id, pid: 900, name: "n", project: "work", cwd: cwd.path, entrypoint: .desktop, kind: "interactive", state: state,
                    statusUpdatedAt: fixtureNow, startedAt: fixtureNow, transcriptURL: nil, control: .viewOnly)
        }
        XCTAssertEqual(Takeover.state(for: session(state: .done), paths: paths, isAlive: { control.isAlive($0) }, control: control), .stoppable(busy: false))
        XCTAssertEqual(Takeover.state(for: session(state: .running), paths: paths, isAlive: { control.isAlive($0) }, control: control), .stoppable(busy: true))
        control.paths[900] = "/bin/zsh"
        XCTAssertEqual(Takeover.state(for: session(state: .done), paths: paths, isAlive: { control.isAlive($0) }, control: control), .unavailable(Takeover.aliveReason),
                       "a live process that is not claude is never offered for stopping")
    }

    func testRouterPassesStopOriginalAndAuditsTheStoppedPid() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, _) = try await rig.login(name: "away-phone", from: "203.0.113.5")
        rig.backend.sessionList = [makeSession("view", control: .viewOnly)]

        await rig.router.handle(#"{"id":2,"op":"session.takeover","sessionId":"view","stopOriginal":"yes"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid", "stopOriginal must be a boolean")
        XCTAssertTrue(rig.backend.takeoverCalls.isEmpty)

        rig.backend.takeoverResult = .success(TakeoverOutcome(sessionId: "resumed-id", stoppedPid: 4242))
        await rig.router.handle(#"{"id":3,"op":"session.takeover","sessionId":"view","stopOriginal":true}"#, from: connection)
        let answer = try io.lastJSON()
        XCTAssertTrue(answer.isOK)
        XCTAssertEqual(answer.dataObject["stoppedPid"] as? Int, 4242)
        XCTAssertEqual(rig.backend.takeoverStopFlags, [true])
        XCTAssertEqual(rig.core.audit.recent(limit: 1).first?.stoppedPid, 4242)

        rig.backend.takeoverResult = .success(TakeoverOutcome(sessionId: "resumed-id"))
        await rig.router.handle(#"{"id":4,"op":"session.takeover","sessionId":"view"}"#, from: connection)
        XCTAssertEqual(rig.backend.takeoverStopFlags, [true, false], "without the flag the behaviour is as before")
        XCTAssertNil(rig.core.audit.recent(limit: 1).first?.stoppedPid)

        rig.backend.takeoverResult = .failure(BridgeFailure(.busy, "Mac chưa dừng được session"))
        await rig.router.handle(#"{"id":5,"op":"session.takeover","sessionId":"view","stopOriginal":true}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "busy")
    }

    func testTheStoppableStateTravelsInTheSessionJSON() {
        var view = makeSession("v", control: .viewOnly)
        view.takeover = .stoppable(busy: true)
        XCTAssertEqual(SessionDTO(view).takeover, TakeoverDTO(available: false, reason: Takeover.aliveReason, stoppable: true, busy: true))
    }
}

private actor Waited {
    private(set) var total: TimeInterval = 0
    func add(_ seconds: TimeInterval) { total += seconds }
}
