import XCTest
@testable import Notchling

/// Is the process in a start record still that run: alive, and not a later process that got its pid.
final class RunProcessTests: XCTestCase {
    func testThisProcessIsAliveFromItsOwnStartOn() throws {
        let began = try XCTUnwrap(RunProcess.startTime(of: getpid()))
        XCTAssertTrue(RunProcess.isAlive(pid: getpid(), startedAt: Date()))
        XCTAssertTrue(RunProcess.isAlive(pid: getpid(), startedAt: began), "the record is written right after the process starts")
        XCTAssertTrue(RunProcess.isAlive(pid: getpid(), startedAt: began.addingTimeInterval(-RunProcess.identityTolerance + 0.5)))
    }

    func testAPidThatStartedAfterTheRunIsAnotherProcess() throws {
        let began = try XCTUnwrap(RunProcess.startTime(of: getpid()))
        XCTAssertFalse(RunProcess.isAlive(pid: getpid(), startedAt: began.addingTimeInterval(-RunProcess.identityTolerance - 1)),
                       "the pid was reused after the run began")
        XCTAssertFalse(RunProcess.isAlive(pid: getpid(), startedAt: Date().addingTimeInterval(-86400 * 3650)))
    }

    func testAProcessWeMayNotSignalIsNotAliveWithoutTheCheck() {
        // launchd belongs to root: kill(1, 0) says EPERM, which used to count as alive.
        XCTAssertFalse(RunProcess.isAlive(pid: 1, startedAt: Date(timeIntervalSince1970: 1_000_000_000)))
    }

    func testAnEndedProcessIsNotAlive() throws {
        let child = Process()
        child.executableURL = URL(fileURLWithPath: "/usr/bin/true")
        try child.run()
        let pid = child.processIdentifier
        let began = Date()
        child.waitUntilExit()
        XCTAssertFalse(RunProcess.isAlive(pid: pid, startedAt: began))
    }

    func testNoPidIsNeverAlive() {
        XCTAssertFalse(RunProcess.isAlive(pid: 0, startedAt: Date()))
        XCTAssertFalse(RunProcess.isAlive(pid: -5, startedAt: Date()))
    }
}
