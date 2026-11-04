import Foundation

public struct PairingCode: Equatable, Sendable {
    public let code: String
    public let expiresAt: Date
}

/// One live pairing code at a time: 8 characters, valid 5 minutes, used once.
public final class PairingCodes: @unchecked Sendable {
    public static let lifetime: TimeInterval = 5 * 60
    // No 0/O or 1/I/L, which look alike on a phone screen.
    private static let alphabet = Array("ABCDEFGHJKMNPQRSTUVWXYZ23456789")

    private let lock = NSLock()
    private var current: PairingCode?

    public init() {}

    public func issue(now: Date = Date()) -> PairingCode {
        let bytes = BridgeCrypto.randomBytes(8)
        let code = String(bytes.map { Self.alphabet[Int($0) % Self.alphabet.count] })
        let issued = PairingCode(code: code, expiresAt: now.addingTimeInterval(Self.lifetime))
        lock.lock(); current = issued; lock.unlock()
        return issued
    }

    /// True once, for the live code, before it expires. A wrong guess leaves the live code alone.
    public func consume(_ candidate: String, now: Date = Date()) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard let live = current, now < live.expiresAt else {
            current = nil
            return false
        }
        let normalized = candidate.trimmingCharacters(in: .whitespaces).uppercased()
        guard BridgeCrypto.constantTimeEqual(normalized, live.code) else { return false }
        current = nil
        return true
    }

    public func live(now: Date = Date()) -> PairingCode? {
        lock.lock(); defer { lock.unlock() }
        if let live = current, now < live.expiresAt { return live }
        return nil
    }

    public func cancel() {
        lock.lock(); current = nil; lock.unlock()
    }
}
