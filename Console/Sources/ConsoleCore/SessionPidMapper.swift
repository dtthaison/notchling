import Foundation

/// Finds the Claude session a Console-owned pty child became, from ~/.claude/sessions/<pid>.json.
public enum SessionPidMapper {
    private struct Entry: Decodable {
        let pid: Int32
        let sessionId: String
    }

    /// The session whose process is `rootPid` or one of its descendants.
    public static func sessionId(forRootPid rootPid: Int32, sessionsDir: URL,
                                 parentOf: (Int32) -> Int32? = parentProcessID(of:)) -> String? {
        for url in JSONFile.contents(of: sessionsDir) where url.pathExtension == "json" {
            guard let entry = JSONFile.decode(Entry.self, at: url) else { continue }
            if OwnedSessions.descends(entry.pid, from: rootPid, parentOf: parentOf) { return entry.sessionId }
        }
        return nil
    }
}
