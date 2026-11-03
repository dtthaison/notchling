import XCTest
@testable import ConsoleCore

final class BridgeSecurityTests: XCTestCase {
    func testAddressFilterAllowsLoopbackLANLinkLocalAndTailscale() {
        for address in ["127.0.0.1", "127.9.9.9", "10.0.0.5", "10.255.255.255", "172.16.0.1", "172.31.255.255",
                        "192.168.0.10", "169.254.12.1", "100.64.0.1", "100.100.1.1", "100.127.255.255",
                        "::1", "fe80::1", "fe80::1%en0", "febf::1", "fc00::1", "fd12:3456::1", "fdff::1",
                        "fd7a:115c:a1e0::5", "fd7a:115c:a1e0:ab12::1", "::ffff:192.168.1.5", "::ffff:127.0.0.1", "[::1]"] {
            XCTAssertTrue(AddressFilter.isAllowed(address), address)
        }
    }

    func testAddressFilterBlocksEverythingElse() {
        for address in ["8.8.8.8", "1.1.1.1", "172.15.255.255", "172.32.0.1", "192.169.0.1", "192.167.255.255",
                        "100.63.255.255", "100.128.0.1", "169.253.0.1", "11.0.0.1", "0.0.0.0",
                        "2001:4860:4860::8888", "fec0::1", "fe00::1", "fb00::1", "fe::1",
                        "::ffff:8.8.8.8", "::", "", "not an address", "300.1.1.1"] {
            XCTAssertFalse(AddressFilter.isAllowed(address), address)
        }
    }

    func testUniqueLocalIPv6IncludesTheTailscaleRange() {
        // fd7a:115c:a1e0::/48 sits inside fc00::/7, so any fdXX address is allowed, Tailscale or not.
        XCTAssertTrue(AddressFilter.isAllowed("fd7a:115c:a1e1::1"))
        XCTAssertTrue(AddressFilter.isAllowed("fd7b:115c:a1e0::1"))
        XCTAssertFalse(AddressFilter.isAllowed("fe00::1"), "the next block after fc00::/7 is not")
    }

    func testTailscaleAndLANClassifiers() {
        XCTAssertTrue(AddressFilter.isTailscaleV4("100.101.102.103"))
        XCTAssertFalse(AddressFilter.isTailscaleV4("100.200.0.1"))
        XCTAssertTrue(AddressFilter.isPrivateLANV4("192.168.1.9"))
        XCTAssertFalse(AddressFilter.isPrivateLANV4("100.64.0.1"))
    }

    func testPairingCodeShapeExpiryAndOneTimeUse() {
        let codes = PairingCodes()
        let issued = codes.issue(now: fixtureNow)
        XCTAssertEqual(issued.code.count, 8)
        XCTAssertFalse(issued.code.contains(where: { "01OIL".contains($0) }))
        XCTAssertEqual(issued.expiresAt, fixtureNow.addingTimeInterval(300))

        XCTAssertFalse(codes.consume("WRONG123", now: fixtureNow.addingTimeInterval(1)))
        XCTAssertNotNil(codes.live(now: fixtureNow.addingTimeInterval(1)), "a wrong guess does not burn the live code")
        XCTAssertTrue(codes.consume(issued.code.lowercased(), now: fixtureNow.addingTimeInterval(2)), "case-insensitive")
        XCTAssertFalse(codes.consume(issued.code, now: fixtureNow.addingTimeInterval(3)), "one time only")
    }

    func testPairingCodeExpiresAfterFiveMinutes() {
        let codes = PairingCodes()
        let issued = codes.issue(now: fixtureNow)
        XCTAssertFalse(codes.consume(issued.code, now: fixtureNow.addingTimeInterval(300)))
        XCTAssertNil(codes.live(now: fixtureNow.addingTimeInterval(300)))

        let first = codes.issue(now: fixtureNow)
        let second = codes.issue(now: fixtureNow)
        XCTAssertFalse(codes.consume(first.code, now: fixtureNow), "issuing a new code replaces the old one")
        XCTAssertTrue(codes.consume(second.code, now: fixtureNow))
    }

    func testTokenShapeHashAndConstantTimeCompare() {
        let token = BridgeCrypto.randomToken()
        XCTAssertEqual(token.count, 43, "32 bytes, base64url, no padding")
        XCTAssertNil(token.rangeOfCharacter(from: CharacterSet(charactersIn: "+/=")))
        XCTAssertNotEqual(token, BridgeCrypto.randomToken())
        XCTAssertEqual(BridgeCrypto.sha256Hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        XCTAssertTrue(BridgeCrypto.constantTimeEqual("same", "same"))
        XCTAssertFalse(BridgeCrypto.constantTimeEqual("same", "sama"))
        XCTAssertFalse(BridgeCrypto.constantTimeEqual("same", "samee"), "different length")
        XCTAssertTrue(BridgeCrypto.constantTimeEqual([], []))
    }

    func testDeviceStoreKeepsOnlyTheHashAndAuthenticates() throws {
        let rig = try makeBridgeRig()
        let (device, token) = try rig.core.devices.register(name: "iPhone của Triết", platform: "ios")
        XCTAssertEqual(rig.core.devices.authenticate(token: token)?.id, device.id)
        XCTAssertNil(rig.core.devices.authenticate(token: token + "x"))
        XCTAssertNil(rig.core.devices.authenticate(token: ""))

        let file = rig.directory.appendingPathComponent("devices.json")
        let text = try String(contentsOf: file, encoding: .utf8)
        XCTAssertFalse(text.contains(token), "the token is never written")
        XCTAssertTrue(text.contains(BridgeCrypto.sha256Hex(token)))
        let permissions = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
        XCTAssertEqual(permissions, 0o600)
        let folder = try FileManager.default.attributesOfItem(atPath: rig.directory.path)[.posixPermissions] as? Int
        XCTAssertEqual(folder, 0o700)

        let reloaded = DeviceStore(fileURL: file)
        XCTAssertEqual(reloaded.devices.map(\.name), ["iPhone của Triết"])
        XCTAssertEqual(reloaded.authenticate(token: token)?.id, device.id, "survives a restart")
    }

    func testRevokeRemovesTheDeviceAndItsToken() throws {
        let rig = try makeBridgeRig()
        let (first, firstToken) = try rig.core.devices.register(name: "a", platform: "ios")
        let (_, secondToken) = try rig.core.devices.register(name: "b", platform: "android")
        XCTAssertNotNil(try rig.core.devices.revoke(id: first.id))
        XCTAssertNil(rig.core.devices.authenticate(token: firstToken))
        XCTAssertNotNil(rig.core.devices.authenticate(token: secondToken))
        XCTAssertNil(try rig.core.devices.revoke(id: first.id), "second revoke finds nothing")
        XCTAssertEqual(DeviceStore(fileURL: rig.directory.appendingPathComponent("devices.json")).devices.count, 1)
    }

    func testBackoffThenFiveMinuteLock() {
        let limiter = AuthRateLimiter()
        let ip = "192.168.1.9"
        XCTAssertEqual(limiter.check(ip, now: fixtureNow), .allowed)
        for (index, delay) in [1.0, 2.0, 4.0, 8.0].enumerated() {
            let at = fixtureNow.addingTimeInterval(Double(index) * 100)
            XCTAssertEqual(limiter.recordFailure(ip, now: at), .blocked(until: at.addingTimeInterval(delay)), "failure \(index + 1)")
        }
        let fifth = fixtureNow.addingTimeInterval(500)
        XCTAssertEqual(limiter.recordFailure(ip, now: fifth), .blocked(until: fifth.addingTimeInterval(300)))
        XCTAssertEqual(limiter.check(ip, now: fifth.addingTimeInterval(299)), .blocked(until: fifth.addingTimeInterval(300)))
        XCTAssertEqual(limiter.check(ip, now: fifth.addingTimeInterval(301)), .allowed)
        XCTAssertEqual(limiter.check("192.168.1.10", now: fifth), .allowed, "per address")

        limiter.recordSuccess(ip)
        XCTAssertEqual(limiter.recordFailure(ip, now: fifth.addingTimeInterval(400)), .blocked(until: fifth.addingTimeInterval(401)), "a success starts over")
    }

    func testAuditLineHasNoMessageText() throws {
        let rig = try makeBridgeRig(clock: { fixtureNow })
        let secret = "do not log this sentence"
        rig.core.audit.append(device: "phone", op: "message", session: "s1", length: secret.count)
        let file = rig.directory.appendingPathComponent("remote-log.jsonl")
        let line = try String(contentsOf: file, encoding: .utf8)
        XCTAssertFalse(line.contains(secret))
        let object = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
        XCTAssertEqual(Set(object.keys), ["time", "device", "op", "session", "length"])
        XCTAssertEqual(object["length"] as? Int, secret.count)
        XCTAssertEqual(object["op"] as? String, "message")
        XCTAssertEqual(rig.core.audit.recent().first?.session, "s1")
        let permissions = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
        XCTAssertEqual(permissions, 0o600)
    }

    func testRecentAuditEntriesComeNewestFirstAndAreCapped() throws {
        let rig = try makeBridgeRig()
        for index in 0..<60 { rig.core.audit.append(device: "d", op: "message", session: "s\(index)", length: 1) }
        let recent = rig.core.audit.recent(limit: 50)
        XCTAssertEqual(recent.count, 50)
        XCTAssertEqual(recent.first?.session, "s59")
    }
}
