import Foundation

/// How the host starts a process on a pty. The real one is `PTYSession.spawn`; tests pass a fake.
public protocol HostSpawner: Sendable {
    func spawn(executable: URL, arguments: [String], cwd: URL, model: String?) throws -> any TerminalHandle
    /// The `claude` executable.
    func claudeExecutable() throws -> URL
}

public struct HostedInfo: Equatable, Sendable {
    public let id: UUID
    public let cwd: String
    public let model: String?
    public let pid: Int32
    public let startedAt: Date
    public let running: Bool
    public let claudeSessionId: String?
}

public enum HostEvent: Sendable {
    case exited(id: UUID, status: Int)
    case spawned(id: UUID)
}

/// The sessions the host owns: starts them, remembers them, finds the Claude session behind each, brings dead ones
/// back after a restart, and hands out the pty for attaching. No sockets here; `HostServer` puts this on one.
public final class SessionHost: @unchecked Sendable {
    private let spawner: HostSpawner
    public let state: HostStateStore
    private let isAlive: (Int32) -> Bool
    private let discoverSessionId: (Int32) -> String?
    private let discoveryInterval: TimeInterval
    private let discoveryAttempts: Int
    private let lock = NSLock()
    private var sessions: [UUID: any TerminalHandle] = [:]
    private var claudeIds: [UUID: String] = [:]
    public var onEvent: (HostEvent) -> Void = { _ in }

    public init(spawner: HostSpawner, state: HostStateStore, isAlive: @escaping (Int32) -> Bool = isProcessAlive,
                discoverSessionId: @escaping (Int32) -> String?, discoveryInterval: TimeInterval = 1, discoveryAttempts: Int = 600) {
        self.spawner = spawner
        self.state = state
        self.isAlive = isAlive
        self.discoverSessionId = discoverSessionId
        self.discoveryInterval = discoveryInterval
        self.discoveryAttempts = discoveryAttempts
    }

    // MARK: - Starting

    @discardableResult
    public func spawnClaude(cwd: URL, model: String?) throws -> HostedInfo {
        let arguments = model.map { ["--model", $0] } ?? []
        return try start(executable: try spawner.claudeExecutable(), arguments: arguments, cwd: cwd, model: model, claudeSessionId: nil, record: true)
    }

    @discardableResult
    public func resume(sessionId: String, cwd: URL) throws -> HostedInfo {
        try start(executable: try spawner.claudeExecutable(), arguments: ["--resume", sessionId], cwd: cwd, model: nil, claudeSessionId: sessionId, record: true)
    }

    /// Any command on a pty (tests, the demo terminal). Not recorded: it cannot be resumed.
    @discardableResult
    public func spawnCommand(executable: URL, arguments: [String], cwd: URL) throws -> HostedInfo {
        try start(executable: executable, arguments: arguments, cwd: cwd, model: nil, claudeSessionId: nil, record: false)
    }

    private func start(executable: URL, arguments: [String], cwd: URL, model: String?, claudeSessionId: String?, record: Bool) throws -> HostedInfo {
        let handle = try spawner.spawn(executable: executable, arguments: arguments, cwd: cwd, model: model)
        lock.lock()
        sessions[handle.id] = handle
        if let claudeSessionId { claudeIds[handle.id] = claudeSessionId }
        lock.unlock()
        if record {
            state.upsert(HostRecord(id: handle.id.uuidString, cwd: cwd.path, model: model, claudeSessionId: claudeSessionId, pid: handle.rootPid, startedAt: handle.startedAt))
            if claudeSessionId == nil { watchForClaudeSession(handle, record: HostRecord(id: handle.id.uuidString, cwd: cwd.path, model: model, claudeSessionId: nil, pid: handle.rootPid, startedAt: handle.startedAt)) }
        }
        handle.onExit { [weak self, id = handle.id] in self?.exited(id: id) }
        onEvent(.spawned(id: handle.id))
        return info(handle)
    }

    /// The session file appears once Claude is past its startup prompts; remember its id so the session can be resumed.
    private func watchForClaudeSession(_ handle: any TerminalHandle, record: HostRecord) {
        DispatchQueue.global().async { [self] in
            for _ in 0..<discoveryAttempts {
                if !handle.isRunning { return }
                if let id = discoverSessionId(handle.rootPid) {
                    lock.lock(); claudeIds[handle.id] = id; lock.unlock()
                    var updated = record
                    updated.claudeSessionId = id
                    state.upsert(updated)
                    return
                }
                Thread.sleep(forTimeInterval: discoveryInterval)
            }
        }
    }

    private func exited(id: UUID) {
        // The process ended by itself (or was ended on purpose): there is nothing to bring back.
        state.remove(id: id.uuidString)
        lock.lock(); sessions[id] = nil; claudeIds[id] = nil; lock.unlock()
        onEvent(.exited(id: id, status: 0))
    }

    // MARK: - Asking

    private func info(_ handle: any TerminalHandle) -> HostedInfo {
        lock.lock(); let claudeId = claudeIds[handle.id]; lock.unlock()
        return HostedInfo(id: handle.id, cwd: handle.cwd.path, model: handle.model, pid: handle.rootPid, startedAt: handle.startedAt,
                          running: handle.isRunning, claudeSessionId: claudeId)
    }

    public func list() -> [HostedInfo] {
        lock.lock(); let all = Array(sessions.values); lock.unlock()
        return all.map(info).sorted { $0.startedAt < $1.startedAt }
    }

    public var runningCount: Int { list().filter(\.running).count }

    public func session(_ id: UUID) -> (any TerminalHandle)? {
        lock.lock(); defer { lock.unlock() }
        return sessions[id]
    }

    public func terminate(_ id: UUID) {
        guard let handle = session(id) else { return }
        state.remove(id: id.uuidString)
        handle.terminate()
    }

    // MARK: - After a restart

    /// Brings back recorded sessions whose process is gone: `claude --resume <id>` in the same folder. Records
    /// without a known Claude session id, or whose folder is gone, are dropped. Returns how many were resumed.
    @discardableResult
    public func resumeRecorded() -> Int {
        guard state.settings.resumeOnStart else { return 0 }
        var resumed = 0
        for record in state.records {
            let alive = record.pid > 1 && isAlive(record.pid)
            guard !alive else { continue }
            state.remove(id: record.id)
            guard let claudeId = record.claudeSessionId else {
                ConsoleLog.logger.notice("hostd: \(record.id, privacy: .public) has no Claude session id, not resuming")
                continue
            }
            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: record.cwd, isDirectory: &isDirectory), isDirectory.boolValue else {
                ConsoleLog.logger.notice("hostd: folder of \(claudeId, privacy: .public) is gone, not resuming")
                continue
            }
            do {
                _ = try resume(sessionId: claudeId, cwd: URL(fileURLWithPath: record.cwd))
                resumed += 1
            } catch {
                ConsoleLog.error("SessionHost.resumeRecorded", claudeId, "\(error)")
            }
        }
        return resumed
    }

    /// Writes a record for a session that is running outside the host right now, to be resumed when the host starts
    /// (the migration of sessions that live inside an old Agent Console).
    public func recordForResume(claudeSessionId: String, cwd: String, model: String?) {
        state.upsert(HostRecord(id: UUID().uuidString, cwd: cwd, model: model, claudeSessionId: claudeSessionId, pid: 0, startedAt: Date()))
    }
}
