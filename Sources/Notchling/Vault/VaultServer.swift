import Darwin
import Foundation

/// One line of JSON each way over the vault socket.
struct VaultRequest: Codable {
    enum Op: String, Codable { case use, request }
    var op: Op
    /// use: the secrets, the command (shown when the user approves) and the folder it runs in.
    var names: [String]?
    var command: String?
    var cwd: String?
    /// request: the form's fields, filled in for the user.
    var name: String?
    var kind: String?
    var url: String?
    var note: String?
    var reason: String?
    /// request: how to get or renew it (multi-line), and its last day as YYYY-MM-DD.
    var howTo: String?
    var expires: String?
}

struct VaultReply: Codable {
    var ok: Bool
    var error: String?
    /// The user said no (exit 3), as against something going wrong (exit 2).
    var denied: Bool?
    var env: [String: String]?
    var hide: [String: String]?
    var entry: VaultEntry?
}

/// The app's end of the vault socket (`vault.sock`, 0600). It answers only a peer that runs as this user
/// from this app's own executable, i.e. `Notchling --secret` started by an agent, never a script that
/// connects by itself. Each connection gets its own thread, which may wait for the user.
final class VaultServer {
    static let shared = VaultServer()
    static let socketPath = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Notchling/vault.sock").path

    private var started = false

    func start() {
        guard !started else { return }
        started = true
        Thread.detachNewThread { self.serve() }
    }

    private func serve() {
        let path = Self.socketPath
        try? FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        unlink(path)
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0, var addr = Self.address(path) else { return }
        let old = umask(0o077)
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        umask(old)
        guard bound == 0, listen(fd, 16) == 0 else { close(fd); return }
        chmod(path, 0o600)
        while true {
            let client = accept(fd, nil, nil)
            if client < 0 { if errno == EINTR { continue }; break }
            Thread.detachNewThread { self.handle(client) }
        }
    }

    private func handle(_ fd: Int32) {
        defer { close(fd) }
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        guard Self.peerIsNotchling(fd) else {
            Self.send(VaultReply(ok: false, error: "Only Notchling's own command line may use the vault."), to: fd)
            return
        }
        guard let line = Self.readLine(fd), let request = try? JSONDecoder().decode(VaultRequest.self, from: line) else {
            Self.send(VaultReply(ok: false, error: "Bad request."), to: fd)
            return
        }
        let reply: VaultReply
        switch request.op {
        case .use: reply = use(request)
        case .request: reply = ask(request)
        }
        Self.send(reply, to: fd)
    }

    private func use(_ request: VaultRequest) -> VaultReply {
        let list = SecretVault.all()
        var entries: [VaultEntry] = []
        for name in request.names ?? [] {
            guard let entry = list.first(where: { $0.id == name.lowercased() }) else {
                return VaultReply(ok: false, error: "No secret named \(name). Saved: \(list.isEmpty ? "none" : list.map(\.name).joined(separator: ", ")). Ask the user for it with --secret-request.")
            }
            entries.append(entry)
        }
        guard !entries.isEmpty else { return VaultReply(ok: false, error: "No secret named.") }
        let asking = entries.filter(\.asks)
        if !asking.isEmpty {
            switch VaultPrompt.approve(asking, command: request.command ?? "", cwd: request.cwd ?? "") {
            case .deny: return VaultReply(ok: false, error: "The user didn't allow it.", denied: true)
            case .once: break
            case .always: VaultStore.shared.stopAsking(Set(asking.map(\.id)))
            }
        }
        var picked: [(VaultEntry, VaultValue)] = []
        for entry in entries {
            do {
                guard let value = try VaultStore.shared.value(for: entry) else {
                    return VaultReply(ok: false, error: "\(entry.name) has no value saved. Ask the user to update it with --secret-request \(entry.name).")
                }
                picked.append((entry, value))
            } catch {
                return VaultReply(ok: false, error: error.localizedDescription)
            }
        }
        let (env, hide) = SecretVault.environment(for: picked)
        return VaultReply(ok: true, env: env, hide: hide)
    }

    private func ask(_ request: VaultRequest) -> VaultReply {
        guard let name = request.name, VaultEntry.validName(name) else {
            return VaultReply(ok: false, error: "Give a name: letters, digits, - _ or ., starting with a letter.")
        }
        guard let entry = VaultPrompt.request(request) else {
            return VaultReply(ok: false, error: "The user cancelled.", denied: true)
        }
        return VaultReply(ok: true, entry: entry)
    }

    // MARK: Socket helpers (the client uses them too)

    static func address(_ path: String) -> sockaddr_un? {
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        guard bytes.count < MemoryLayout.size(ofValue: addr.sun_path) else { return nil }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            raw.copyBytes(from: bytes)
            raw[bytes.count] = 0
        }
        addr.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        return addr
    }

    /// Same user, and the peer's executable is this one (`/Applications/Notchling.app/Contents/MacOS/Notchling`).
    private static func peerIsNotchling(_ fd: Int32) -> Bool {
        var uid: uid_t = 0, gid: gid_t = 0
        guard getpeereid(fd, &uid, &gid) == 0, uid == getuid() else { return false }
        var pid: pid_t = 0
        var size = socklen_t(MemoryLayout<pid_t>.size)
        guard getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &pid, &size) == 0, pid > 0 else { return false }
        var buffer = [CChar](repeating: 0, count: 4096)
        guard proc_pidpath(pid, &buffer, UInt32(buffer.count)) > 0,
              let mine = Bundle.main.executableURL?.resolvingSymlinksInPath().path else { return false }
        let theirs = URL(fileURLWithPath: String(cString: buffer)).resolvingSymlinksInPath().path
        return theirs == mine
    }

    static func readLine(_ fd: Int32) -> Data? {
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 16384)
        while data.count < 4_000_000 {
            let n = read(fd, &buffer, buffer.count)
            if n < 0 && errno == EINTR { continue }
            if n <= 0 { break }
            data.append(buffer, count: n)
            if let nl = data.firstIndex(of: 0x0A) { return data.prefix(upTo: nl) }
        }
        return data.isEmpty ? nil : data
    }

    static func writeAll(_ data: Data, to fd: Int32) -> Bool {
        var sent = 0
        return data.withUnsafeBytes { raw in
            while sent < raw.count {
                let n = write(fd, raw.baseAddress! + sent, raw.count - sent)
                if n < 0 && errno == EINTR { continue }
                if n <= 0 { return false }
                sent += n
            }
            return true
        }
    }

    private static func send(_ reply: VaultReply, to fd: Int32) {
        guard var data = try? JSONEncoder().encode(reply) else { return }
        data.append(0x0A)
        _ = writeAll(data, to: fd)
    }
}

/// The command line's end: one request, one reply.
enum VaultClient {
    static func send(_ request: VaultRequest) -> VaultReply {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0, var addr = VaultServer.address(VaultServer.socketPath) else {
            return VaultReply(ok: false, error: "Couldn't open a socket.")
        }
        defer { close(fd) }
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        let connected = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard connected == 0 else {
            return VaultReply(ok: false, error: "Notchling isn't running. Ask the user to open it, then try again.")
        }
        guard var data = try? JSONEncoder().encode(request) else { return VaultReply(ok: false, error: "Bad request.") }
        data.append(0x0A)
        // A refused peer gets its reply before the request is read, so read even when the write failed.
        _ = VaultServer.writeAll(data, to: fd)
        guard let line = VaultServer.readLine(fd),
              let reply = try? JSONDecoder().decode(VaultReply.self, from: line) else {
            return VaultReply(ok: false, error: "Notchling didn't answer.")
        }
        return reply
    }
}
