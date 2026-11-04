import XCTest
@testable import ConsoleCore

final class AppURLTests: XCTestCase {
    func testParsesOpenAndOpenWithPhones() {
        XCTAssertEqual(AppURL.parse(URL(string: "agentconsole://open")!), .open(phones: false))
        XCTAssertEqual(AppURL.parse(URL(string: "agentconsole://open?phones=1")!), .open(phones: true))
        XCTAssertEqual(AppURL.parse(URL(string: "AgentConsole://OPEN?phones=1")!), .open(phones: true), "case-insensitive")
        XCTAssertEqual(AppURL.parse(URL(string: "agentconsole://open?phones=0")!), .open(phones: false))
        XCTAssertEqual(AppURL.parse(URL(string: "agentconsole://open?phones=1&x=2&utm=3")!), .open(phones: true), "unknown parameters are ignored")
    }

    func testJunkIsRejected() {
        for text in ["agentconsole://close", "agentconsole://", "agentconsole:open", "https://open?phones=1", "other://open", "agentconsole://evil.com/open"] {
            XCTAssertNil(AppURL.parse(URL(string: text)!), text)
        }
    }

    func testOpenEventGoesOnlyToAuthenticatedConnections() async throws {
        let rig = try makeBridgeRig()
        let (_, loggedIn, _) = try await rig.login()
        let (_, anonymous) = rig.connect(from: "192.168.1.99")

        XCTAssertEqual(rig.core.hub.notifyOpen(), 1)
        XCTAssertEqual(loggedIn.texts.last, #"{"event":"open"}"#)
        XCTAssertTrue(anonymous.texts.isEmpty, "a client that has not said hello hears nothing")
    }

    func testHandleOpenAuditsAndCountsPhones() async throws {
        let rig = try makeBridgeRig()
        XCTAssertEqual(rig.core.handleOpen(phones: true), 0, "nobody connected: no-op, but still audited")
        XCTAssertEqual(rig.core.audit.recent().first?.op, "open")
        XCTAssertEqual(rig.core.audit.recent().first?.device, "mac")

        _ = try await rig.login()
        XCTAssertEqual(rig.core.handleOpen(phones: true), 1)
        XCTAssertEqual(rig.core.audit.recent().first?.length, 1)
        XCTAssertEqual(rig.core.handleOpen(phones: false), 0)
        XCTAssertEqual(rig.core.audit.recent().count, 2, "phones=0 writes nothing")
    }
}
