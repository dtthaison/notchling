import Foundation

public struct PairedDevice: Codable, Equatable, Identifiable, Sendable {
    public let id: String
    public var name: String
    public var platform: String
    /// SHA-256 of the device token, hex. The token itself is never stored.
    public let tokenHash: String
    public let pairedAt: Date
    public var lastSeen: Date?
}

/// Paired phones, in bridge/devices.json (0600). Only token hashes are kept.
public final class DeviceStore: @unchecked Sendable {
    private let fileURL: URL
    private let now: () -> Date
    private let lock = NSLock()
    private var storage: [PairedDevice]
    private var lastTouchWrite: [String: Date] = [:]

    public init(fileURL: URL, now: @escaping () -> Date = Date.init) {
        self.fileURL = fileURL
        self.now = now
        storage = Self.load(fileURL)
    }

    public var devices: [PairedDevice] {
        lock.lock(); defer { lock.unlock() }
        return storage
    }

    /// Creates the device and returns its token. This is the only time the token exists in the clear.
    public func register(name: String, platform: String) throws -> (device: PairedDevice, token: String) {
        let token = BridgeCrypto.randomToken()
        let device = PairedDevice(id: UUID().uuidString, name: String(name.prefix(80)), platform: String(platform.prefix(40)),
                                  tokenHash: BridgeCrypto.sha256Hex(token), pairedAt: now(), lastSeen: nil)
        lock.lock(); defer { lock.unlock() }
        storage.append(device)
        try save()
        return (device, token)
    }

    /// Checks every stored hash, so the time taken does not depend on which device matched.
    public func authenticate(token: String) -> PairedDevice? {
        let candidate = Array(BridgeCrypto.sha256(Data(token.utf8)))
        lock.lock(); defer { lock.unlock() }
        var match: PairedDevice?
        for device in storage {
            let stored = hexBytes(device.tokenHash)
            if BridgeCrypto.constantTimeEqual(candidate, stored) { match = device }
        }
        return match
    }

    @discardableResult
    public func revoke(id: String) throws -> PairedDevice? {
        lock.lock(); defer { lock.unlock() }
        guard let index = storage.firstIndex(where: { $0.id == id }) else { return nil }
        let removed = storage.remove(at: index)
        try save()
        return removed
    }

    /// Updates lastSeen in memory always, and on disk at most once a minute per device.
    public func touch(id: String) {
        lock.lock(); defer { lock.unlock() }
        guard let index = storage.firstIndex(where: { $0.id == id }) else { return }
        let current = now()
        storage[index].lastSeen = current
        if let last = lastTouchWrite[id], current.timeIntervalSince(last) < 60 { return }
        lastTouchWrite[id] = current
        do { try save() } catch { ConsoleLog.error("DeviceStore.touch", fileURL.path, "\(error)") }
    }

    private func hexBytes(_ hex: String) -> [UInt8] {
        var bytes: [UInt8] = []
        var index = hex.startIndex
        while index < hex.endIndex, let next = hex.index(index, offsetBy: 2, limitedBy: hex.endIndex) {
            bytes.append(UInt8(hex[index..<next], radix: 16) ?? 0)
            index = next
        }
        return bytes
    }

    private func save() throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        try BridgeFiles.writePrivate(try encoder.encode(storage), to: fileURL)
    }

    private static func load(_ url: URL) -> [PairedDevice] {
        guard FileManager.default.fileExists(atPath: url.path) else { return [] }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        do {
            return try decoder.decode([PairedDevice].self, from: Data(contentsOf: url))
        } catch {
            ConsoleLog.error("DeviceStore.load", url.path, "\(error)")
            return []
        }
    }
}

enum BridgeFiles {
    /// Writes to a 0600 file inside a 0700 folder.
    static func writePrivate(_ data: Data, to url: URL) throws {
        let directory = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: 0o700])
        let temporary = directory.appendingPathComponent(".\(url.lastPathComponent).tmp")
        guard FileManager.default.createFile(atPath: temporary.path, contents: data, attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
        if rename(temporary.path, url.path) != 0 { throw CocoaError(.fileWriteUnknown) }
    }
}
