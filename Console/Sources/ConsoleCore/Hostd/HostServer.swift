import Darwin
import Foundation

/// SessionHost on a Unix socket. Only the user who runs the host may connect (checked with getpeereid on every
/// connection); the socket's folder is 0700 and the socket itself 0600.
public final class HostServer: @unchecked Sendable {
    public static let protocolVersion = 1

    private let host: SessionHost
    public let socketPath: String
    private let authorize: @Sendable (uid_t) -> Bool
    private let lock = NSLock()
    private var listenFD: Int32 = -1
    private var connections: [ObjectIdentifier: HostConnection] = [:]
    private var running = false

    public init(host: SessionHost, socketPath: String, authorize: @escaping @Sendable (uid_t) -> Bool = { $0 == getuid() }) {
        self.host = host
        self.socketPath = socketPath
        self.authorize = authorize
        host.onEvent = { [weak self] event in self?.broadcast(event) }
    }

    public func start() throws {
        let directory = URL(fileURLWithPath: socketPath).deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)

        // A leftover socket file from a host that died: remove it, unless something still answers on it.
        if FileManager.default.fileExists(atPath: socketPath) {
            if let existing = try? HostClient.connect(socketPath: socketPath, timeout: 0.5) {
                existing.close()
                throw HostError.alreadyRunning
            }
            unlink(socketPath)
        }

        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw HostError.socket(errno) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(socketPath.utf8CString)
        guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else { close(fd); throw HostError.pathTooLong }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            for (index, byte) in bytes.enumerated() { raw[index] = UInt8(bitPattern: byte) }
        }
        let bound = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard bound == 0 else { let code = errno; close(fd); throw HostError.socket(code) }
        chmod(socketPath, 0o600)
        guard listen(fd, 16) == 0 else { let code = errno; close(fd); throw HostError.socket(code) }
        lock.lock(); listenFD = fd; running = true; lock.unlock()

        Thread.detachNewThread { [weak self] in self?.acceptLoop(fd) }
    }

    public func stop() {
        lock.lock()
        running = false
        let fd = listenFD
        listenFD = -1
        let open = Array(connections.values)
        connections = [:]
        lock.unlock()
        if fd >= 0 { shutdown(fd, SHUT_RDWR); close(fd) }
        for connection in open { connection.close() }
        unlink(socketPath)
    }

    private func acceptLoop(_ fd: Int32) {
        while true {
            let client = accept(fd, nil, nil)
            lock.lock(); let stillRunning = running; lock.unlock()
            guard stillRunning else { if client >= 0 { close(client) }; return }
            guard client >= 0 else { continue }
            // A write to a client that has gone must fail with EPIPE, not kill the process with SIGPIPE.
            var noSigPipe: Int32 = 1
            setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size))
            var uid: uid_t = 0, gid: gid_t = 0
            guard getpeereid(client, &uid, &gid) == 0, authorize(uid) else {
                ConsoleLog.logger.notice("hostd: refused a connection from uid \(uid)")
                close(client)
                continue
            }
            let connection = HostConnection(fd: client, host: host) { [weak self] connection in
                guard let self else { return }
                self.lock.lock(); self.connections[ObjectIdentifier(connection)] = nil; self.lock.unlock()
            }
            lock.lock(); connections[ObjectIdentifier(connection)] = connection; lock.unlock()
            connection.run()
        }
    }

    private func broadcast(_ event: HostEvent) {
        lock.lock(); let all = Array(connections.values); lock.unlock()
        var message: [String: HostJSON]
        switch event {
        case .exited(let id, let status): message = ["event": .string("exit"), "sessionId": .string(id.uuidString), "status": .int(status)]
        case .spawned(let id): message = ["event": .string("spawned"), "sessionId": .string(id.uuidString)]
        }
        for connection in all { connection.send(HostWire.json(message)) }
    }
}

public enum HostError: Error, Equatable, LocalizedError {
    case alreadyRunning, pathTooLong, socket(Int32), connectFailed, timeout, closed, refused(String)

    public var errorDescription: String? {
        switch self {
        case .alreadyRunning: return "Một hostd khác đang chạy trên socket này."
        case .pathTooLong: return "Đường dẫn socket quá dài."
        case .socket(let code): return "Lỗi socket (\(code))."
        case .connectFailed: return "Không kết nối được tới hostd."
        case .timeout: return "hostd không trả lời."
        case .closed: return "hostd đã đóng kết nối."
        case .refused(let why): return why
        }
    }
}

/// One connected client of the server.
final class HostConnection: @unchecked Sendable {
    private let fd: Int32
    private let host: SessionHost
    private let onClose: (HostConnection) -> Void
    private let writeLock = NSLock()
    private let lock = NSLock()
    private var listeners: [UUID: UUID] = [:]   // session id -> listener token
    private var closed = false

    init(fd: Int32, host: SessionHost, onClose: @escaping (HostConnection) -> Void) {
        self.fd = fd
        self.host = host
        self.onClose = onClose
    }

    func run() { Thread.detachNewThread { [self] in readLoop() } }

    func send(_ data: Data) {
        writeLock.lock(); defer { writeLock.unlock() }
        lock.lock(); let isClosed = closed; lock.unlock()
        guard !isClosed else { return }
        data.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let written = Darwin.write(fd, raw.baseAddress!.advanced(by: offset), raw.count - offset)
                if written <= 0 { return }
                offset += written
            }
        }
    }

    func close() {
        lock.lock()
        if closed { lock.unlock(); return }
        closed = true
        let attached = listeners
        listeners = [:]
        lock.unlock()
        for (sessionId, token) in attached { host.session(sessionId)?.removeListener(token) }
        shutdown(fd, SHUT_RDWR)
        Darwin.close(fd)
        onClose(self)
    }

    private func readLoop() {
        var reader = HostFrameReader()
        var buffer = [UInt8](repeating: 0, count: 65536)
        while true {
            let count = read(fd, &buffer, buffer.count)
            if count <= 0 { break }
            for frame in reader.feed(Data(buffer[0..<count])) { handle(frame) }
            if reader.failure != nil { break }
        }
        close()
    }

    private func handle(_ frame: HostFrame) {
        switch frame {
        case .data(let id, let bytes):
            host.session(id)?.write(bytes)
        case .json(let message):
            guard let id = message["id"]?.int, let op = message["op"]?.string else { return }
            do {
                let data = try perform(op, message)
                send(HostWire.json(["id": .int(id), "ok": .bool(true), "data": .object(data)]))
            } catch {
                send(HostWire.json(["id": .int(id), "ok": .bool(false), "error": .string("\(error.localizedDescription)")]))
            }
        }
    }

    private func requireUUID(_ message: [String: HostJSON], _ key: String) throws -> UUID {
        guard let text = message[key]?.string, let id = UUID(uuidString: text) else { throw HostError.refused("Thiếu \"\(key)\".") }
        return id
    }

    private func perform(_ op: String, _ message: [String: HostJSON]) throws -> [String: HostJSON] {
        switch op {
        case "hello":
            return ["version": .int(HostServer.protocolVersion), "pid": .int(Int(getpid())), "running": .int(host.runningCount)]
        case "status":
            return ["running": .int(host.runningCount), "total": .int(host.list().count)]
        case "list":
            return ["sessions": .array(host.list().map(Self.json(of:)))]
        case "spawn":
            guard let cwd = message["cwd"]?.string else { throw HostError.refused("Thiếu \"cwd\".") }
            let url = URL(fileURLWithPath: cwd)
            switch message["kind"]?.string ?? "claude" {
            case "claude": return Self.json(of: try host.spawnClaude(cwd: url, model: message["model"]?.string)).asObject
            case "command":
                guard let executable = message["executable"]?.string else { throw HostError.refused("Thiếu \"executable\".") }
                let arguments = message["arguments"]?.array?.compactMap(\.string) ?? []
                return Self.json(of: try host.spawnCommand(executable: URL(fileURLWithPath: executable), arguments: arguments, cwd: url)).asObject
            default: throw HostError.refused("kind không hợp lệ.")
            }
        case "resume":
            guard let sessionId = message["sessionId"]?.string, let cwd = message["cwd"]?.string else { throw HostError.refused("Thiếu sessionId hoặc cwd.") }
            return Self.json(of: try host.resume(sessionId: sessionId, cwd: URL(fileURLWithPath: cwd))).asObject
        case "resize":
            let id = try requireUUID(message, "session")
            host.session(id)?.resize(columns: UInt16(clamping: message["columns"]?.int ?? 200), rows: UInt16(clamping: message["rows"]?.int ?? 50))
            return [:]
        case "attach":
            let id = try requireUUID(message, "session")
            guard let handle = host.session(id) else { throw HostError.refused("Không có session này.") }
            lock.lock()
            let existing = listeners[id]
            lock.unlock()
            if let existing { handle.removeListener(existing) }
            // The listener replays the scrollback first, then live output, as binary frames.
            let token = handle.addListener { [weak self] data in self?.send(HostWire.encode(.data(sessionId: id, bytes: data))) }
            lock.lock(); listeners[id] = token; lock.unlock()
            return [:]
        case "detach":
            let id = try requireUUID(message, "session")
            lock.lock(); let token = listeners.removeValue(forKey: id); lock.unlock()
            if let token { host.session(id)?.removeListener(token) }
            return [:]
        case "terminate":
            let id = try requireUUID(message, "session")
            guard message["confirm"]?.bool == true else { throw HostError.refused("Cần confirm: true để dừng session.") }
            host.terminate(id)
            return [:]
        case "settings.get":
            return ["resumeOnStart": .bool(host.state.settings.resumeOnStart)]
        case "settings.set":
            var settings = host.state.settings
            if let value = message["resumeOnStart"]?.bool { settings.resumeOnStart = value }
            host.state.setSettings(settings)
            return ["resumeOnStart": .bool(settings.resumeOnStart)]
        default:
            throw HostError.refused("Không có lệnh \"\(op)\".")
        }
    }

    static func json(of info: HostedInfo) -> HostJSON {
        var object: [String: HostJSON] = [
            "id": .string(info.id.uuidString), "cwd": .string(info.cwd), "pid": .int(Int(info.pid)),
            "startedAt": .int(Int(info.startedAt.timeIntervalSince1970 * 1000)), "running": .bool(info.running),
        ]
        if let model = info.model { object["model"] = .string(model) }
        if let claudeId = info.claudeSessionId { object["claudeSessionId"] = .string(claudeId) }
        return .object(object)
    }
}

extension HostJSON {
    var asObject: [String: HostJSON] { object ?? [:] }
}
