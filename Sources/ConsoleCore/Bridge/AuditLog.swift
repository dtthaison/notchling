import Foundation

public struct AuditEntry: Codable, Equatable, Sendable {
    public let time: Date
    public let device: String
    public let op: String
    public let session: String?
    /// Length of the text that was sent, never the text itself.
    public let length: Int?
    /// Where the command came from; for the internet route this is the client's own address.
    public let address: String?
    /// For a takeover that stopped the original session: its pid.
    public let stoppedPid: Int?
}

/// Every remote op that changes something, one JSON line each, in bridge/remote-log.jsonl.
public final class AuditLog: @unchecked Sendable {
    private let fileURL: URL
    private let now: () -> Date
    private let lock = NSLock()

    public init(fileURL: URL, now: @escaping () -> Date = Date.init) {
        self.fileURL = fileURL
        self.now = now
    }

    public func append(device: String, op: String, session: String? = nil, length: Int? = nil, address: String? = nil, stoppedPid: Int? = nil) {
        let entry = AuditEntry(time: now(), device: device, op: op, session: session, length: length, address: address, stoppedPid: stoppedPid)
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys]
        lock.lock(); defer { lock.unlock() }
        do {
            let line = try encoder.encode(entry) + Data("\n".utf8)
            var existing = (try? Data(contentsOf: fileURL)) ?? Data()
            existing.append(line)
            try BridgeFiles.writePrivate(existing, to: fileURL)
        } catch {
            ConsoleLog.error("AuditLog.append", fileURL.path, "\(error)")
        }
    }

    public func recent(limit: Int = 50) -> [AuditEntry] {
        lock.lock(); defer { lock.unlock() }
        guard let data = try? Data(contentsOf: fileURL), let text = String(data: data, encoding: .utf8) else { return [] }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return text.split(separator: "\n").suffix(limit).compactMap { line in
            do { return try decoder.decode(AuditEntry.self, from: Data(line.utf8)) } catch {
                ConsoleLog.error("AuditLog.recent", fileURL.path, "bad line skipped: \(error)")
                return nil
            }
        }.reversed()
    }
}
