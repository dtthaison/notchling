import Foundation

/// Per-address backoff for failed logins: 1, 2, 4, 8 seconds, then a 5-minute lock on the 5th failure.
public final class AuthRateLimiter: @unchecked Sendable {
    public enum Verdict: Equatable, Sendable {
        case allowed
        case blocked(until: Date)
    }

    public static let maxFailures = 5
    public static let lockDuration: TimeInterval = 5 * 60

    private struct Entry { var failures = 0; var blockedUntil: Date? }
    private let lock = NSLock()
    private var entries: [String: Entry] = [:]

    public init() {}

    public func check(_ address: String, now: Date = Date()) -> Verdict {
        lock.lock(); defer { lock.unlock() }
        if let until = entries[address]?.blockedUntil, until > now { return .blocked(until: until) }
        return .allowed
    }

    @discardableResult
    public func recordFailure(_ address: String, now: Date = Date()) -> Verdict {
        lock.lock(); defer { lock.unlock() }
        var entry = entries[address] ?? Entry()
        // A lock that has run out starts counting from zero again.
        if let until = entry.blockedUntil, until <= now, entry.failures >= Self.maxFailures { entry = Entry() }
        entry.failures += 1
        let delay = entry.failures >= Self.maxFailures ? Self.lockDuration : pow(2, Double(entry.failures - 1))
        entry.blockedUntil = now.addingTimeInterval(delay)
        entries[address] = entry
        return .blocked(until: entry.blockedUntil!)
    }

    public func recordSuccess(_ address: String) {
        lock.lock(); entries[address] = nil; lock.unlock()
    }
}
