import Foundation

/// One `claude` process Console started, attached to a pseudo-terminal Console owns. The pty is what
/// carries replies in, and what the Terminal toggle shows, so the user answers TUI prompts (such as the
/// folder trust question) themselves.
public final class PTYSession: TerminalHandle, @unchecked Sendable {
    public let id = UUID()
    public let cwd: URL
    public let model: String?
    public let startedAt = Date()
    public var rootPid: Int32 { process.processIdentifier }

    private let master: Int32
    private let process: Process
    private let queue = DispatchQueue(label: "local.agentconsole.pty")
    private let lock = NSLock()
    private var readSource: DispatchSourceRead?
    private var listeners: [UUID: (Data) -> Void] = [:]
    private var scrollback = Data()
    private var exitHandlers: [() -> Void] = []
    private(set) var hasExited = false

    /// Bytes kept so a terminal view attached later can show what is already on screen.
    static let scrollbackLimit = 2 * 1024 * 1024

    private init(master: Int32, process: Process, cwd: URL, model: String?) {
        self.master = master
        self.process = process
        self.cwd = cwd
        self.model = model
    }

    public static func spawn(executable: URL, arguments: [String], cwd: URL, model: String?,
                             environment: [String: String],
                             columns: UInt16 = 200, rows: UInt16 = 50) throws -> PTYSession {
        let master = posix_openpt(O_RDWR | O_NOCTTY)
        guard master >= 0, grantpt(master) == 0, unlockpt(master) == 0, let name = ptsname(master) else {
            if master >= 0 { close(master) }
            throw SendError.failed("Không mở được pty.")
        }
        _ = fcntl(master, F_SETFD, FD_CLOEXEC)
        var size = winsize(ws_row: rows, ws_col: columns, ws_xpixel: 0, ws_ypixel: 0)
        _ = ioctl(master, TIOCSWINSZ, &size)

        guard let slave = FileHandle(forUpdatingAtPath: String(cString: name)) else {
            close(master)
            throw SendError.failed("Không mở được đầu slave của pty.")
        }
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.currentDirectoryURL = cwd
        process.environment = environment
        process.standardInput = slave
        process.standardOutput = slave
        process.standardError = slave
        do {
            try process.run()
        } catch {
            close(master)
            ConsoleLog.error("PTYSession.spawn", executable.path, "launch failed: \(error)")
            throw SendError.failed("Không chạy được claude: \(error.localizedDescription)")
        }
        do { try slave.close() } catch { ConsoleLog.error("PTYSession.spawn", cwd.path, "slave close: \(error)") }

        let session = PTYSession(master: master, process: process, cwd: cwd, model: model)
        session.startReading()
        return session
    }

    private func startReading() {
        let source = DispatchSource.makeReadSource(fileDescriptor: master, queue: queue)
        source.setEventHandler { [weak self] in self?.drain() }
        source.setCancelHandler { [master] in close(master) }
        readSource = source
        source.resume()
    }

    private func drain() {
        var buffer = [UInt8](repeating: 0, count: 65536)
        let count = read(master, &buffer, buffer.count)
        if count <= 0 {
            finish()
            return
        }
        let chunk = Data(buffer[0..<count])
        lock.lock()
        scrollback.append(chunk)
        if scrollback.count > Self.scrollbackLimit { scrollback.removeFirst(scrollback.count - Self.scrollbackLimit) }
        let current = Array(listeners.values)
        lock.unlock()
        for listener in current { listener(chunk) }
    }

    private func finish() {
        lock.lock()
        guard !hasExited else { lock.unlock(); return }
        hasExited = true
        let handlers = exitHandlers
        exitHandlers = []
        lock.unlock()
        readSource?.cancel()
        readSource = nil
        for handler in handlers { handler() }
    }

    public var isRunning: Bool {
        lock.lock(); defer { lock.unlock() }
        return !hasExited && process.isRunning
    }

    public func onExit(_ handler: @escaping () -> Void) {
        lock.lock()
        if hasExited { lock.unlock(); handler(); return }
        exitHandlers.append(handler)
        lock.unlock()
    }

    /// Listener gets everything already on screen first (in one chunk), then live output.
    @discardableResult
    public func addListener(_ handler: @escaping (Data) -> Void) -> UUID {
        let token = UUID()
        lock.lock()
        let replay = scrollback
        listeners[token] = handler
        lock.unlock()
        if !replay.isEmpty { handler(replay) }
        return token
    }

    public func removeListener(_ token: UUID) {
        lock.lock(); listeners[token] = nil; lock.unlock()
    }

    public func write(_ text: String) { write(Data(text.utf8)) }

    public func write(_ data: Data) {
        queue.async { [master] in
            var offset = 0
            data.withUnsafeBytes { raw in
                while offset < raw.count {
                    let written = Darwin.write(master, raw.baseAddress!.advanced(by: offset), raw.count - offset)
                    if written <= 0 { return }
                    offset += written
                }
            }
        }
    }

    public func resize(columns: UInt16, rows: UInt16) {
        var size = winsize(ws_row: rows, ws_col: columns, ws_xpixel: 0, ws_ypixel: 0)
        _ = ioctl(master, TIOCSWINSZ, &size)
    }

    /// Blocking: SIGTERM, then SIGKILL if it is still alive after `grace`. For app quit, where a delayed
    /// kill would never run.
    public func terminateAndWait(grace: TimeInterval) {
        guard process.isRunning else { return }
        process.terminate()
        let deadline = Date().addingTimeInterval(grace)
        while process.isRunning, Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        if process.isRunning { kill(process.processIdentifier, SIGKILL) }
    }

    /// SIGTERM first; the process gets a moment to save its session before SIGKILL.
    public func terminate() {
        guard process.isRunning else { finish(); return }
        process.terminate()
        let pid = process.processIdentifier
        DispatchQueue.global().asyncAfter(deadline: .now() + 2) { [weak self] in
            if let self, self.process.isRunning { kill(pid, SIGKILL) }
        }
    }
}

/// The pty sessions Console owns. A Claude session belongs to one when its process is the pty child or
/// a descendant of it (the `claude` launcher may start the real binary as a child).
public final class OwnedSessions: @unchecked Sendable {
    private let lock = NSLock()
    private var sessions: [any TerminalHandle] = []
    private let parentOf: (Int32) -> Int32?

    public init(parentOf: @escaping (Int32) -> Int32? = parentProcessID(of:)) {
        self.parentOf = parentOf
    }

    public var all: [any TerminalHandle] {
        lock.lock(); defer { lock.unlock() }
        return sessions
    }

    public var running: [any TerminalHandle] { all.filter { $0.isRunning } }

    public func add(_ session: any TerminalHandle) {
        lock.lock(); sessions.append(session); lock.unlock()
    }

    public func remove(_ id: UUID) {
        lock.lock(); sessions.removeAll { $0.id == id }; lock.unlock()
    }

    public func pty(owning pid: Int32) -> (any TerminalHandle)? {
        all.first { Self.descends(pid, from: $0.rootPid, parentOf: parentOf) }
    }

    public func isOwned(pid: Int32) -> Bool { pty(owning: pid) != nil }

    static func descends(_ pid: Int32, from root: Int32, parentOf: (Int32) -> Int32?) -> Bool {
        var current = pid
        for _ in 0..<8 {
            if current == root { return true }
            guard let parent = parentOf(current) else { return false }
            current = parent
        }
        return false
    }
}
