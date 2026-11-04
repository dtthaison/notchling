import Foundation

/// The real spawner: `claude` (or any command) on a pty, with the same scrubbed environment as a new session.
public final class PTYHostSpawner: HostSpawner, @unchecked Sendable {
    private let lock = NSLock()
    private var environment: [String: String]?

    public init() {}

    private func scrubbedEnvironment() -> [String: String] {
        lock.lock(); defer { lock.unlock() }
        if let environment { return environment }
        let value = SpawnEnvironment.scrubbed(SpawnEnvironment.loginEnvironment())
        environment = value
        return value
    }

    public func claudeExecutable() throws -> URL { try ClaudeExecutable.resolve() }

    public func spawn(executable: URL, arguments: [String], cwd: URL, model: String?) throws -> any TerminalHandle {
        try PTYSession.spawn(executable: executable, arguments: arguments, cwd: cwd, model: model, environment: scrubbedEnvironment())
    }
}

/// Where Console's sessions live: in the session host (they survive Console restarting) or, when no host is
/// installed, inside the app itself (they end with it).
public protocol TerminalBackend: Sendable {
    func spawnClaude(folder: URL, model: String?) throws -> any TerminalHandle
    func resume(sessionId: String, folder: URL) throws -> any TerminalHandle
    func spawnCommand(executable: URL, arguments: [String], cwd: URL) throws -> any TerminalHandle
    /// Sessions that were already running when the app started.
    func existingSessions() -> [any TerminalHandle]
    var sessionsOutliveApp: Bool { get }
    /// Asks the host to bring recorded sessions back after a restart on/off (host only).
    var resumeOnStart: Bool { get set }
}

/// Sessions as children of the app, as before the host existed.
public final class LocalBackend: TerminalBackend, @unchecked Sendable {
    private let spawner = PTYHostSpawner()
    public init() {}
    public var sessionsOutliveApp: Bool { false }
    public var resumeOnStart: Bool { get { false } set {} }
    public func existingSessions() -> [any TerminalHandle] { [] }

    public func spawnClaude(folder: URL, model: String?) throws -> any TerminalHandle {
        try spawner.spawn(executable: try spawner.claudeExecutable(), arguments: model.map { ["--model", $0] } ?? [], cwd: folder, model: model)
    }

    public func resume(sessionId: String, folder: URL) throws -> any TerminalHandle {
        try spawner.spawn(executable: try spawner.claudeExecutable(), arguments: ["--resume", sessionId], cwd: folder, model: nil)
    }

    public func spawnCommand(executable: URL, arguments: [String], cwd: URL) throws -> any TerminalHandle {
        try spawner.spawn(executable: executable, arguments: arguments, cwd: cwd, model: nil)
    }
}

/// Sessions in the host, through its socket.
public final class HostBackend: TerminalBackend, @unchecked Sendable {
    private let client: HostClient
    private let lock = NSLock()
    private var handles: [UUID: RemotePTY] = [:]

    public init(client: HostClient) {
        self.client = client
        client.onEvent = { [weak self] message in
            guard message["event"]?.string == "exit", let text = message["sessionId"]?.string, let id = UUID(uuidString: text) else { return }
            self?.lock.lock(); let handle = self?.handles[id]; self?.lock.unlock()
            handle?.markExited()
        }
        client.onClosed = { [weak self] in
            self?.lock.lock(); let all = Array(self?.handles.values ?? [:].values); self?.lock.unlock()
            for handle in all { handle.markExited() }
        }
    }

    public var sessionsOutliveApp: Bool { true }

    public var resumeOnStart: Bool {
        get { (try? client.request("settings.get")["resumeOnStart"]?.bool) ?? true }
        set { _ = try? client.request("settings.set", ["resumeOnStart": .bool(newValue)]) }
    }

    private func adopt(_ info: HostedInfo) -> RemotePTY {
        lock.lock(); defer { lock.unlock() }
        if let existing = handles[info.id] { return existing }
        let handle = client.remote(info)
        handles[info.id] = handle
        return handle
    }

    private func spawned(_ reply: [String: HostJSON]) throws -> any TerminalHandle {
        guard reply["id"] != nil, let info = HostClient.info(from: HostJSON.object(reply)) else { throw HostError.refused("hostd trả lời không hợp lệ.") }
        return adopt(info)
    }

    public func spawnClaude(folder: URL, model: String?) throws -> any TerminalHandle {
        var params: [String: HostJSON] = ["kind": .string("claude"), "cwd": .string(folder.path)]
        if let model { params["model"] = .string(model) }
        return try spawned(try client.request("spawn", params, timeout: 30))
    }

    public func resume(sessionId: String, folder: URL) throws -> any TerminalHandle {
        try spawned(try client.request("resume", ["sessionId": .string(sessionId), "cwd": .string(folder.path)], timeout: 30))
    }

    public func spawnCommand(executable: URL, arguments: [String], cwd: URL) throws -> any TerminalHandle {
        try spawned(try client.request("spawn", ["kind": .string("command"), "cwd": .string(cwd.path), "executable": .string(executable.path),
                                                 "arguments": .array(arguments.map(HostJSON.string))], timeout: 30))
    }

    public func existingSessions() -> [any TerminalHandle] {
        ((try? client.sessions()) ?? []).filter(\.running).map { adopt($0) }
    }
}

public enum TerminalBackends {
    public static var defaultSocketPath: String {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/AgentConsole/hostd.sock").path
    }

    /// The host when it answers (starting it through launchd if it is installed but not running), the app's own
    /// ptys otherwise.
    public static func connectOrLocal(socketPath: String = defaultSocketPath, startHost: () -> Void = kickstartHost) -> TerminalBackend {
        if let client = try? HostClient.connect(socketPath: socketPath) { return HostBackend(client: client) }
        startHost()
        for _ in 0..<10 {
            Thread.sleep(forTimeInterval: 0.3)
            if let client = try? HostClient.connect(socketPath: socketPath) { return HostBackend(client: client) }
        }
        ConsoleLog.logger.notice("hostd not reachable; sessions will live inside the app")
        return LocalBackend()
    }

    public static var launchAgentPlist: String {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/LaunchAgents/local.agentconsole.hostd.plist").path
    }

    /// Installed means the LaunchAgent is there; a build run from the command line without it keeps sessions local.
    public static var hostIsInstalled: Bool { FileManager.default.fileExists(atPath: launchAgentPlist) }

    public static func kickstartHost() {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        process.arguments = ["kickstart", "gui/\(getuid())/local.agentconsole.hostd"]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do { try process.run(); process.waitUntilExit() } catch { ConsoleLog.error("TerminalBackends.kickstartHost", "launchctl", "\(error)") }
    }
}
