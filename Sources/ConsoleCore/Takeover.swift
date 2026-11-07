import Foundation

/// Whether a session can be taken over, as the phone sees it (`takeover` in the session JSON).
public enum TakeoverState: Equatable, Sendable {
    case available
    /// The original is still running but is a verified claude process: Console can stop it, then resume.
    /// [busy] means it is in the middle of a turn, which the stop interrupts.
    case stoppable(busy: Bool)
    case unavailable(String)
}

public enum TakeoverError: Error, Equatable, LocalizedError, Sendable {
    /// The original process is still running: two writers on one session would corrupt it.
    case alive
    case noTranscript
    case sdk
    case alreadyControllable
    case noWorkingFolder

    public var errorDescription: String? {
        switch self {
        case .alive: return Takeover.aliveReason
        case .noTranscript: return "Không có transcript tiếp tục được trong ~/.claude/projects, nên không tiếp quản được."
        case .sdk: return "Session chạy bằng claude -p, không tiếp quản được."
        case .alreadyControllable: return "Console đã điều khiển được session này."
        case .noWorkingFolder: return "Thư mục làm việc của session không còn."
        }
    }
}

/// What to run to resume the session in a Console-owned pty.
public struct TakeoverPlan: Equatable, Sendable {
    public let sessionId: String
    public let cwd: URL
    public let transcript: URL
    public let arguments: [String]
}

/// Everything the pty spawner needs; the same shape as "Session mới".
public struct TakeoverSpawn: Equatable, Sendable {
    public let executable: URL
    public let arguments: [String]
    public let cwd: URL
    public let environment: [String: String]
}

public enum Takeover {
    public static let aliveReason = "Đóng session ở nơi đang mở (Claude desktop / VS Code) rồi tiếp quản"

    private static let idPattern = #"^[0-9A-Fa-f]{8}(-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}$"#

    /// ~/.claude/projects/*/<sessionId>.jsonl, the only place `claude --resume` reads from.
    public static func resumableTranscript(sessionId: String, projectsDir: URL) -> URL? {
        guard sessionId.range(of: idPattern, options: .regularExpression) != nil else { return nil }
        let folders = (try? FileManager.default.contentsOfDirectory(at: projectsDir, includingPropertiesForKeys: nil)) ?? []
        for folder in folders {
            let candidate = folder.appendingPathComponent("\(sessionId).jsonl")
            if FileManager.default.fileExists(atPath: candidate.path) { return candidate }
        }
        return nil
    }

    /// True when any session file for this id belongs to a process that is still running.
    static func hasLiveSessionFile(sessionId: String, sessionsDir: URL, isAlive: (Int32) -> Bool) -> Bool {
        for url in JSONFile.contents(of: sessionsDir) where url.pathExtension == "json" {
            guard let object = JSONFile.object(at: url), object["sessionId"] as? String == sessionId,
                  let pid = (object["pid"] as? NSNumber)?.int32Value else { continue }
            if isAlive(pid) { return true }
        }
        return false
    }

    /// The checks, in the order that matters: a running original is `.alive` before anything else.
    public static func evaluate(_ session: Session, paths: ClaudePaths = .live,
                                assumeStopped: Bool = false,
                                isAlive: (Int32) -> Bool = isProcessAlive,
                                directoryExists: (String) -> Bool = { path in
                                    var isDirectory: ObjCBool = false
                                    return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory) && isDirectory.boolValue
                                }) -> Result<TakeoverPlan, TakeoverError> {
        switch session.control {
        case .consolePTY, .job: return .failure(.alreadyControllable)
        case .viewOnly: break
        }
        if session.entrypoint == .sdk { return .failure(.sdk) }
        // `assumeStopped`: the original was just stopped here, and the session list has not caught up with it yet.
        if !assumeStopped, session.state == .running || session.state == .needsInput { return .failure(.alive) }
        if let pid = session.pid, isAlive(pid) { return .failure(.alive) }
        if hasLiveSessionFile(sessionId: session.id, sessionsDir: paths.sessionsDir, isAlive: isAlive) { return .failure(.alive) }
        guard !session.cwd.isEmpty, session.cwd.hasPrefix("/"), directoryExists(session.cwd) else { return .failure(.noWorkingFolder) }
        guard let transcript = resumableTranscript(sessionId: session.id, projectsDir: paths.projectsDir) else {
            return .failure(.noTranscript)
        }
        return .success(TakeoverPlan(sessionId: session.id, cwd: URL(fileURLWithPath: session.cwd), transcript: transcript,
                                     arguments: ["--resume", session.id]))
    }

    public static func state(for session: Session, paths: ClaudePaths = .live, isAlive: (Int32) -> Bool = isProcessAlive,
                             control: ProcessControl = LiveProcessControl()) -> TakeoverState? {
        guard session.control == .viewOnly else { return nil }
        switch evaluate(session, paths: paths, isAlive: isAlive) {
        case .success: return .available
        case .failure(.alive):
            // Still running: stoppable when everything else needed to resume is there and the process is verified.
            var stopped = session
            stopped.state = .exited
            let rest = evaluate(stopped, paths: paths, assumeStopped: true, isAlive: { _ in false })
            guard case .success = rest,
                  case .success(let pids) = OriginalStopper.liveProcesses(sessionId: session.id, knownPid: session.pid,
                                                                          sessionsDir: paths.sessionsDir, control: control),
                  !pids.isEmpty else { return .unavailable(TakeoverError.alive.localizedDescription) }
            return .stoppable(busy: session.state == .running || session.state == .needsInput)
        case .failure(let error): return .unavailable(error.localizedDescription)
        }
    }

    /// The spawn request, with the same environment scrubbing as a new session (no CLAUDE_CODE_* markers).
    public static func spawn(for plan: TakeoverPlan, executable: URL, environment: [String: String]) -> TakeoverSpawn {
        TakeoverSpawn(executable: executable, arguments: plan.arguments, cwd: plan.cwd, environment: SpawnEnvironment.scrubbed(environment))
    }

    public enum Resolution: Equatable, Sendable {
        case sameSession
        /// The resumed process wrote a new session id; the old row must not show next to it.
        case replaced(old: String, new: String)
    }

    public static func resolution(original: String, resumed: String) -> Resolution {
        original == resumed ? .sameSession : .replaced(old: original, new: resumed)
    }
}
