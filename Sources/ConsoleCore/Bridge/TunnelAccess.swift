import Foundation

/// "Truy cập qua internet": the Cloudflare tunnel and Access settings, kept in bridge/access.json (0600).
/// The client secret lives only in that file; it is never logged.
public struct AccessConfig: Codable, Equatable, Sendable {
    public static let defaultHostname = "console.trietkun.shop"
    public static let defaultTeamDomain = "bold-dew-100f.cloudflareaccess.com"

    public var enabled = false
    public var hostname = AccessConfig.defaultHostname
    public var teamDomain = AccessConfig.defaultTeamDomain
    public var audience = ""
    public var clientId = ""
    public var clientSecret = ""

    public init() {}

    /// Everything the Mac needs to verify tunnel clients and everything the phone needs to reach it.
    public var isComplete: Bool {
        ![hostname, teamDomain, audience, clientId, clientSecret].contains { $0.trimmingCharacters(in: .whitespaces).isEmpty }
    }

    /// Whether the pairing QR carries a tunnel block.
    public var offersTunnel: Bool { enabled && isComplete }

    var tunnelPayload: [String: Any]? {
        guard offersTunnel else { return nil }
        let host = hostname.trimmingCharacters(in: .whitespaces)
        return ["url": "wss://\(host)", "accessClientId": clientId, "accessClientSecret": clientSecret]
    }
}

public final class AccessConfigStore: @unchecked Sendable {
    private let fileURL: URL
    private let lock = NSLock()
    private var cached: AccessConfig

    public init(fileURL: URL) {
        self.fileURL = fileURL
        cached = Self.load(fileURL)
    }

    public var config: AccessConfig { lock.withLock { cached } }

    public func update(_ change: (inout AccessConfig) -> Void) {
        lock.lock(); defer { lock.unlock() }
        var next = cached
        change(&next)
        guard next != cached else { return }
        cached = next
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys, .prettyPrinted]
            try BridgeFiles.writePrivate(try encoder.encode(next), to: fileURL)
        } catch {
            ConsoleLog.error("AccessConfigStore.update", fileURL.path, "\(error)")
        }
    }

    private static func load(_ url: URL) -> AccessConfig {
        guard FileManager.default.fileExists(atPath: url.path) else { return AccessConfig() }
        do {
            return try JSONDecoder().decode(AccessConfig.self, from: Data(contentsOf: url))
        } catch {
            ConsoleLog.error("AccessConfigStore.load", url.path, "\(error)")
            return AccessConfig()
        }
    }
}

/// Decides at the WebSocket upgrade whether a client came through the Cloudflare tunnel and whether it
/// may connect. Tunnel traffic reaches the Mac from loopback (cloudflared), so the headers are the only
/// way to tell it apart from a local client.
public final class TunnelGate: @unchecked Sendable {
    public enum Decision: Equatable, Sendable {
        /// No Cloudflare headers: LAN or loopback, as before.
        case local
        /// Came through the tunnel with a valid Access token; `clientAddress` is the real client (Cf-Connecting-Ip).
        case tunnel(clientAddress: String)
        case reject(String)
    }

    public static let unknownClient = "cf-unknown"

    private let config: @Sendable () -> AccessConfig
    private let verifier: AccessJWTVerifier
    private let limiter: AuthRateLimiter
    private let now: @Sendable () -> Date

    public init(config: @escaping @Sendable () -> AccessConfig, verifier: AccessJWTVerifier, limiter: AuthRateLimiter,
                now: @escaping @Sendable () -> Date = { Date() }) {
        self.config = config
        self.verifier = verifier
        self.limiter = limiter
        self.now = now
    }

    public static func header(_ name: String, in headers: [(name: String, value: String)]) -> String? {
        headers.first { $0.name.lowercased() == name.lowercased() }?.value.trimmingCharacters(in: .whitespaces)
    }

    public static func isTunnelOrigin(_ headers: [(name: String, value: String)]) -> Bool {
        header("Cf-Connecting-Ip", in: headers) != nil || header("Cf-Ray", in: headers) != nil
    }

    public struct Refusal: Error, Equatable, Sendable {
        public let status: Int
        public let reason: String
    }

    public func evaluate(headers: [(name: String, value: String)]) -> Decision {
        guard Self.isTunnelOrigin(headers) else { return .local }
        switch evaluateTunnel(headers: headers) {
        case .success(let client): return .tunnel(clientAddress: client)
        case .failure(let refusal): return .reject(refusal.reason)
        }
    }

    /// The checks for a request that carries Cloudflare headers, with the HTTP status to answer a refusal with.
    public func evaluateTunnel(headers: [(name: String, value: String)]) -> Result<String, Refusal> {
        let settings = config()
        guard settings.enabled else { return .failure(Refusal(status: 403, reason: "internet access is off")) }
        guard settings.isComplete else { return .failure(Refusal(status: 403, reason: "internet access is not fully configured")) }

        let client = Self.header("Cf-Connecting-Ip", in: headers).flatMap { $0.isEmpty ? nil : $0 } ?? Self.unknownClient
        if case .blocked = limiter.check(client, now: now()) { return .failure(Refusal(status: 429, reason: "\(client) is locked out")) }

        guard let token = Self.header("Cf-Access-Jwt-Assertion", in: headers), !token.isEmpty else {
            return .failure(Refusal(status: 401, reason: "missing Cf-Access-Jwt-Assertion"))
        }
        switch verifier.verify(token, team: settings.teamDomain, audience: settings.audience) {
        case .success: return .success(client)
        case .failure(let reason): return .failure(Refusal(status: 401, reason: "access token refused: \(reason)"))
        }
    }
}

/// The text inside the pairing QR. `v` stays 1 and every v1 field is kept, so an app that predates the
/// tunnel still parses it and just ignores `tunnel`.
public enum PairingPayload {
    public static func make(name: String, port: Int, fingerprint: String, hosts: [String], code: String,
                            access: AccessConfig) -> String? {
        var object: [String: Any] = ["v": 1, "name": name, "port": port, "fp": fingerprint, "hosts": hosts, "code": code]
        if let tunnel = access.tunnelPayload { object["tunnel"] = tunnel }
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) else { return nil }
        return String(data: data, encoding: .utf8)
    }
}
