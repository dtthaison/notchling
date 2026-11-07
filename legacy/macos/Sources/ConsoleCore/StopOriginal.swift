import Darwin
import Foundation

/// What Console may do to another process, behind a protocol so tests can stand in for real ones.
public protocol ProcessControl: Sendable {
    func isAlive(_ pid: Int32) -> Bool
    /// The path of the running executable (proc_pidpath), nil when the process is gone or unreadable.
    func executablePath(of pid: Int32) -> String?
    /// SIGTERM to this one process. Never SIGKILL, never a process group.
    func terminate(_ pid: Int32) -> Bool
}

public struct LiveProcessControl: ProcessControl {
    public init() {}

    public func isAlive(_ pid: Int32) -> Bool { isProcessAlive(pid) }

    public func executablePath(of pid: Int32) -> String? {
        var buffer = [CChar](repeating: 0, count: 4096)
        let length = proc_pidpath(pid, &buffer, UInt32(buffer.count))
        return length > 0 ? String(cString: buffer) : nil
    }

    public func terminate(_ pid: Int32) -> Bool { pid > 1 && kill(pid, SIGTERM) == 0 }
}

public enum OriginalStopError: Error, Equatable, LocalizedError, Sendable {
    /// The process behind the session file is not Claude (a reused pid, say). Nothing is signalled.
    case notClaude(pid: Int32)
    /// The session file for that pid names another session. Nothing is signalled.
    case sessionMismatch(pid: Int32)
    /// SIGTERM was sent and the process is still there after the wait. No SIGKILL follows.
    case stillRunning(pid: Int32)

    public var errorDescription: String? {
        switch self {
        case .notClaude: return "Tiến trình giữ session này không phải claude, Console không dừng nó."
        case .sessionMismatch: return "File session của tiến trình đó ghi session khác, Console không dừng nó."
        case .stillRunning: return "Mac chưa dừng được session"
        }
    }
}

/// Stops the process that still holds a session opened elsewhere (Claude desktop, VS Code, a terminal), so Console
/// can resume it without two writers on one session. It signals only the verified claude process of that session.
public enum OriginalStopper {
    /// Claude's own binary: `claude` itself, or the versioned binary the CLI installer links to.
    public static func isClaudeExecutable(_ path: String) -> Bool {
        let name = (path as NSString).lastPathComponent
        return name == "claude" || path.contains("/claude/versions/")
    }

    private struct SessionFile { let pid: Int32; let sessionId: String }

    private static func readFile(pid: Int32, sessionsDir: URL) -> SessionFile? {
        let url = sessionsDir.appendingPathComponent("\(pid).json")
        guard let object = JSONFile.object(at: url), let id = object["sessionId"] as? String else { return nil }
        return SessionFile(pid: pid, sessionId: id)
    }

    /// The live, verified claude processes of the session: from its own pid and from any session file naming it.
    /// Empty when nothing is running (nothing to stop). A live process that is not claude, or whose file names
    /// another session, is an error rather than a candidate.
    public static func liveProcesses(sessionId: String, knownPid: Int32?, sessionsDir: URL, control: ProcessControl) -> Result<[Int32], OriginalStopError> {
        var pids = Set<Int32>()
        if let knownPid { pids.insert(knownPid) }
        for url in JSONFile.contents(of: sessionsDir) where url.pathExtension == "json" {
            guard let object = JSONFile.object(at: url), object["sessionId"] as? String == sessionId,
                  let pid = (object["pid"] as? NSNumber)?.int32Value else { continue }
            pids.insert(pid)
        }
        var verified: [Int32] = []
        for pid in pids.sorted() where control.isAlive(pid) {
            guard let file = readFile(pid: pid, sessionsDir: sessionsDir), file.sessionId == sessionId else {
                return .failure(.sessionMismatch(pid: pid))
            }
            guard let path = control.executablePath(of: pid), isClaudeExecutable(path) else { return .failure(.notClaude(pid: pid)) }
            verified.append(pid)
        }
        return .success(verified)
    }

    /// SIGTERM each verified process, then wait up to [timeout] for it to go. Returns the pids that were stopped
    /// (empty when none was running).
    public static func stop(sessionId: String, knownPid: Int32?, sessionsDir: URL, control: ProcessControl,
                            timeout: TimeInterval = 5, poll: TimeInterval = 0.1,
                            sleep: @Sendable (TimeInterval) async -> Void = { try? await Task.sleep(nanoseconds: UInt64($0 * 1_000_000_000)) }) async -> Result<[Int32], OriginalStopError> {
        let found = liveProcesses(sessionId: sessionId, knownPid: knownPid, sessionsDir: sessionsDir, control: control)
        guard case .success(let pids) = found else { return found }
        for pid in pids { _ = control.terminate(pid) }
        var waited: TimeInterval = 0
        while waited < timeout {
            if pids.allSatisfy({ !control.isAlive($0) }) { return .success(pids) }
            await sleep(poll)
            waited += poll
        }
        if let alive = pids.first(where: { control.isAlive($0) }) { return .failure(.stillRunning(pid: alive)) }
        return .success(pids)
    }
}
