import Foundation

/// What the host remembers about a session it owns, so it can bring it back after the host itself was stopped
/// (a reboot, a crash, an update): the Claude session id to `--resume` and where it ran.
public struct HostRecord: Codable, Equatable, Sendable {
    public var id: String
    public var cwd: String
    public var model: String?
    /// Known once Claude has written its session file; without it a dead session cannot be resumed.
    public var claudeSessionId: String?
    /// The pid of the pty's process when recorded; 0 for a record written by the migration script.
    public var pid: Int32
    public var startedAt: Date

    public init(id: String, cwd: String, model: String?, claudeSessionId: String?, pid: Int32, startedAt: Date) {
        self.id = id; self.cwd = cwd; self.model = model; self.claudeSessionId = claudeSessionId; self.pid = pid; self.startedAt = startedAt
    }
}

public struct HostSettings: Codable, Equatable, Sendable {
    /// Resume recorded sessions whose process is gone when the host starts. On by default.
    public var resumeOnStart = true
    public init() {}
}

struct HostStateFile: Codable {
    var records: [HostRecord] = []
    var settings = HostSettings()
}

/// hostd-state.json, 0600, written whole after each change.
public final class HostStateStore: @unchecked Sendable {
    private let url: URL
    private let lock = NSLock()
    private var file: HostStateFile

    public init(url: URL) {
        self.url = url
        if let data = try? Data(contentsOf: url) {
            let decoder = JSONDecoder()
            decoder.dateDecodingStrategy = .iso8601
            do { file = try decoder.decode(HostStateFile.self, from: data) } catch {
                ConsoleLog.error("HostStateStore.init", url.path, "unreadable, starting empty: \(error)")
                file = HostStateFile()
            }
        } else {
            file = HostStateFile()
        }
    }

    public var records: [HostRecord] { lock.withLock { file.records } }
    public var settings: HostSettings { lock.withLock { file.settings } }

    public func upsert(_ record: HostRecord) {
        mutate { if let index = $0.records.firstIndex(where: { $0.id == record.id }) { $0.records[index] = record } else { $0.records.append(record) } }
    }

    public func remove(id: String) { mutate { $0.records.removeAll { $0.id == id } } }

    public func setSettings(_ settings: HostSettings) { mutate { $0.settings = settings } }

    private func mutate(_ change: (inout HostStateFile) -> Void) {
        lock.lock(); defer { lock.unlock() }
        change(&file)
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys, .prettyPrinted]
        do {
            let directory = url.deletingLastPathComponent()
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            let temporary = directory.appendingPathComponent(".\(url.lastPathComponent).tmp")
            guard FileManager.default.createFile(atPath: temporary.path, contents: try encoder.encode(file), attributes: [.posixPermissions: 0o600]) else {
                throw CocoaError(.fileWriteUnknown)
            }
            if rename(temporary.path, url.path) != 0 { throw CocoaError(.fileWriteUnknown) }
        } catch {
            ConsoleLog.error("HostStateStore.mutate", url.path, "\(error)")
        }
    }
}

/// What to do about the host binary when a new one has been installed next to the running one.
public enum HostdUpdatePolicy {
    public enum Action: Equatable, Sendable {
        case nothing
        /// Restart the host now: nothing is running, so nothing is lost.
        case restartNow
        /// Leave the old host running; the new binary takes over at the next idle moment.
        case deferUntilIdle
    }

    public static func decide(binaryChanged: Bool, runningSessions: Int) -> Action {
        guard binaryChanged else { return .nothing }
        return runningSessions == 0 ? .restartNow : .deferUntilIdle
    }
}
