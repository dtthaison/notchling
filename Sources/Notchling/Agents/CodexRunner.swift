import Foundation

/// Runs Codex turns for Chat with `codex exec`: a new session (`codex exec -C <folder> <prompt>`) or
/// the next turn of one (`codex exec resume <id> <prompt>`). Each turn is its own process that
/// writes to the session's file and exits when the turn ends, so Chat reads the conversation from the
/// file like any other session. `--json` gives the session id (`thread.started`) for a new one.
/// Exec mode can't ask for approval, so turns run in the workspace-write sandbox: the session's
/// folder is writable, the network and the rest of the disk aren't.
final class CodexRunner {
    static let shared = CodexRunner()

    private let lock = NSLock()
    private var processes: [String: Process] = [:]

    /// Session ids with a turn running from here.
    var running: Set<String> {
        lock.lock(); defer { lock.unlock() }
        return Set(processes.keys)
    }

    func isRunning(_ sessionID: String) -> Bool { running.contains(sessionID) }

    static func arguments(prompt: String, session: String?, folder: String, pick: CodexPick = CodexPick()) -> [String] {
        var common = ["--json", "--skip-git-repo-check", "-c", "sandbox_mode=\"workspace-write\""]
        // A picked model or effort holds from this turn on; Codex keeps its own default otherwise.
        if let model = pick.model { common += ["-m", model] }
        if let effort = pick.effort { common += ["-c", "model_reasoning_effort=\"\(effort)\""] }
        if let session {
            return ["exec", "resume"] + common + [session, prompt]
        }
        return ["exec"] + common + ["-C", folder, prompt]
    }

    /// Starts a turn. `started` gets the session id once Codex names it (at once for a resume), or an
    /// error; `ended` runs on the main thread when the turn's process exits, with what it printed last
    /// when it failed.
    func run(_ prompt: String, session: String?, folder: String, pick: CodexPick = CodexPick(),
             started: @escaping (Result<String, Error>) -> Void,
             ended: @escaping (_ sessionID: String?, _ failure: String?) -> Void) {
        guard let codex = CodexStore.binary() else {
            DispatchQueue.main.async { started(.failure(AccountError("Couldn't find the codex command. Install the Codex CLI first."))) }
            return
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: codex)
        process.arguments = Self.arguments(prompt: prompt, session: session, folder: folder, pick: pick)
        process.currentDirectoryURL = URL(fileURLWithPath: folder)
        process.environment = Self.environment()
        let out = Pipe(), err = Pipe()
        process.standardOutput = out
        process.standardError = err
        process.standardInput = FileHandle.nullDevice

        var id = session
        var announced = false
        var lastError = ""
        var buffer = Data()
        let announce: (Result<String, Error>) -> Void = { result in
            guard !announced else { return }
            announced = true
            DispatchQueue.main.async { started(result) }
        }
        if let session {
            register(session, process)
        }
        // Output, errors and the exit arrive on different threads; this keeps them in order.
        let serial = DispatchQueue(label: "local.notchling.codex-turn")
        out.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let chunk = handle.availableData
            guard !chunk.isEmpty else { handle.readabilityHandler = nil; return }
            serial.async {
            buffer.append(chunk)
            while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
                let line = buffer[buffer.startIndex..<newline]
                buffer.removeSubrange(buffer.startIndex...newline)
                guard let event = (try? JSONSerialization.jsonObject(with: Data(line))) as? [String: Any] else { continue }
                switch event["type"] as? String {
                case "thread.started":
                    if let thread = event["thread_id"] as? String {
                        if id == nil { id = thread; self?.register(thread, process) }
                        announce(.success(thread))
                    }
                case "error", "turn.failed":
                    let message = (event["message"] as? String) ?? ((event["error"] as? [String: Any])?["message"] as? String)
                    if let message { lastError = message }
                default:
                    break
                }
            }
            }
        }
        err.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            guard !chunk.isEmpty else { handle.readabilityHandler = nil; return }
            let text = String(decoding: chunk, as: UTF8.self)
            serial.async {
                if let line = text.split(whereSeparator: \.isNewline).last(where: { $0.contains("rror") }) { lastError = String(line) }
            }
        }
        process.terminationHandler = { [weak self] p in
            // Let the last output be read first.
            serial.asyncAfter(deadline: .now() + 0.3) {
            if let id { self?.unregister(id, p) }
            // Stopped on purpose (SIGINT, or Codex's own exit code for it) isn't a failure.
            let failed = p.terminationStatus != 0 && p.terminationStatus != 130 && p.terminationReason != .uncaughtSignal
            let message = failed ? (lastError.isEmpty ? "Codex exited with code \(p.terminationStatus)." : lastError) : nil
            announce(id.map { .success($0) } ?? .failure(AccountError(message ?? "Codex didn't start a session.")))
            DispatchQueue.main.async { ended(id, message) }
            }
        }
        do {
            try process.run()
        } catch {
            if let session { unregister(session, process) }
            serial.async { announce(.failure(error)) }
            return
        }
        if let session { serial.async { announce(.success(session)) } }
    }

    /// Stops a running turn the way Ctrl+C would.
    func stop(_ sessionID: String) {
        lock.lock()
        let process = processes[sessionID]
        lock.unlock()
        process?.interrupt()
    }

    private func register(_ id: String, _ process: Process) {
        lock.lock(); processes[id] = process; lock.unlock()
    }

    private func unregister(_ id: String, _ process: Process) {
        lock.lock()
        if processes[id] === process { processes[id] = nil }
        lock.unlock()
    }

    /// The app's environment plus the login shell's PATH, so the tools Codex runs are found the way they
    /// are in a terminal (an app opened from Finder has only the system PATH).
    private static func environment() -> [String: String] {
        var env = ProcessInfo.processInfo.environment
        if let path = loginPath { env["PATH"] = path }
        env.removeValue(forKey: "NO_COLOR")
        return env
    }

    private static let loginPath: String? = {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/zsh")
        p.arguments = ["-ilc", "printf %s \"$PATH\""]
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch { return nil }
        let deadline = Date().addingTimeInterval(5)
        while p.isRunning, Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        if p.isRunning { p.terminate(); return nil }
        let path = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .split(whereSeparator: \.isNewline).last.map(String.init)
        return path?.contains("/usr/bin") == true ? path : nil
    }()
}
