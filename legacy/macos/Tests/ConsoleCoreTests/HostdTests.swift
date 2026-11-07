import XCTest
@testable import ConsoleCore

private final class FakeHandle: TerminalHandle, @unchecked Sendable {
    let id = UUID()
    let cwd: URL
    let model: String?
    let startedAt = Date()
    let rootPid: Int32
    private let lock = NSLock()
    private var running = true
    private var exitHandlers: [() -> Void] = []
    private var listeners: [UUID: (Data) -> Void] = [:]
    private var screen = Data()
    private(set) var written: [Data] = []
    private(set) var terminated = false
    private(set) var resizes: [(UInt16, UInt16)] = []

    init(cwd: URL, model: String?, pid: Int32) { self.cwd = cwd; self.model = model; rootPid = pid }

    var isRunning: Bool { lock.withLock { running } }
    func onExit(_ handler: @escaping () -> Void) {
        lock.lock()
        if !running { lock.unlock(); handler(); return }
        exitHandlers.append(handler); lock.unlock()
    }
    func addListener(_ handler: @escaping (Data) -> Void) -> UUID {
        let token = UUID()
        lock.lock(); let replay = screen; listeners[token] = handler; lock.unlock()
        if !replay.isEmpty { handler(replay) }
        return token
    }
    func removeListener(_ token: UUID) { lock.withLock { listeners[token] = nil } }
    func write(_ text: String) { write(Data(text.utf8)) }
    func write(_ data: Data) { lock.withLock { written.append(data) } }
    func resize(columns: UInt16, rows: UInt16) { lock.withLock { resizes.append((columns, rows)) } }
    func terminate() { lock.withLock { terminated = true }; finish() }
    func terminateAndWait(grace: TimeInterval) { terminate() }

    func emit(_ text: String) {
        lock.lock(); screen.append(Data(text.utf8)); let all = Array(listeners.values); lock.unlock()
        all.forEach { $0(Data(text.utf8)) }
    }
    func finish() {
        lock.lock(); running = false; let handlers = exitHandlers; exitHandlers = []; lock.unlock()
        handlers.forEach { $0() }
    }
}

private final class FakeSpawner: HostSpawner, @unchecked Sendable {
    private let lock = NSLock()
    private(set) var spawns: [(executable: String, arguments: [String], cwd: String, model: String?)] = []
    private(set) var handles: [FakeHandle] = []
    var nextPid: Int32 = 5000
    func claudeExecutable() throws -> URL { URL(fileURLWithPath: "/fake/claude") }
    func spawn(executable: URL, arguments: [String], cwd: URL, model: String?) throws -> any TerminalHandle {
        lock.lock(); defer { lock.unlock() }
        spawns.append((executable.path, arguments, cwd.path, model))
        nextPid += 1
        let handle = FakeHandle(cwd: cwd, model: model, pid: nextPid)
        handles.append(handle)
        return handle
    }
}

final class HostdTests: XCTestCase {
    private func tempDir() -> URL {
        // Unix socket paths are limited to 104 bytes, so keep it short.
        let dir = URL(fileURLWithPath: "/tmp/hd-\(UUID().uuidString.prefix(8))")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func makeHost(dir: URL, spawner: FakeSpawner = FakeSpawner(), isAlive: @escaping (Int32) -> Bool = { _ in false },
                          discover: @escaping (Int32) -> String? = { _ in nil }) -> (SessionHost, HostStateStore, FakeSpawner) {
        let state = HostStateStore(url: dir.appendingPathComponent("state.json"))
        let host = SessionHost(spawner: spawner, state: state, isAlive: isAlive, discoverSessionId: discover, discoveryInterval: 0.02, discoveryAttempts: 50)
        return (host, state, spawner)
    }

    // MARK: Framing

    func testFramesRoundTripAndSurviveBeingSplitAnywhere() {
        let id = UUID()
        let json = HostWire.json(["id": .int(7), "op": .string("list"), "ok": .bool(true), "nested": .object(["a": .array([.int(1), .string("x")])])])
        let binary = HostWire.encode(.data(sessionId: id, bytes: Data([0, 1, 2, 255])))
        let stream = json + binary
        var reader = HostFrameReader()
        var frames: [HostFrame] = []
        for byte in stream { frames += reader.feed(Data([byte])) }
        XCTAssertEqual(frames.count, 2)
        guard case .json(let object) = frames[0] else { return XCTFail() }
        XCTAssertEqual(object["id"]?.int, 7)
        XCTAssertEqual(object["ok"]?.bool, true)
        XCTAssertEqual(object["nested"]?.object?["a"]?.array, [.int(1), .string("x")])
        XCTAssertEqual(frames[1], .data(sessionId: id, bytes: Data([0, 1, 2, 255])))
        XCTAssertNil(reader.failure)
    }

    func testOversizedAndMalformedFramesEndTheStream() {
        var big = HostFrameReader()
        XCTAssertTrue(big.feed(Data([0x7F, 0xFF, 0xFF, 0xFF, 1])).isEmpty)
        XCTAssertEqual(big.failure, .tooLarge)
        var bad = HostFrameReader()
        _ = bad.feed(Data([0, 0, 0, 3, 9, 1, 2]))
        XCTAssertEqual(bad.failure, .malformed, "unknown frame type")
        var notJSON = HostFrameReader()
        _ = notJSON.feed(Data([0, 0, 0, 3, 1, 0x7B, 0x7B]))
        XCTAssertEqual(notJSON.failure, .malformed)
        XCTAssertTrue(bad.feed(HostWire.json(["id": .int(1)])).isEmpty, "nothing is read after a failure")
    }

    // MARK: Scrollback

    func testScrollbackKeepsTheLastTwoMegabytesOfARealPty() throws {
        let done = expectation(description: "exited")
        let pty = try PTYSession.spawn(executable: URL(fileURLWithPath: "/bin/sh"),
                                       arguments: ["-c", "head -c 3000000 /dev/zero | tr '\\0' 'a'; echo END; sleep 0.3"],
                                       cwd: URL(fileURLWithPath: "/tmp"), model: nil, environment: ["PATH": "/usr/bin:/bin", "TERM": "xterm"])
        pty.onExit { done.fulfill() }
        wait(for: [done], timeout: 15)
        var total = 0
        var tail = Data()
        pty.addListener { chunk in total += chunk.count; tail.append(chunk) }
        XCTAssertLessThanOrEqual(total, 2 * 1024 * 1024)
        XCTAssertGreaterThan(total, 1_900_000, "a late attach sees nearly all of the last 2 MB")
        XCTAssertTrue(String(decoding: tail.suffix(10), as: UTF8.self).contains("END"), "and it is the newest part")
    }

    // MARK: Host core

    func testSpawnAndResumeUseTheSameArgumentsAsTheApp() throws {
        let dir = tempDir()
        let (host, state, spawner) = makeHost(dir: dir)
        let new = try host.spawnClaude(cwd: URL(fileURLWithPath: "/tmp"), model: "haiku")
        XCTAssertEqual(spawner.spawns[0].arguments, ["--model", "haiku"])
        XCTAssertEqual(spawner.spawns[0].executable, "/fake/claude")
        let resumed = try host.resume(sessionId: "abc-123", cwd: URL(fileURLWithPath: "/tmp"))
        XCTAssertEqual(spawner.spawns[1].arguments, ["--resume", "abc-123"])
        XCTAssertEqual(host.list().map(\.id).sorted { $0.uuidString < $1.uuidString }, [new.id, resumed.id].sorted { $0.uuidString < $1.uuidString })
        XCTAssertEqual(state.records.count, 2)
        _ = try host.spawnCommand(executable: URL(fileURLWithPath: "/bin/zsh"), arguments: ["-f"], cwd: URL(fileURLWithPath: "/tmp"))
        XCTAssertEqual(state.records.count, 2, "a plain command is not recorded: it cannot be resumed")
    }

    func testTheClaudeSessionBehindANewSessionIsFoundAndRecorded() throws {
        let dir = tempDir()
        let (host, state, _) = makeHost(dir: dir, discover: { _ in "found-id" })
        let info = try host.spawnClaude(cwd: URL(fileURLWithPath: "/tmp"), model: nil)
        let deadline = Date().addingTimeInterval(3)
        while state.records.first?.claudeSessionId == nil, Date() < deadline { Thread.sleep(forTimeInterval: 0.02) }
        XCTAssertEqual(state.records.first?.claudeSessionId, "found-id")
        XCTAssertEqual(host.list().first { $0.id == info.id }?.claudeSessionId, "found-id")
    }

    func testASessionThatEndsIsForgotten() throws {
        let dir = tempDir()
        let (host, state, spawner) = makeHost(dir: dir)
        _ = try host.spawnClaude(cwd: URL(fileURLWithPath: "/tmp"), model: nil)
        XCTAssertEqual(state.records.count, 1)
        spawner.handles[0].finish()
        XCTAssertTrue(state.records.isEmpty, "it ended by itself: not brought back later")
    }

    func testRecordedSessionsWithDeadProcessesAreResumedAtStart() throws {
        let dir = tempDir()
        let work = dir.appendingPathComponent("work")
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        let state = HostStateStore(url: dir.appendingPathComponent("state.json"))
        let now = Date()
        state.upsert(HostRecord(id: "a", cwd: work.path, model: nil, claudeSessionId: "session-a", pid: 111, startedAt: now))
        state.upsert(HostRecord(id: "b", cwd: work.path, model: nil, claudeSessionId: "session-b", pid: 222, startedAt: now))   // still alive
        state.upsert(HostRecord(id: "c", cwd: work.path, model: nil, claudeSessionId: nil, pid: 333, startedAt: now))           // no session id
        state.upsert(HostRecord(id: "d", cwd: "/definitely/not/here", model: nil, claudeSessionId: "session-d", pid: 444, startedAt: now))
        state.upsert(HostRecord(id: "e", cwd: work.path, model: nil, claudeSessionId: "session-e", pid: 0, startedAt: now))     // migration record
        let spawner = FakeSpawner()
        let host = SessionHost(spawner: spawner, state: state, isAlive: { $0 == 222 }, discoverSessionId: { _ in nil })

        XCTAssertEqual(host.resumeRecorded(), 2)
        XCTAssertEqual(Set(spawner.spawns.map { $0.arguments }), [["--resume", "session-a"], ["--resume", "session-e"]])
        XCTAssertTrue(spawner.spawns.allSatisfy { $0.cwd == work.path })
        let kept = Set(state.records.map(\.claudeSessionId))
        XCTAssertTrue(kept.contains("session-b"), "a record whose process is alive is left alone")
        XCTAssertFalse(kept.contains(nil), "a record with no Claude id is dropped")
    }

    func testResumeOnStartCanBeSwitchedOff() throws {
        let dir = tempDir()
        let work = dir.appendingPathComponent("w")
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        let state = HostStateStore(url: dir.appendingPathComponent("state.json"))
        state.upsert(HostRecord(id: "a", cwd: work.path, model: nil, claudeSessionId: "s", pid: 0, startedAt: Date()))
        state.setSettings({ var s = HostSettings(); s.resumeOnStart = false; return s }())
        let spawner = FakeSpawner()
        let host = SessionHost(spawner: spawner, state: state, isAlive: { _ in false }, discoverSessionId: { _ in nil })
        XCTAssertEqual(host.resumeRecorded(), 0)
        XCTAssertTrue(spawner.spawns.isEmpty)
        XCTAssertEqual(HostStateStore(url: dir.appendingPathComponent("state.json")).settings.resumeOnStart, false, "the setting is persisted")
        XCTAssertTrue(HostSettings().resumeOnStart, "on by default")
    }

    func testMigrationRecordsAreResumedByTheNextHost() throws {
        let dir = tempDir()
        let work = dir.appendingPathComponent("w")
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        let (first, state, _) = makeHost(dir: dir)
        first.recordForResume(claudeSessionId: "migrated", cwd: work.path, model: nil)
        let spawner = FakeSpawner()
        let second = SessionHost(spawner: spawner, state: HostStateStore(url: dir.appendingPathComponent("state.json")), isAlive: { _ in false }, discoverSessionId: { _ in nil })
        XCTAssertEqual(second.resumeRecorded(), 1)
        XCTAssertEqual(spawner.spawns.first?.arguments, ["--resume", "migrated"])
        XCTAssertEqual(state.records.count, 1)
    }

    func testStateFileIsPrivate() throws {
        let dir = tempDir()
        let url = dir.appendingPathComponent("sub/state.json")
        HostStateStore(url: url).upsert(HostRecord(id: "a", cwd: "/tmp", model: nil, claudeSessionId: "s", pid: 1, startedAt: Date()))
        XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int, 0o600)
        XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: url.deletingLastPathComponent().path)[.posixPermissions] as? Int, 0o700)
    }

    // MARK: Update policy

    func testAnUpdatedHelperRestartsOnlyWhenNothingIsRunning() {
        XCTAssertEqual(HostdUpdatePolicy.decide(binaryChanged: false, runningSessions: 0), .nothing)
        XCTAssertEqual(HostdUpdatePolicy.decide(binaryChanged: false, runningSessions: 3), .nothing)
        XCTAssertEqual(HostdUpdatePolicy.decide(binaryChanged: true, runningSessions: 0), .restartNow)
        XCTAssertEqual(HostdUpdatePolicy.decide(binaryChanged: true, runningSessions: 2), .deferUntilIdle)
    }

    // MARK: Over the socket

    private func startServer(dir: URL, authorize: @escaping @Sendable (uid_t) -> Bool = { $0 == getuid() }) throws -> (HostServer, SessionHost, FakeSpawner, String) {
        let (host, _, spawner) = makeHost(dir: dir)
        let path = dir.appendingPathComponent("h.sock").path
        let server = HostServer(host: host, socketPath: path, authorize: authorize)
        try server.start()
        addTeardownBlock { server.stop() }
        return (server, host, spawner, path)
    }

    func testTheSocketIsPrivateAndServesTheOwner() throws {
        let dir = tempDir()
        let (_, _, _, path) = try startServer(dir: dir)
        XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: path)[.posixPermissions] as? Int, 0o600)
        XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: dir.path)[.posixPermissions] as? Int, 0o700)
        let client = try HostClient.connect(socketPath: path)
        defer { client.close() }
        let hello = try client.request("hello")
        XCTAssertEqual(hello["version"]?.int, HostServer.protocolVersion)
        XCTAssertEqual(hello["pid"]?.int, Int(getpid()))
    }

    func testAnotherUidIsRefusedBeforeAnyRequestIsServed() throws {
        let dir = tempDir()
        let (_, host, spawner, path) = try startServer(dir: dir, authorize: { $0 != getuid() })
        let client = try HostClient.connect(socketPath: path)
        defer { client.close() }
        XCTAssertThrowsError(try client.request("spawn", ["cwd": .string("/tmp"), "kind": .string("claude")], timeout: 2))
        XCTAssertTrue(spawner.spawns.isEmpty)
        XCTAssertTrue(host.list().isEmpty)
    }

    func testASecondHostOnTheSameSocketRefusesToStart() throws {
        let dir = tempDir()
        let (_, _, _, path) = try startServer(dir: dir)
        let (other, _, _) = makeHost(dir: dir)
        XCTAssertThrowsError(try HostServer(host: other, socketPath: path).start()) { XCTAssertEqual($0 as? HostError, .alreadyRunning) }
    }

    func testAStaleSocketFileIsReplaced() throws {
        let dir = tempDir()
        let path = dir.appendingPathComponent("h.sock").path
        FileManager.default.createFile(atPath: path, contents: nil)
        let (host, _, _) = makeHost(dir: dir)
        let server = HostServer(host: host, socketPath: path)
        try server.start()
        addTeardownBlock { server.stop() }
        XCTAssertNoThrow(try HostClient.connect(socketPath: path).request("hello"))
    }

    func testSpawnListWriteAttachResizeAndTerminateOverTheSocket() throws {
        let dir = tempDir()
        let (_, _, spawner, path) = try startServer(dir: dir)
        let client = try HostClient.connect(socketPath: path)
        defer { client.close() }
        let backend = HostBackend(client: client)

        let pty = try backend.spawnClaude(folder: URL(fileURLWithPath: "/tmp"), model: "sonnet")
        XCTAssertEqual(spawner.spawns[0].arguments, ["--model", "sonnet"])
        XCTAssertEqual(try client.sessions().count, 1)
        XCTAssertTrue(pty.isRunning)

        // Output reaches an attached listener: the scrollback first, then live bytes.
        spawner.handles[0].emit("before attach ")
        let received = Locked<String>("")
        let token = pty.addListener { chunk in received.mutate { $0 += String(decoding: chunk, as: UTF8.self) } }
        spawner.handles[0].emit("after")
        try waitUntil { received.value == "before attach after" }

        // Input goes the other way.
        pty.write("hello\r")
        try waitUntil { spawner.handles[0].written.contains(Data("hello\r".utf8)) }
        pty.resize(columns: 120, rows: 40)
        XCTAssertEqual(spawner.handles[0].resizes.last?.0, 120)
        pty.removeListener(token)

        let exited = expectation(description: "exit event")
        pty.onExit { exited.fulfill() }
        pty.terminate()
        wait(for: [exited], timeout: 3)
        XCTAssertTrue(spawner.handles[0].terminated)
        XCTAssertFalse(pty.isRunning)
    }

    func testTerminateNeedsTheConfirmFlag() throws {
        let dir = tempDir()
        let (_, _, spawner, path) = try startServer(dir: dir)
        let client = try HostClient.connect(socketPath: path)
        defer { client.close() }
        let info = try HostBackend(client: client).spawnClaude(folder: URL(fileURLWithPath: "/tmp"), model: nil)
        XCTAssertThrowsError(try client.request("terminate", ["session": .string(info.id.uuidString)])) {
            XCTAssertTrue(($0 as? HostError)?.errorDescription?.contains("confirm") == true, "\($0)")
        }
        XCTAssertFalse(spawner.handles[0].terminated)
    }

    func testAnAppThatRestartsFindsTheRunningSessionsAgain() throws {
        let dir = tempDir()
        let (_, _, spawner, path) = try startServer(dir: dir)
        let first = try HostClient.connect(socketPath: path)
        _ = try HostBackend(client: first).spawnClaude(folder: URL(fileURLWithPath: "/tmp"), model: nil)
        first.close()   // the app quit

        let second = try HostClient.connect(socketPath: path)
        defer { second.close() }
        let found = HostBackend(client: second).existingSessions()
        XCTAssertEqual(found.count, 1)
        XCTAssertTrue(found[0].isRunning)
        XCTAssertEqual(found[0].rootPid, spawner.handles[0].rootPid)
        // And it can type into it.
        found[0].write("still here")
        try waitUntil { spawner.handles[0].written.contains(Data("still here".utf8)) }
    }

    func testSettingsAndStatusOverTheSocket() throws {
        let dir = tempDir()
        let (_, _, _, path) = try startServer(dir: dir)
        let client = try HostClient.connect(socketPath: path)
        defer { client.close() }
        XCTAssertEqual(try client.request("status")["running"]?.int, 0)
        XCTAssertEqual(try client.request("settings.get")["resumeOnStart"]?.bool, true)
        XCTAssertEqual(try client.request("settings.set", ["resumeOnStart": .bool(false)])["resumeOnStart"]?.bool, false)
        XCTAssertEqual(try client.request("settings.get")["resumeOnStart"]?.bool, false)
        XCTAssertThrowsError(try client.request("nope"))
    }

    func testLosingTheHostMarksSessionsGone() throws {
        let dir = tempDir()
        let (server, _, _, path) = try startServer(dir: dir)
        let client = try HostClient.connect(socketPath: path)
        let backend = HostBackend(client: client)
        let pty = try backend.spawnClaude(folder: URL(fileURLWithPath: "/tmp"), model: nil)
        let gone = expectation(description: "gone")
        pty.onExit { gone.fulfill() }
        server.stop()
        wait(for: [gone], timeout: 3)
        XCTAssertFalse(pty.isRunning)
        withExtendedLifetime(backend) {}
    }

    private func waitUntil(_ condition: () -> Bool, timeout: TimeInterval = 3) throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() > deadline { XCTFail("condition not met in \(timeout) s"); return }
            Thread.sleep(forTimeInterval: 0.01)
        }
    }
}

private final class Locked<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: T
    init(_ value: T) { stored = value }
    var value: T { lock.withLock { stored } }
    func mutate(_ change: (inout T) -> Void) { lock.withLock { change(&stored) } }
}
