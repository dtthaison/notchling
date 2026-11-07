import Foundation
import Security

public enum AccessJWTError: Error, Equatable, Sendable {
    case malformed
    case unsupportedAlgorithm
    case unknownKey
    case badSignature
    case wrongAudience
    case wrongIssuer
    case expired
    case notYetValid
    case issuedInFuture
}

public struct AccessClaims: Equatable, Sendable {
    public let audiences: [String]
    public let issuer: String
    public let expiresAt: Date
}

enum Base64URL {
    static func decode(_ text: String) -> Data? {
        var value = text.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while value.count % 4 != 0 { value.append("=") }
        return Data(base64Encoded: value)
    }
}

/// The signing keys of one Cloudflare Access team (`https://<team>/cdn-cgi/access/certs`).
/// Keys are cached; a token with a `kid` that is not cached triggers one refetch, but never more often
/// than `minimumRefetchInterval`, so a client sending random `kid`s cannot make the Mac hammer Cloudflare.
public final class AccessKeySet: @unchecked Sendable {
    public typealias Fetch = @Sendable (URL) throws -> Data

    public static let maximumAge: TimeInterval = 60 * 60
    public static let minimumRefetchInterval: TimeInterval = 30

    private struct Snapshot { var keys: [String: SecKey]; var fetchedAt: Date }
    private let fetch: Fetch
    private let now: @Sendable () -> Date
    private let lock = NSLock()
    private var snapshots: [String: Snapshot] = [:]
    private var lastAttempt: [String: Date] = [:]
    public private(set) var fetchCount = 0

    public init(fetch: @escaping Fetch, now: @escaping @Sendable () -> Date = { Date() }) {
        self.fetch = fetch
        self.now = now
    }

    public static func certsURL(team: String) -> URL? { URL(string: "https://\(team)/cdn-cgi/access/certs") }

    public func key(kid: String, team: String) -> SecKey? {
        let current = now()
        lock.lock()
        let snapshot = snapshots[team]
        let hit = snapshot?.keys[kid]
        let stale = snapshot.map { current.timeIntervalSince($0.fetchedAt) >= Self.maximumAge } ?? true
        let attempted = lastAttempt[team]
        lock.unlock()
        if let hit, !stale { return hit }

        if let attempted, current.timeIntervalSince(attempted) < Self.minimumRefetchInterval {
            return stale ? nil : hit
        }
        lock.lock(); lastAttempt[team] = current; fetchCount += 1; lock.unlock()
        guard let url = Self.certsURL(team: team) else { return nil }
        do {
            let keys = try Self.parse(try fetch(url))
            lock.lock(); snapshots[team] = Snapshot(keys: keys, fetchedAt: current); lock.unlock()
            return keys[kid]
        } catch {
            ConsoleLog.error("AccessKeySet.key", url.absoluteString, "\(error)")
            return stale ? nil : hit
        }
    }

    static func parse(_ data: Data) throws -> [String: SecKey] {
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let list = object["keys"] as? [[String: Any]] else { throw AccessJWTError.malformed }
        var keys: [String: SecKey] = [:]
        for entry in list {
            guard entry["kty"] as? String == "RSA", let kid = entry["kid"] as? String,
                  let n = (entry["n"] as? String).flatMap(Base64URL.decode),
                  let e = (entry["e"] as? String).flatMap(Base64URL.decode) else { continue }
            let attributes: [String: Any] = [kSecAttrKeyType as String: kSecAttrKeyTypeRSA,
                                             kSecAttrKeyClass as String: kSecAttrKeyClassPublic]
            if let key = SecKeyCreateWithData(rsaPublicKeyDER(modulus: n, exponent: e) as CFData, attributes as CFDictionary, nil) {
                keys[kid] = key
            }
        }
        return keys
    }

    /// PKCS#1 RSAPublicKey: SEQUENCE { INTEGER n, INTEGER e }.
    static func rsaPublicKeyDER(modulus: Data, exponent: Data) -> Data {
        func length(_ count: Int) -> [UInt8] {
            if count < 128 { return [UInt8(count)] }
            if count < 256 { return [0x81, UInt8(count)] }
            return [0x82, UInt8(count >> 8), UInt8(count & 0xFF)]
        }
        func integer(_ raw: Data) -> [UInt8] {
            var bytes = Array(raw.drop(while: { $0 == 0 }))
            if bytes.isEmpty { bytes = [0] }
            if bytes[0] & 0x80 != 0 { bytes.insert(0, at: 0) }
            return [0x02] + length(bytes.count) + bytes
        }
        let body = integer(modulus) + integer(exponent)
        return Data([0x30] + length(body.count) + body)
    }

    /// The live fetch: a blocking GET with a 5 s limit. Called from the handshake queue, never the main thread.
    public static let liveFetch: Fetch = { url in
        var request = URLRequest(url: url)
        request.timeoutInterval = 5
        let semaphore = DispatchSemaphore(value: 0)
        let box = FetchBox()
        URLSession.shared.dataTask(with: request) { data, response, error in
            if let error { box.result = .failure(error) }
            else if let data, (response as? HTTPURLResponse)?.statusCode == 200 { box.result = .success(data) }
            else { box.result = .failure(URLError(.badServerResponse)) }
            semaphore.signal()
        }.resume()
        if semaphore.wait(timeout: .now() + 7) == .timedOut { throw URLError(.timedOut) }
        return try box.result.get()
    }

    private final class FetchBox: @unchecked Sendable {
        var result: Result<Data, Error> = .failure(URLError(.unknown))
    }
}

/// Verifies the `Cf-Access-Jwt-Assertion` header: RS256 signature against the team's keys, then audience,
/// issuer and time claims (60 s of clock skew allowed).
public struct AccessJWTVerifier: Sendable {
    public static let skew: TimeInterval = 60

    private let keys: AccessKeySet
    private let now: @Sendable () -> Date

    public init(keys: AccessKeySet, now: @escaping @Sendable () -> Date = { Date() }) {
        self.keys = keys
        self.now = now
    }

    public func verify(_ token: String, team: String, audience: String) -> Result<AccessClaims, AccessJWTError> {
        let parts = token.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 3,
              let headerData = Base64URL.decode(String(parts[0])),
              let payloadData = Base64URL.decode(String(parts[1])),
              let signature = Base64URL.decode(String(parts[2])),
              let header = (try? JSONSerialization.jsonObject(with: headerData)) as? [String: Any],
              let payload = (try? JSONSerialization.jsonObject(with: payloadData)) as? [String: Any]
        else { return .failure(.malformed) }

        guard header["alg"] as? String == "RS256" else { return .failure(.unsupportedAlgorithm) }
        guard let kid = header["kid"] as? String, let key = keys.key(kid: kid, team: team) else { return .failure(.unknownKey) }

        let signed = Data("\(parts[0]).\(parts[1])".utf8)
        guard SecKeyVerifySignature(key, .rsaSignatureMessagePKCS1v15SHA256, signed as CFData, signature as CFData, nil) else {
            return .failure(.badSignature)
        }

        let audiences: [String]
        if let list = payload["aud"] as? [String] { audiences = list }
        else if let single = payload["aud"] as? String { audiences = [single] }
        else { audiences = [] }
        guard !audience.isEmpty, audiences.contains(audience) else { return .failure(.wrongAudience) }

        let issuer = payload["iss"] as? String ?? ""
        guard issuer == "https://\(team)" else { return .failure(.wrongIssuer) }

        guard let exp = (payload["exp"] as? NSNumber)?.doubleValue else { return .failure(.malformed) }
        let current = now()
        if current.timeIntervalSince1970 > exp + Self.skew { return .failure(.expired) }
        if let nbf = (payload["nbf"] as? NSNumber)?.doubleValue, current.timeIntervalSince1970 < nbf - Self.skew { return .failure(.notYetValid) }
        if let iat = (payload["iat"] as? NSNumber)?.doubleValue, current.timeIntervalSince1970 < iat - Self.skew { return .failure(.issuedInFuture) }
        return .success(AccessClaims(audiences: audiences, issuer: issuer, expiresAt: Date(timeIntervalSince1970: exp)))
    }
}
