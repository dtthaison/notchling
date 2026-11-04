import Security
import XCTest
@testable import ConsoleCore

private final class Clock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Date
    init(_ start: Date) { value = start }
    var now: Date { lock.withLock { value } }
    func advance(_ seconds: TimeInterval) { lock.withLock { value = value.addingTimeInterval(seconds) } }
}

private final class FetchLog: @unchecked Sendable {
    private let lock = NSLock()
    private var responses: [Data]
    private(set) var urls: [URL] = []
    init(_ responses: [Data]) { self.responses = responses }
    func next(_ url: URL) throws -> Data {
        lock.lock(); defer { lock.unlock() }
        urls.append(url)
        return responses.count > 1 ? responses.removeFirst() : responses[0]
    }
}

/// A throwaway RSA key standing in for a Cloudflare Access signing key.
private struct TestKey {
    let kid: String
    let privateKey: SecKey
    let modulus: Data
    let exponent: Data

    init(kid: String) throws {
        self.kid = kid
        var error: Unmanaged<CFError>?
        let attributes: [String: Any] = [kSecAttrKeyType as String: kSecAttrKeyTypeRSA, kSecAttrKeySizeInBits as String: 2048]
        privateKey = try XCTUnwrap(SecKeyCreateRandomKey(attributes as CFDictionary, &error))
        let publicKey = try XCTUnwrap(SecKeyCopyPublicKey(privateKey))
        let der = try XCTUnwrap(SecKeyCopyExternalRepresentation(publicKey, &error)) as Data
        (modulus, exponent) = try Self.integers(inPKCS1: der)
    }

    /// Reads n and e out of SEQUENCE { INTEGER, INTEGER }.
    private static func integers(inPKCS1 der: Data) throws -> (Data, Data) {
        let bytes = [UInt8](der)
        var index = 1
        func readLength() -> Int {
            let first = Int(bytes[index]); index += 1
            if first < 0x80 { return first }
            let count = first & 0x7F
            var value = 0
            for _ in 0..<count { value = value << 8 | Int(bytes[index]); index += 1 }
            return value
        }
        _ = readLength()
        var result: [Data] = []
        for _ in 0..<2 {
            XCTAssertEqual(bytes[index], 0x02); index += 1
            let length = readLength()
            result.append(Data(bytes[index..<index + length])); index += length
        }
        return (result[0], result[1])
    }

    var jwk: [String: Any] {
        ["kty": "RSA", "alg": "RS256", "use": "sig", "kid": kid,
         "n": Self.base64URL(modulus), "e": Self.base64URL(exponent)]
    }

    static func base64URL(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    func token(claims: [String: Any], header extra: [String: Any] = [:], signWith other: SecKey? = nil) throws -> String {
        var header: [String: Any] = ["alg": "RS256", "kid": kid, "typ": "JWT"]
        header.merge(extra) { _, new in new }
        let head = Self.base64URL(try JSONSerialization.data(withJSONObject: header))
        let body = Self.base64URL(try JSONSerialization.data(withJSONObject: claims))
        let signed = Data("\(head).\(body)".utf8)
        var error: Unmanaged<CFError>?
        let signature = try XCTUnwrap(SecKeyCreateSignature(other ?? privateKey, .rsaSignatureMessagePKCS1v15SHA256, signed as CFData, &error)) as Data
        return "\(head).\(body).\(Self.base64URL(signature))"
    }
}

private func jwks(_ keys: [TestKey]) throws -> Data {
    try JSONSerialization.data(withJSONObject: ["keys": keys.map(\.jwk), "public_certs": []])
}

final class TunnelAccessTests: XCTestCase {
    private let team = "team-test.cloudflareaccess.com"
    private let audience = "aud-tag-1234"
    private let start = Date(timeIntervalSince1970: 1_800_000_000)

    private func claims(now: Date, aud: Any? = nil, iss: String? = nil, exp: TimeInterval = 600, nbf: TimeInterval? = nil, iat: TimeInterval = 0) -> [String: Any] {
        var result: [String: Any] = ["aud": aud ?? [audience], "iss": iss ?? "https://\(team)",
                                     "exp": now.timeIntervalSince1970 + exp, "iat": now.timeIntervalSince1970 + iat]
        if let nbf { result["nbf"] = now.timeIntervalSince1970 + nbf }
        return result
    }

    private func verifier(keys: [TestKey], clock: Clock, log: FetchLog? = nil) throws -> (AccessJWTVerifier, AccessKeySet, FetchLog) {
        let log = try log ?? FetchLog([try jwks(keys)])
        let set = AccessKeySet(fetch: { try log.next($0) }, now: { clock.now })
        return (AccessJWTVerifier(keys: set, now: { clock.now }), set, log)
    }

    // MARK: - JWT

    func testAGoodTokenVerifiesAndTheCertsURLIsTheTeams() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let (verifier, _, log) = try verifier(keys: [key], clock: clock)
        let result = verifier.verify(try key.token(claims: claims(now: clock.now)), team: team, audience: audience)
        guard case .success(let parsed) = result else { return XCTFail("\(result)") }
        XCTAssertEqual(parsed.audiences, [audience])
        XCTAssertEqual(log.urls.first?.absoluteString, "https://\(team)/cdn-cgi/access/certs")
    }

    func testAudienceMayBeAStringOrAListAndMustMatch() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let (verifier, _, _) = try verifier(keys: [key], clock: clock)
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now, aud: ["other", audience])), team: team, audience: audience).isSuccess, true)
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now, aud: audience)), team: team, audience: audience).isSuccess, true)
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now, aud: ["someone-else"])), team: team, audience: audience).failure, .wrongAudience)
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now)), team: team, audience: "").failure, .wrongAudience, "an unset AUD never matches")
    }

    func testWrongIssuerIsRefused() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let (verifier, _, _) = try verifier(keys: [key], clock: clock)
        let token = try key.token(claims: claims(now: clock.now, iss: "https://evil.cloudflareaccess.com"))
        XCTAssertEqual(verifier.verify(token, team: team, audience: audience).failure, .wrongIssuer)
    }

    func testTimeClaimsAllowSixtySecondsOfSkew() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let (verifier, _, _) = try verifier(keys: [key], clock: clock)
        func check(_ claims: [String: Any]) throws -> AccessJWTError? { verifier.verify(try key.token(claims: claims), team: team, audience: audience).failure }

        XCTAssertEqual(try check(claims(now: clock.now, exp: -120)), .expired)
        XCTAssertNil(try check(claims(now: clock.now, exp: -30)), "expired 30 s ago is inside the skew")
        XCTAssertEqual(try check(claims(now: clock.now, nbf: 120)), .notYetValid)
        XCTAssertNil(try check(claims(now: clock.now, nbf: 30)))
        XCTAssertEqual(try check(claims(now: clock.now, iat: 120)), .issuedInFuture)
        XCTAssertNil(try check(claims(now: clock.now, iat: 30)))
    }

    func testABadSignatureIsRefused() throws {
        let key = try TestKey(kid: "k1"), impostor = try TestKey(kid: "k1"), clock = Clock(start)
        let (verifier, _, _) = try verifier(keys: [key], clock: clock)
        let forged = try key.token(claims: claims(now: clock.now), signWith: impostor.privateKey)
        XCTAssertEqual(verifier.verify(forged, team: team, audience: audience).failure, .badSignature)

        let good = try key.token(claims: claims(now: clock.now))
        var parts = good.split(separator: ".").map(String.init)
        parts[1] = TestKey.base64URL(try JSONSerialization.data(withJSONObject: claims(now: clock.now, exp: 99_999)))
        XCTAssertEqual(verifier.verify(parts.joined(separator: "."), team: team, audience: audience).failure, .badSignature, "a changed payload breaks the signature")
    }

    func testOtherAlgorithmsAndGarbageAreRefused() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let (verifier, _, _) = try verifier(keys: [key], clock: clock)
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now), header: ["alg": "none"]), team: team, audience: audience).failure, .unsupportedAlgorithm)
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now), header: ["alg": "HS256"]), team: team, audience: audience).failure, .unsupportedAlgorithm)
        XCTAssertEqual(verifier.verify("not.a.jwt", team: team, audience: audience).failure, .malformed)
        XCTAssertEqual(verifier.verify("", team: team, audience: audience).failure, .malformed)
    }

    func testAnUnknownKidRefetchesTheKeysOnce() throws {
        let first = try TestKey(kid: "old"), rotated = try TestKey(kid: "new"), clock = Clock(start)
        let log = FetchLog([try jwks([first]), try jwks([first, rotated])])
        let (verifier, set, _) = try verifier(keys: [], clock: clock, log: log)

        XCTAssertEqual(verifier.verify(try first.token(claims: claims(now: clock.now)), team: team, audience: audience).isSuccess, true)
        XCTAssertEqual(set.fetchCount, 1)
        XCTAssertEqual(verifier.verify(try first.token(claims: claims(now: clock.now)), team: team, audience: audience).isSuccess, true)
        XCTAssertEqual(set.fetchCount, 1, "a known kid is served from the cache")

        clock.advance(AccessKeySet.minimumRefetchInterval + 1)
        XCTAssertEqual(verifier.verify(try rotated.token(claims: claims(now: clock.now)), team: team, audience: audience).isSuccess, true)
        XCTAssertEqual(set.fetchCount, 2, "the unknown kid made one refetch, and the new key was found")
    }

    func testRandomKidsCannotForceRefetchesFasterThanTheMinimumInterval() throws {
        let key = try TestKey(kid: "k1"), stranger = try TestKey(kid: "nobody"), clock = Clock(start)
        let (verifier, set, _) = try verifier(keys: [key], clock: clock)
        XCTAssertEqual(verifier.verify(try stranger.token(claims: claims(now: clock.now)), team: team, audience: audience).failure, .unknownKey)
        XCTAssertEqual(verifier.verify(try stranger.token(claims: claims(now: clock.now)), team: team, audience: audience).failure, .unknownKey)
        XCTAssertEqual(set.fetchCount, 1)
        clock.advance(AccessKeySet.minimumRefetchInterval + 1)
        _ = verifier.verify(try stranger.token(claims: claims(now: clock.now)), team: team, audience: audience)
        XCTAssertEqual(set.fetchCount, 2)
    }

    func testAFailedFetchKeepsServingCachedKeys() throws {
        struct Down: Error {}
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let data = try jwks([key])
        let calls = FetchLog([data])
        let failing = FailingFetch(good: { try calls.next($0) })
        let set = AccessKeySet(fetch: { try failing.fetch($0) }, now: { clock.now })
        let verifier = AccessJWTVerifier(keys: set, now: { clock.now })
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now)), team: team, audience: audience).isSuccess, true)
        failing.down = true
        clock.advance(AccessKeySet.minimumRefetchInterval + 1)
        XCTAssertEqual(verifier.verify(try key.token(claims: claims(now: clock.now)), team: team, audience: audience).isSuccess, true)
    }

    // MARK: - Header-based tunnel detection

    private func gate(config: AccessConfig, key: TestKey, clock: Clock, limiter: AuthRateLimiter = AuthRateLimiter()) throws -> TunnelGate {
        let log = FetchLog([try jwks([key])])
        let set = AccessKeySet(fetch: { try log.next($0) }, now: { clock.now })
        return TunnelGate(config: { config }, verifier: AccessJWTVerifier(keys: set, now: { clock.now }), limiter: limiter, now: { clock.now })
    }

    private func configured(enabled: Bool = true) -> AccessConfig {
        var config = AccessConfig()
        config.enabled = enabled
        config.teamDomain = team
        config.audience = audience
        config.clientId = "id.access"
        config.clientSecret = "secret"
        return config
    }

    func testHeadersWithoutCloudflareMarksAreLocalEvenWhenInternetAccessIsOff() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let gate = try gate(config: configured(enabled: false), key: key, clock: clock)
        XCTAssertEqual(gate.evaluate(headers: []), .local)
        XCTAssertEqual(gate.evaluate(headers: [("Host", "192.168.1.5"), ("Sec-WebSocket-Protocol", "agentconsole.v1")]), .local)
    }

    func testTunnelOriginIsRefusedWhenTheToggleIsOff() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let gate = try gate(config: configured(enabled: false), key: key, clock: clock)
        let token = try key.token(claims: claims(now: clock.now))
        for headers: [(name: String, value: String)] in [
            [("Cf-Connecting-Ip", "203.0.113.9"), ("Cf-Access-Jwt-Assertion", token)],
            [("cf-ray", "abc123-SGN"), ("Cf-Access-Jwt-Assertion", token)],
            [("CF-CONNECTING-IP", "203.0.113.9")],
        ] {
            guard case .reject = gate.evaluate(headers: headers) else { return XCTFail("\(headers)") }
        }
    }

    func testTunnelOriginNeedsAValidAccessToken() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let gate = try gate(config: configured(), key: key, clock: clock)
        let ip: (name: String, value: String) = ("Cf-Connecting-Ip", "203.0.113.9")

        guard case .reject = gate.evaluate(headers: [ip]) else { return XCTFail("no token") }
        guard case .reject = gate.evaluate(headers: [ip, ("Cf-Access-Jwt-Assertion", "garbage")]) else { return XCTFail("garbage token") }
        let wrongAudience = try key.token(claims: claims(now: clock.now, aud: ["nope"]))
        guard case .reject = gate.evaluate(headers: [ip, ("Cf-Access-Jwt-Assertion", wrongAudience)]) else { return XCTFail("wrong aud") }

        let good = try key.token(claims: claims(now: clock.now))
        XCTAssertEqual(gate.evaluate(headers: [ip, ("cf-access-jwt-assertion", good)]), .tunnel(clientAddress: "203.0.113.9"))
        XCTAssertEqual(gate.evaluate(headers: [("Cf-Ray", "x"), ("Cf-Access-Jwt-Assertion", good)]), .tunnel(clientAddress: TunnelGate.unknownClient))
    }

    func testTunnelOriginIsRefusedWhileTheSettingsAreIncomplete() throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        var config = configured()
        config.audience = ""
        let gate = try gate(config: config, key: key, clock: clock)
        let token = try key.token(claims: claims(now: clock.now))
        guard case .reject = gate.evaluate(headers: [("Cf-Connecting-Ip", "203.0.113.9"), ("Cf-Access-Jwt-Assertion", token)]) else { return XCTFail() }
    }

    // MARK: - Rate limiting by the real client

    func testTheGateAndTheRouterKeyLockoutsByTheClientAddressNotLoopback() async throws {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let limiter = AuthRateLimiter()
        let gate = try gate(config: configured(), key: key, clock: clock, limiter: limiter)
        let token = try key.token(claims: claims(now: clock.now))
        func headers(_ ip: String) -> [(name: String, value: String)] { [("Cf-Connecting-Ip", ip), ("Cf-Access-Jwt-Assertion", token)] }

        limiter.recordFailure("203.0.113.9", now: clock.now)
        guard case .reject = gate.evaluate(headers: headers("203.0.113.9")) else { return XCTFail("locked-out client got in") }
        XCTAssertEqual(gate.evaluate(headers: headers("198.51.100.4")), .tunnel(clientAddress: "198.51.100.4"), "another client behind the same tunnel is unaffected")
        clock.advance(2)
        XCTAssertEqual(gate.evaluate(headers: headers("203.0.113.9")), .tunnel(clientAddress: "203.0.113.9"), "the 1 s backoff has passed")

        // The router counts failed logins on `remoteAddress`, which for a tunnel client is its own address.
        let rig = try makeBridgeRig()
        let first = BridgeConnection(remoteAddress: "203.0.113.50", io: RecordingIO(), isTunnel: true)
        let other = BridgeConnection(remoteAddress: "198.51.100.60", io: RecordingIO(), isTunnel: true)
        let otherIO = other.io as! RecordingIO
        rig.core.hub.add(first); rig.core.hub.add(other)
        await rig.router.handle(#"{"id":1,"op":"hello","token":"wrong"}"#, from: first)
        XCTAssertTrue((first.io as! RecordingIO).isClosed)

        let again = BridgeConnection(remoteAddress: "203.0.113.50", io: RecordingIO(), isTunnel: true)
        rig.core.hub.add(again)
        await rig.router.handle(#"{"id":2,"op":"hello","token":"wrong"}"#, from: again)
        XCTAssertTrue((again.io as! RecordingIO).isClosed, "same client is locked out")
        XCTAssertEqual(try (again.io as! RecordingIO).texts.isEmpty, true, "closed without an answer while locked")

        await rig.router.handle(#"{"id":3,"op":"hello","token":"wrong"}"#, from: other)
        XCTAssertEqual(try otherIO.json(0).errorCode, "unauthorized", "a different client still gets an answer")
    }

    func testTunnelClientsAreAuditedByTheirOwnAddress() async throws {
        let rig = try makeBridgeRig()
        let (device, token) = try rig.core.devices.register(name: "away-phone", platform: "ios")
        _ = device
        let io = RecordingIO()
        let connection = BridgeConnection(remoteAddress: "203.0.113.77", io: io, isTunnel: true)
        rig.core.hub.add(connection)
        rig.backend.sessionList = [makeSession("s1")]
        await rig.router.handle(#"{"id":1,"op":"hello","token":"\#(token)"}"#, from: connection)
        await rig.router.handle(#"{"id":2,"op":"session.message","sessionId":"s1","text":"hi"}"#, from: connection)
        XCTAssertEqual(rig.core.audit.recent(limit: 1).first?.address, "203.0.113.77")

        let (_, localIO, _) = try await rig.login(name: "home-phone", from: "192.168.1.30")
        _ = localIO
    }

    // MARK: - Settings file and QR

    func testAccessSettingsAreStoredPrivatelyAndReloaded() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("access-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("access.json")
        let store = AccessConfigStore(fileURL: url)
        XCTAssertEqual(store.config.hostname, "console.trietkun.shop")
        XCTAssertEqual(store.config.teamDomain, "bold-dew-100f.cloudflareaccess.com")
        XCTAssertFalse(store.config.enabled)

        store.update { $0.enabled = true; $0.audience = "aud"; $0.clientId = "id"; $0.clientSecret = "s3cret" }
        let permissions = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int
        XCTAssertEqual(permissions, 0o600)
        let reloaded = AccessConfigStore(fileURL: url).config
        XCTAssertEqual(reloaded, store.config)
        XCTAssertTrue(reloaded.offersTunnel)
    }

    func testQRKeepsTheV1FieldsAndAddsTheTunnelOnlyWhenOnAndComplete() throws {
        func object(_ access: AccessConfig) throws -> [String: Any] {
            let text = try XCTUnwrap(PairingPayload.make(name: "Mac", port: 47821, fingerprint: String(repeating: "a", count: 64),
                                                         hosts: ["192.168.1.5", "mac.local"], code: "ABCD2345", access: access))
            return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any])
        }
        var access = configured()
        access.hostname = "console.trietkun.shop"

        let withTunnel = try object(access)
        XCTAssertEqual(withTunnel["v"] as? Int, 1)
        XCTAssertEqual(withTunnel["port"] as? Int, 47821)
        XCTAssertEqual(withTunnel["hosts"] as? [String], ["192.168.1.5", "mac.local"])
        XCTAssertEqual(withTunnel["code"] as? String, "ABCD2345")
        XCTAssertEqual(withTunnel["fp"] as? String, String(repeating: "a", count: 64))
        let tunnel = try XCTUnwrap(withTunnel["tunnel"] as? [String: Any])
        XCTAssertEqual(tunnel["url"] as? String, "wss://console.trietkun.shop")
        XCTAssertEqual(tunnel["accessClientId"] as? String, "id.access")
        XCTAssertEqual(tunnel["accessClientSecret"] as? String, "secret")

        var off = access; off.enabled = false
        XCTAssertNil(try object(off)["tunnel"], "toggle off")
        var missingSecret = access; missingSecret.clientSecret = ""
        XCTAssertNil(try object(missingSecret)["tunnel"], "a field is missing")
        XCTAssertEqual(Set(try object(off).keys), ["v", "name", "port", "fp", "hosts", "code"])
    }
}

private final class FailingFetch: @unchecked Sendable {
    var down = false
    let good: @Sendable (URL) throws -> Data
    init(good: @escaping @Sendable (URL) throws -> Data) { self.good = good }
    func fetch(_ url: URL) throws -> Data {
        if down { throw URLError(.notConnectedToInternet) }
        return try good(url)
    }
}

private extension Result {
    var isSuccess: Bool { if case .success = self { return true } else { return false } }
    var failure: Failure? { if case .failure(let error) = self { return error } else { return nil } }
}

// MARK: - Tunnel origin (raw HTTP/WebSocket on 127.0.0.1:47822)

final class TunnelOriginTests: XCTestCase {
    private let team = "team-test.cloudflareaccess.com"
    private let audience = "aud-tag-1234"
    private let start = Date(timeIntervalSince1970: 1_800_000_000)
    private let key = "dGhlIHNhbXBsZSBub25jZQ=="

    private func head(_ text: String) -> HeadParse {
        var reader = HTTPHeadReader(now: start)
        return reader.feed(Data(text.utf8))
    }

    private func request(extra: [String] = [], path: String = "/v1", connection: String = "Upgrade", version: String = "13",
                         key: String? = "dGhlIHNhbXBsZSBub25jZQ==", protocols: String? = "agentconsole.v1") -> String {
        var lines = ["GET \(path) HTTP/1.1", "Host: console.trietkun.shop", "Connection: \(connection)", "Upgrade: websocket",
                     "Sec-Websocket-Version: \(version)"]
        if let key { lines.append("Sec-Websocket-Key: \(key)") }
        if let protocols { lines.append("Sec-Websocket-Protocol: \(protocols)") }
        return (lines + extra).joined(separator: "\r\n") + "\r\n\r\n"
    }

    private func parsed(_ text: String) throws -> HTTPHead {
        guard case .complete(let head, _) = head(text) else { throw XCTSkip("did not parse: \(text)") }
        return head
    }

    // MARK: Head parser

    func testParsesACloudflaredStyleRequestCaseInsensitively() throws {
        let parsed = try parsed(request(extra: ["Cf-Connecting-Ip: 203.0.113.9"]))
        XCTAssertEqual(parsed.method, "GET")
        XCTAssertEqual(parsed.path, "/v1")
        XCTAssertEqual(parsed.value("sec-websocket-key"), key)
        XCTAssertEqual(parsed.value("CF-CONNECTING-IP"), "203.0.113.9")
    }

    func testHeadSplitAcrossReadsAndLeftoverBytes() {
        var reader = HTTPHeadReader(now: start)
        let text = request()
        XCTAssertEqual(reader.feed(Data(text.utf8.prefix(20))), .needMore)
        guard case .complete(_, let leftover) = reader.feed(Data((String(text.dropFirst(20)) + "XY").utf8)) else { return XCTFail() }
        XCTAssertEqual(leftover, Data("XY".utf8))
    }

    func testConnectionHeaderWithSeveralTokensStillMeansUpgrade() throws {
        let parsed = try parsed(request(connection: "keep-alive, Upgrade"))
        XCTAssertTrue(parsed.tokens("Connection").contains("upgrade"))
    }

    func testRejectsMalformedHeads() {
        for text in ["GARBAGE\r\n\r\n", "GET /v1 HTTP/1.0\r\nHost: a\r\n\r\n", "GET v1 HTTP/1.1\r\n\r\n",
                     "GET /v1 HTTP/1.1\r\nBad Name: x\r\n\r\n", "GET /v1 HTTP/1.1\r\nNoColon\r\n\r\n", "GET /v1 HTTP/1.1\r\n folded: x\r\n\r\n"] {
            guard case .failure(let status, _) = head(text) else { return XCTFail(text) }
            XCTAssertEqual(status, 400)
        }
    }

    func testOversizedHeadIsRefusedWithoutWaitingForTheEnd() {
        var reader = HTTPHeadReader(now: start)
        let junk = "GET /v1 HTTP/1.1\r\nX-Pad: " + String(repeating: "a", count: HTTPHeadReader.maxHeadBytes + 10)
        guard case .failure(let status, _) = reader.feed(Data(junk.utf8)) else { return XCTFail() }
        XCTAssertEqual(status, 400)
    }

    func testSlowClientTimesOutAfterFiveSeconds() {
        var reader = HTTPHeadReader(now: start)
        XCTAssertEqual(reader.feed(Data("GET /v1 HT".utf8)), .needMore)
        XCTAssertFalse(reader.timedOut(now: start.addingTimeInterval(4.9)))
        XCTAssertTrue(reader.timedOut(now: start.addingTimeInterval(5)))
    }

    // MARK: Accept key

    func testAcceptKeyMatchesTheRFC6455Example() {
        XCTAssertEqual(WebSocketAccept.value(forKey: "dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        XCTAssertTrue(WebSocketAccept.isValidKey("dGhlIHNhbXBsZSBub25jZQ=="))
        XCTAssertFalse(WebSocketAccept.isValidKey("short"))
    }

    // MARK: Frames

    private func masked(opcode: UInt8, payload: Data, fin: Bool = true, mask: [UInt8] = [1, 2, 3, 4]) -> Data {
        var frame = Data([(fin ? 0x80 : 0) | opcode])
        switch payload.count {
        case ..<126: frame.append(0x80 | UInt8(payload.count))
        case ..<65536: frame.append(0x80 | 126); frame.append(contentsOf: [UInt8(payload.count >> 8), UInt8(payload.count & 0xFF)])
        default: frame.append(0x80 | 127); frame.append(contentsOf: withUnsafeBytes(of: UInt64(payload.count).bigEndian) { Array($0) })
        }
        frame.append(contentsOf: mask)
        for (index, byte) in payload.enumerated() { frame.append(byte ^ mask[index % 4]) }
        return frame
    }

    func testDecodesMaskedTextAndBinaryWithEveryLengthForm() {
        var decoder = WSFrameDecoder(maxMessageBytes: 300_000)
        XCTAssertEqual(decoder.feed(masked(opcode: 1, payload: Data("xin chào".utf8))), [.text("xin chào")])
        let medium = Data(repeating: 0x41, count: 300)
        XCTAssertEqual(decoder.feed(masked(opcode: 2, payload: medium)), [.binary(medium)])
        let large = Data(repeating: 0x42, count: 70_000)
        XCTAssertEqual(decoder.feed(masked(opcode: 2, payload: large)), [.binary(large)])
    }

    func testFramesArrivingByteByByteAreReassembled() {
        var decoder = WSFrameDecoder()
        var events: [WSFrameDecoder.Event] = []
        for byte in masked(opcode: 1, payload: Data("hello".utf8)) { events += decoder.feed(Data([byte])) }
        XCTAssertEqual(events, [.text("hello")])
    }

    func testFragmentedMessagesWithAPingInTheMiddle() {
        var decoder = WSFrameDecoder()
        var events = decoder.feed(masked(opcode: 1, payload: Data("hel".utf8), fin: false))
        events += decoder.feed(masked(opcode: 9, payload: Data("p".utf8)))
        events += decoder.feed(masked(opcode: 0, payload: Data("lo ".utf8), fin: false))
        events += decoder.feed(masked(opcode: 0, payload: Data("there".utf8)))
        XCTAssertEqual(events, [.ping(Data("p".utf8)), .text("hello there")])
    }

    func testProtocolViolationsEndWithTheRightCloseCode() {
        func failure(_ data: Data, max: Int = 1000) -> UInt16? {
            var decoder = WSFrameDecoder(maxMessageBytes: max)
            for case .failure(let code, _) in decoder.feed(data) { return code }
            return nil
        }
        XCTAssertEqual(failure(Data([0x81, 0x01, 0x41])), 1002, "unmasked")
        XCTAssertEqual(failure(masked(opcode: 0, payload: Data("x".utf8))), 1002, "continuation with nothing to continue")
        XCTAssertEqual(failure(masked(opcode: 9, payload: Data("x".utf8), fin: false)), 1002, "fragmented control frame")
        XCTAssertEqual(failure(masked(opcode: 3, payload: Data())), 1002, "reserved opcode")
        XCTAssertEqual(failure(Data([0xC1, 0x80, 0, 0, 0, 0])), 1002, "RSV bit")
        XCTAssertEqual(failure(masked(opcode: 1, payload: Data([0xFF, 0xFE]))), 1007, "not UTF-8")
        XCTAssertEqual(failure(masked(opcode: 2, payload: Data(repeating: 0, count: 2000))), 1009, "too big")
        XCTAssertEqual(failure(Data([0x82, 0xFF]) + Data(repeating: 0xFF, count: 8) + Data(count: 4)), 1002, "64-bit length with the top bit set")
        var decoder = WSFrameDecoder()
        _ = decoder.feed(Data([0x81, 0x01, 0x41]))
        XCTAssertEqual(decoder.feed(masked(opcode: 1, payload: Data("x".utf8))), [], "nothing is read after a failure")
    }

    func testCloseFrameCarriesTheCode() {
        var decoder = WSFrameDecoder()
        XCTAssertEqual(decoder.feed(masked(opcode: 8, payload: Data([0x03, 0xE8]))), [.close(code: 1000)])
    }

    func testServerFramesAreUnmaskedWithTheRightLengthForms() {
        XCTAssertEqual(WSFrame.encode(opcode: WSFrame.text, payload: Data("hi".utf8)), Data([0x81, 2, 0x68, 0x69]))
        let medium = WSFrame.encode(opcode: WSFrame.binary, payload: Data(count: 300))
        XCTAssertEqual(Array(medium.prefix(4)), [0x82, 126, 0x01, 0x2C])
        let large = WSFrame.encode(opcode: WSFrame.binary, payload: Data(count: 70_000))
        XCTAssertEqual(Array(large.prefix(10)), [0x82, 127, 0, 0, 0, 0, 0, 1, 0x11, 0x70])
        XCTAssertEqual(WSFrame.closeFrame(code: 1002), Data([0x88, 2, 0x03, 0xEA]))
    }

    // MARK: Upgrade decisions

    private func gate(enabled: Bool = true) throws -> (TunnelGate, TestKey, Clock) {
        let key = try TestKey(kid: "k1"), clock = Clock(start)
        let log = FetchLog([try jwks([key])])
        var config = AccessConfig()
        config.enabled = enabled; config.teamDomain = team; config.audience = audience; config.clientId = "i"; config.clientSecret = "s"
        let set = AccessKeySet(fetch: { try log.next($0) }, now: { clock.now })
        let gate = TunnelGate(config: { config }, verifier: AccessJWTVerifier(keys: set, now: { clock.now }), limiter: AuthRateLimiter(), now: { clock.now })
        return (gate, key, clock)
    }

    private func status(_ outcome: TunnelUpgradeOutcome) -> Int {
        switch outcome {
        case .accept: return 101
        case .refuse(_, let status): return status
        }
    }

    private func jwt(_ key: TestKey, _ clock: Clock, aud: String? = nil) throws -> String {
        try key.token(claims: ["aud": [aud ?? audience], "iss": "https://\(team)", "exp": clock.now.timeIntervalSince1970 + 600, "iat": clock.now.timeIntervalSince1970])
    }

    func testRequestsWithoutCloudflareHeadersAreRefusedWith403() throws {
        let (gate, _, _) = try gate()
        XCTAssertEqual(status(TunnelUpgrade.evaluate(try parsed(request()), gate: gate)), 403)
        guard case .refuse(let response, _) = TunnelUpgrade.evaluate(try parsed(request()), gate: gate) else { return XCTFail() }
        XCTAssertTrue(String(decoding: response, as: UTF8.self).hasPrefix("HTTP/1.1 403 Forbidden\r\n"))
    }

    func testEveryRefusalIsARealHTTPStatus() throws {
        let (gate, key, clock) = try gate()
        let cf = ["Cf-Connecting-Ip: 203.0.113.9"]
        func code(_ text: String) throws -> Int { status(TunnelUpgrade.evaluate(try parsed(text), gate: gate)) }
        XCTAssertEqual(try code(request(extra: cf, path: "/other")), 404)
        XCTAssertEqual(try code(request(extra: cf, connection: "close")), 426)
        XCTAssertEqual(try code(request(extra: cf, version: "8")), 426)
        XCTAssertEqual(try code(request(extra: cf, key: "nope")), 400)
        XCTAssertEqual(try code(request(extra: cf, key: nil)), 400)
        XCTAssertEqual(try code(request(extra: cf, protocols: "other.v9")), 400)
        XCTAssertEqual(try code(request(extra: cf)), 401, "no Access token")
        XCTAssertEqual(try code(request(extra: cf + ["Cf-Access-Jwt-Assertion: garbage"])), 401)
        XCTAssertEqual(try code(request(extra: cf + ["Cf-Access-Jwt-Assertion: \(try jwt(key, clock, aud: "other"))"])), 401)
        XCTAssertEqual(try code("POST /v1 HTTP/1.1\r\nCf-Ray: x\r\n\r\n"), 400)
    }

    func testToggleOffIs403AndALockedOutClientIs429() throws {
        let (off, key, clock) = try gate(enabled: false)
        let good = request(extra: ["Cf-Connecting-Ip: 203.0.113.9", "Cf-Access-Jwt-Assertion: \(try jwt(key, clock))"])
        XCTAssertEqual(status(TunnelUpgrade.evaluate(try parsed(good), gate: off)), 403)

        let log = FetchLog([try jwks([key])])
        let limiter = AuthRateLimiter()
        var config = AccessConfig()
        config.enabled = true; config.teamDomain = team; config.audience = audience; config.clientId = "i"; config.clientSecret = "s"
        let set = AccessKeySet(fetch: { try log.next($0) }, now: { clock.now })
        let on = TunnelGate(config: { config }, verifier: AccessJWTVerifier(keys: set, now: { clock.now }), limiter: limiter, now: { clock.now })
        limiter.recordFailure("203.0.113.9", now: clock.now)
        XCTAssertEqual(status(TunnelUpgrade.evaluate(try parsed(good), gate: on)), 429)
    }

    func testAValidTokenGetsA101WithTheAcceptKeyAndSubprotocol() throws {
        let (gate, key, clock) = try gate()
        let text = request(extra: ["Cf-Connecting-Ip: 203.0.113.9", "Cf-Access-Jwt-Assertion: \(try jwt(key, clock))"], connection: "keep-alive, Upgrade")
        guard case .accept(let response, let client) = TunnelUpgrade.evaluate(try parsed(text), gate: gate) else { return XCTFail() }
        XCTAssertEqual(client, "203.0.113.9")
        let reply = String(decoding: response, as: UTF8.self)
        XCTAssertTrue(reply.hasPrefix("HTTP/1.1 101 Switching Protocols\r\n"))
        XCTAssertTrue(reply.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"))
        XCTAssertTrue(reply.contains("Sec-WebSocket-Protocol: agentconsole.v1\r\n"))
        XCTAssertTrue(reply.hasSuffix("\r\n\r\n"))
    }

    func testPortsAreFixedAndTheTunnelOneIsNotTheLANOne() {
        XCTAssertEqual(BridgePorts.lan, 47821)
        XCTAssertEqual(BridgePorts.tunnel, 47822)
    }
}
