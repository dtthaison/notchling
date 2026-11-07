import Darwin
import Foundation

/// The app's side of the host socket: blocking requests with a timeout, a reader thread for replies, events and
/// terminal output. One client per app; `RemotePTY` objects share it.
public final class HostClient: @unchecked Sendable {
    private let fd: Int32
    private let lock = NSLock()
    private let writeLock = NSLock()
    private var nextId = 1
    private var waiters: [Int: (DispatchSemaphore, Box)] = [:]
    private var outputHandlers: [UUID: (Data) -> Void] = [:]
    private var closed = false
    /// "exit" and "spawned" events, on the reader thread.
    public var onEvent: ([String: HostJSON]) -> Void = { _ in }
    /// Called once when the connection ends (the host stopped or crashed).
    public var onClosed: () -> Void = {}

    private final class Box: @unchecked Sendable { var reply: [String: HostJSON]? }

    private init(fd: Int32) { self.fd = fd }

    public static func connect(socketPath: String, timeout: TimeInterval = 3) throws -> HostClient {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw HostError.socket(errno) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(socketPath.utf8CString)
        guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else { Darwin.close(fd); throw HostError.pathTooLong }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            for (index, byte) in bytes.enumerated() { raw[index] = UInt8(bitPattern: byte) }
        }
        let result = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard result == 0 else { Darwin.close(fd); throw HostError.connectFailed }
        var noSigPipe: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size))
        var tv = timeval(tv_sec: Int(timeout), tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
        let client = HostClient(fd: fd)
        Thread.detachNewThread { client.readLoop() }
        return client
    }

    public var isConnected: Bool { lock.withLock { !closed } }

    public func close() {
        lock.lock()
        if closed { lock.unlock(); return }
        closed = true
        let pending = waiters
        waiters = [:]
        lock.unlock()
        shutdown(fd, SHUT_RDWR)
        Darwin.close(fd)
        for (_, waiter) in pending { waiter.0.signal() }
        onClosed()
    }

    private func readLoop() {
        var reader = HostFrameReader()
        var buffer = [UInt8](repeating: 0, count: 65536)
        while true {
            let count = read(fd, &buffer, buffer.count)
            if count <= 0 { break }
            for frame in reader.feed(Data(buffer[0..<count])) { dispatch(frame) }
            if reader.failure != nil { break }
        }
        close()
    }

    private func dispatch(_ frame: HostFrame) {
        switch frame {
        case .data(let id, let bytes):
            lock.lock(); let handler = outputHandlers[id]; lock.unlock()
            handler?(bytes)
        case .json(let message):
            if let id = message["id"]?.int {
                lock.lock(); let waiter = waiters.removeValue(forKey: id); lock.unlock()
                waiter?.1.reply = message
                waiter?.0.signal()
            } else if message["event"] != nil {
                onEvent(message)
            }
        }
    }

    /// A request and its answer's `data`. Throws `HostError.refused` with the host's message when it says no.
    @discardableResult
    public func request(_ op: String, _ params: [String: HostJSON] = [:], timeout: TimeInterval = 5) throws -> [String: HostJSON] {
        let semaphore = DispatchSemaphore(value: 0)
        let box = Box()
        lock.lock()
        guard !closed else { lock.unlock(); throw HostError.closed }
        let id = nextId; nextId += 1
        waiters[id] = (semaphore, box)
        lock.unlock()
        var message = params
        message["id"] = .int(id)
        message["op"] = .string(op)
        send(HostWire.json(message))
        guard semaphore.wait(timeout: .now() + timeout) == .success else {
            lock.lock(); waiters[id] = nil; lock.unlock()
            throw HostError.timeout
        }
        guard let reply = box.reply else { throw HostError.closed }
        guard reply["ok"]?.bool == true else { throw HostError.refused(reply["error"]?.string ?? "Lỗi không rõ.") }
        return reply["data"]?.object ?? [:]
    }

    func send(_ data: Data) {
        writeLock.lock(); defer { writeLock.unlock() }
        data.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let written = Darwin.write(fd, raw.baseAddress!.advanced(by: offset), raw.count - offset)
                if written <= 0 { return }
                offset += written
            }
        }
    }

    func setOutputHandler(_ id: UUID, _ handler: ((Data) -> Void)?) {
        lock.lock(); outputHandlers[id] = handler; lock.unlock()
    }

    // MARK: - Typed calls

    public func sessions() throws -> [HostedInfo] {
        try request("list")["sessions"]?.array?.compactMap(Self.info(from:)) ?? []
    }

    static func info(from value: HostJSON) -> HostedInfo? {
        guard let object = value.object, let idText = object["id"]?.string, let id = UUID(uuidString: idText), let cwd = object["cwd"]?.string else { return nil }
        return HostedInfo(id: id, cwd: cwd, model: object["model"]?.string, pid: Int32(object["pid"]?.int ?? 0),
                          startedAt: Date(timeIntervalSince1970: Double(object["startedAt"]?.int ?? 0) / 1000),
                          running: object["running"]?.bool ?? false, claudeSessionId: object["claudeSessionId"]?.string)
    }

    public func remote(_ info: HostedInfo) -> RemotePTY { RemotePTY(client: self, info: info) }
}

/// A session that lives in the host, seen through the same interface as a local pty.
public final class RemotePTY: TerminalHandle, @unchecked Sendable {
    public let id: UUID
    public let cwd: URL
    public let model: String?
    public let startedAt: Date
    public let rootPid: Int32

    private let client: HostClient
    private let lock = NSLock()
    private var listeners: [UUID: (Data) -> Void] = [:]
    private var exitHandlers: [() -> Void] = []
    private var running: Bool
    private var attached = false

    init(client: HostClient, info: HostedInfo) {
        self.client = client
        id = info.id
        cwd = URL(fileURLWithPath: info.cwd)
        model = info.model
        startedAt = info.startedAt
        rootPid = info.pid
        running = info.running
    }

    public var isRunning: Bool { lock.withLock { running && client.isConnected } }

    /// Called when the host reports the process exited, or the connection to the host is lost.
    public func markExited() {
        lock.lock()
        guard running else { lock.unlock(); return }
        running = false
        let handlers = exitHandlers
        exitHandlers = []
        lock.unlock()
        for handler in handlers { handler() }
    }

    public func onExit(_ handler: @escaping () -> Void) {
        lock.lock()
        if !running { lock.unlock(); handler(); return }
        exitHandlers.append(handler)
        lock.unlock()
    }

    @discardableResult
    public func addListener(_ handler: @escaping (Data) -> Void) -> UUID {
        let token = UUID()
        lock.lock()
        listeners[token] = handler
        let needsAttach = !attached
        attached = true
        lock.unlock()
        if needsAttach {
            client.setOutputHandler(id) { [weak self] data in
                guard let self else { return }
                self.lock.lock(); let all = Array(self.listeners.values); self.lock.unlock()
                for listener in all { listener(data) }
            }
            do { try client.request("attach", ["session": .string(id.uuidString)]) } catch {
                ConsoleLog.error("RemotePTY.addListener", id.uuidString, "\(error)")
            }
        }
        return token
    }

    public func removeListener(_ token: UUID) {
        lock.lock()
        listeners[token] = nil
        let last = listeners.isEmpty && attached
        if last { attached = false }
        lock.unlock()
        if last {
            client.setOutputHandler(id, nil)
            _ = try? client.request("detach", ["session": .string(id.uuidString)])
        }
    }

    public func write(_ text: String) { write(Data(text.utf8)) }

    public func write(_ data: Data) { client.send(HostWire.encode(.data(sessionId: id, bytes: data))) }

    public func resize(columns: UInt16, rows: UInt16) {
        _ = try? client.request("resize", ["session": .string(id.uuidString), "columns": .int(Int(columns)), "rows": .int(Int(rows))])
    }

    public func terminate() {
        _ = try? client.request("terminate", ["session": .string(id.uuidString), "confirm": .bool(true)])
    }

    public func terminateAndWait(grace: TimeInterval) { terminate() }
}
