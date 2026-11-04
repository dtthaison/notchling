import CryptoKit
import Foundation
import os

public enum ConsoleLog {
    public static let logger = Logger(subsystem: "local.agentconsole", category: "core")

    public static func error(_ function: String, _ path: String, _ message: String) {
        logger.error("\(function, privacy: .public) \(path, privacy: .public): \(message, privacy: .public)")
    }
}

/// Where Console reads. Injectable so tests point it at fixtures.
public struct ClaudePaths: Sendable {
    public var claudeDir: URL
    /// Notchling's hook requests (read-only).
    public var requestsDir: URL

    public init(claudeDir: URL, requestsDir: URL) {
        self.claudeDir = claudeDir
        self.requestsDir = requestsDir
    }

    /// Where Notchling's hook looks for decisions, next to `requests/`.
    public var responsesDir: URL { requestsDir.deletingLastPathComponent().appendingPathComponent("responses") }
    public var sessionsDir: URL { claudeDir.appendingPathComponent("sessions") }
    public var jobsDir: URL { claudeDir.appendingPathComponent("jobs") }
    public var projectsDir: URL { claudeDir.appendingPathComponent("projects") }

    public static var live: ClaudePaths {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return ClaudePaths(
            claudeDir: home.appendingPathComponent(".claude"),
            requestsDir: home.appendingPathComponent("Library/Application Support/Notchling/requests"))
    }
}

enum JSONFile {
    private static let decoder = JSONDecoder()

    /// Files vanish between listing and reading all the time (jobs finish, hooks answer), so a
    /// missing file logs at debug; anything else is an error. Nil either way.
    private static func read(_ url: URL, function: String) -> Data? {
        do {
            return try Data(contentsOf: url)
        } catch let error as CocoaError where error.code == .fileReadNoSuchFile || error.code == .fileNoSuchFile {
            ConsoleLog.logger.debug("\(function, privacy: .public) \(url.path, privacy: .public): vanished")
            return nil
        } catch {
            ConsoleLog.error(function, url.path, "read failed: \(error)")
            return nil
        }
    }

    static func decode<T: Decodable>(_ type: T.Type, at url: URL, function: String = #function) -> T? {
        guard let data = read(url, function: function) else { return nil }
        do {
            return try decoder.decode(type, from: data)
        } catch {
            ConsoleLog.error(function, url.path, "decode failed: \(error)")
            return nil
        }
    }

    static func object(at url: URL, function: String = #function) -> [String: Any]? {
        guard let data = read(url, function: function) else { return nil }
        do {
            guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                ConsoleLog.error(function, url.path, "top level is not an object")
                return nil
            }
            return object
        } catch {
            ConsoleLog.error(function, url.path, "decode failed: \(error)")
            return nil
        }
    }

    static func contents(of directory: URL, function: String = #function) -> [URL] {
        do {
            return try FileManager.default.contentsOfDirectory(
                at: directory, includingPropertiesForKeys: nil, options: [])
        } catch let error as CocoaError where error.code == .fileReadNoSuchFile || error.code == .fileNoSuchFile {
            return []
        } catch {
            ConsoleLog.error(function, directory.path, "list failed: \(error)")
            return []
        }
    }
}

func modificationDate(of url: URL) -> Date? {
    (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
}

public func isProcessAlive(_ pid: Int32) -> Bool {
    kill(pid, 0) == 0 || errno == EPERM
}

func dateFromMilliseconds(_ ms: Double?) -> Date? {
    ms.map { Date(timeIntervalSince1970: $0 / 1000) }
}

/// Same input, same UUID, so a re-read transcript keeps SwiftUI row identity.
func stableUUID(_ seed: String) -> UUID {
    let bytes = Array(SHA256.hash(data: Data(seed.utf8)).prefix(16))
    return UUID(uuid: (bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                       bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]))
}
