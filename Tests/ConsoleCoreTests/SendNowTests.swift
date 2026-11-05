import XCTest
@testable import ConsoleCore

final class SendNowTests: XCTestCase {
    func testAnOwnedSessionIsInterruptedOnceAndAudited() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p")]
        let (connection, io, _) = try await rig.login()
        await rig.router.handle(#"{"id":1,"op":"session.sendNow","sessionId":"p"}"#, from: connection)
        XCTAssertTrue(try io.lastJSON().isOK)
        XCTAssertEqual(rig.backend.interrupted, ["p"])
        XCTAssertEqual(try XCTUnwrap(rig.core.audit.recent().last).op, "sendNow")
        // A second ESC right away would cancel the queued message that just started.
        await rig.router.handle(#"{"id":2,"op":"session.sendNow","sessionId":"p"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "busy")
        XCTAssertEqual(rig.backend.interrupted, ["p"])
    }

    func testViewOnlyJobAndUnknownSessionsAreRefused() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("v", control: .viewOnly), makeSession("j", control: .job("x"))]
        let (connection, io, _) = try await rig.login()
        await rig.router.handle(#"{"id":1,"op":"session.sendNow","sessionId":"v"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "view_only")
        await rig.router.handle(#"{"id":2,"op":"session.sendNow","sessionId":"j"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "view_only")
        await rig.router.handle(#"{"id":3,"op":"session.sendNow","sessionId":"ghost"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
        await rig.router.handle(#"{"id":4,"op":"session.sendNow"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        XCTAssertTrue(rig.backend.interrupted.isEmpty)
    }
}
