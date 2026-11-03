import Foundation

/// Finds `<sessionId>.jsonl` under ~/.claude/projects/<encoded cwd>/, falling back to scanning every
/// project directory for the file name.
public final class TranscriptLocator {
    private let projectsDir: URL
    private let now: () -> Date
    private let lock = NSLock()
    private var found: [String: URL] = [:]
    private var missedAt: [String: Date] = [:]
    /// Retrying a miss scans every project directory, so not on every tick.
    private let retryInterval: TimeInterval = 5

    public init(projectsDir: URL, now: @escaping () -> Date = Date.init) {
        self.projectsDir = projectsDir
        self.now = now
    }

    /// Claude Code names a project directory after the cwd with every character outside [A-Za-z0-9] as "-".
    public static func encode(cwd: String) -> String {
        let units = cwd.utf16.map { unit -> UInt16 in
            let isAlphanumeric = (0x30...0x39).contains(unit) || (0x41...0x5A).contains(unit) || (0x61...0x7A).contains(unit)
            return isAlphanumeric ? unit : 0x2D
        }
        return String(decoding: units, as: UTF16.self)
    }

    /// The recorded cwd may or may not carry the /private prefix of macOS's /tmp, /var and /etc links.
    static func candidateDirectoryNames(cwd: String) -> [String] {
        var paths = [cwd]
        if cwd.hasPrefix("/private/") {
            paths.append(String(cwd.dropFirst("/private".count)))
        } else if ["/tmp", "/var", "/etc"].contains(where: { cwd == $0 || cwd.hasPrefix($0 + "/") }) {
            paths.append("/private" + cwd)
        }
        return paths.map(encode(cwd:))
    }

    public func locate(sessionId: String, cwd: String, hint: URL? = nil) -> URL? {
        let fileManager = FileManager.default
        if let hint, fileManager.fileExists(atPath: hint.path) { return hint }

        lock.lock()
        if let cached = found[sessionId], fileManager.fileExists(atPath: cached.path) {
            lock.unlock()
            return cached
        }
        if let missed = missedAt[sessionId], now().timeIntervalSince(missed) < retryInterval {
            lock.unlock()
            return nil
        }
        lock.unlock()

        let fileName = "\(sessionId).jsonl"
        var result: URL?
        for name in Self.candidateDirectoryNames(cwd: cwd) {
            let candidate = projectsDir.appendingPathComponent(name).appendingPathComponent(fileName)
            if fileManager.fileExists(atPath: candidate.path) { result = candidate; break }
        }
        if result == nil {
            for directory in JSONFile.contents(of: projectsDir) {
                let candidate = directory.appendingPathComponent(fileName)
                if fileManager.fileExists(atPath: candidate.path) { result = candidate; break }
            }
        }

        lock.lock()
        if let result {
            found[sessionId] = result
            missedAt[sessionId] = nil
        } else {
            missedAt[sessionId] = now()
        }
        lock.unlock()
        return result
    }
}
