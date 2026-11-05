import Foundation

/// One Agent Client Protocol server process (`agy acp`): JSON-RPC 2.0, one message per line on stdin
/// and stdout. Everything here runs on the main thread: replies, notifications and the agent's own
/// requests (`session/request_permission`) are handed over there, in the order they arrived.
final class ACPConnection {
    struct Failure: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    private let process = Process()
    private let stdin = Pipe()
    private let stdout = Pipe()
    private let stderr = Pipe()
    private var buffer = Data()
    private var nextID = 1
    private var waiting: [Int: (Result<[String: Any], Error>) -> Void] = [:]
    /// The last lines it printed to stderr, for an error message when it dies.
    private var errorTail = ""
    private(set) var closed = false

    /// `session/update` and other notifications: method and params.
    var onNotification: ((String, [String: Any]) -> Void)?
    /// A call from the agent that wants an answer (`respond`): its id, method and params.
    var onRequest: ((Any, String, [String: Any]) -> Void)?
    var onExit: ((String?) -> Void)?

    init(binary: String, arguments: [String] = ["acp"], folder: String) {
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = arguments
        process.currentDirectoryURL = URL(fileURLWithPath: folder.isEmpty ? NSHomeDirectory() : folder)
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = ["/opt/homebrew/bin", "/usr/local/bin", NSHomeDirectory() + "/.local/bin", env["PATH"] ?? "/usr/bin:/bin"].joined(separator: ":")
        env["NO_COLOR"] = "1"
        process.environment = env
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = stderr
    }

    func start() throws {
        stdout.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil; return }
            DispatchQueue.main.async { self?.received(chunk) }
        }
        stderr.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil; return }
            let text = String(decoding: chunk, as: UTF8.self)
            DispatchQueue.main.async {
                guard let self else { return }
                self.errorTail = String((self.errorTail + text).suffix(2000))
            }
        }
        process.terminationHandler = { [weak self] _ in
            // Let the last output arrive first.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { self?.ended() }
        }
        try process.run()
    }

    /// A request; `done` runs on the main thread with the result or the error.
    func request(_ method: String, _ params: [String: Any], done: @escaping (Result<[String: Any], Error>) -> Void) {
        guard !closed else { done(.failure(Failure(message: "Antigravity isn't running."))); return }
        let id = nextID
        nextID += 1
        waiting[id] = done
        write(["jsonrpc": "2.0", "id": id, "method": method, "params": params])
    }

    func notify(_ method: String, _ params: [String: Any]) {
        write(["jsonrpc": "2.0", "method": method, "params": params])
    }

    func respond(_ id: Any, result: [String: Any]) {
        write(["jsonrpc": "2.0", "id": id, "result": result])
    }

    func respond(_ id: Any, errorCode: Int, message: String) {
        write(["jsonrpc": "2.0", "id": id, "error": ["code": errorCode, "message": message]])
    }

    func close() {
        guard !closed else { return }
        closed = true
        try? stdin.fileHandleForWriting.close()
        if process.isRunning { process.terminate() }
        failWaiting("Antigravity was closed.")
    }

    // MARK: Private

    private func write(_ message: [String: Any]) {
        guard !closed, var data = try? JSONSerialization.data(withJSONObject: message) else { return }
        data.append(0x0A)
        do { try stdin.fileHandleForWriting.write(contentsOf: data) } catch { ended() }
    }

    private func received(_ chunk: Data) {
        buffer.append(chunk)
        while let newline = buffer.firstIndex(of: 0x0A) {
            let line = buffer[buffer.startIndex..<newline]
            buffer.removeSubrange(buffer.startIndex...newline)
            guard let message = (try? JSONSerialization.jsonObject(with: Data(line))) as? [String: Any] else { continue }
            handle(message)
        }
    }

    private func handle(_ message: [String: Any]) {
        if let method = message["method"] as? String {
            let params = message["params"] as? [String: Any] ?? [:]
            if let id = message["id"] {
                onRequest?(id, method, params)
            } else {
                onNotification?(method, params)
            }
            return
        }
        guard let id = (message["id"] as? Int) ?? Int(message["id"] as? String ?? ""), let done = waiting.removeValue(forKey: id) else { return }
        if let error = message["error"] as? [String: Any] {
            done(.failure(Failure(message: error["message"] as? String ?? "Antigravity returned an error.")))
        } else {
            done(.success(message["result"] as? [String: Any] ?? [:]))
        }
    }

    private func ended() {
        let wasClosed = closed
        closed = true
        let tail = errorTail.split(whereSeparator: \.isNewline).last.map(String.init)
        failWaiting(tail ?? "Antigravity stopped.")
        if !wasClosed { onExit?(tail) }
        onExit = nil
    }

    private func failWaiting(_ message: String) {
        let pending = waiting
        waiting = [:]
        for done in pending.values { done(.failure(Failure(message: message))) }
    }
}
