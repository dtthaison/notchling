import XCTest
@testable import Notchling

/// The real `ProcessRunner`, with harmless local commands only: the environment a check runs in, and what a
/// timeout leaves behind.
final class MachinesRunnerTests: XCTestCase {
    private func pgrep(_ pattern: String) -> Bool {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
        p.arguments = ["-f", pattern]
        p.standardOutput = FileHandle.nullDevice
        try? p.run()
        p.waitUntilExit()
        return p.terminationStatus == 0
    }

    private func pkill(_ pattern: String) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        p.arguments = ["-f", pattern]
        try? p.run()
        p.waitUntilExit()
    }

    func testEnvironmentIsLaidOverTheAppsOwn() async {
        let r = await ProcessRunner().run(["/bin/sh", "-c", "printf '%s|%s' \"$MACHINES_TEST_VAR\" \"$HOME\""],
                                          cwd: nil, env: ["MACHINES_TEST_VAR": "from-the-caller"], timeout: 5)
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out, "from-the-caller|" + (ProcessInfo.processInfo.environment["HOME"] ?? ""))
    }

    func testNoEnvironmentOverrideKeepsTheAppsOwn() async {
        let r = await ProcessRunner().run(["/bin/sh", "-c", "printf '%s' \"$HOME\""], cwd: nil, timeout: 5)
        XCTAssertEqual(r.out, ProcessInfo.processInfo.environment["HOME"])
    }

    func testCwdIsUsed() async {
        let r = await ProcessRunner().run(["/bin/pwd", "-P"], cwd: "/usr", timeout: 5)
        XCTAssertEqual(r.out, "/usr\n")
    }

    func testAStartThatFailsIs127() async {
        let r = await ProcessRunner().run(["/bin/pwd"], cwd: "/nonexistent/dir/here", timeout: 5)
        XCTAssertEqual(r.code, 127)
    }

    func testTimeoutKillsTheWholeProcessGroup() async {
        let marker = "sleep 30.8217"
        let start = Date()
        let r = await ProcessRunner().run(["/bin/sh", "-c", "\(marker) & \(marker)"], cwd: nil, timeout: 0.5)
        XCTAssertEqual(r.code, 124)
        XCTAssertLessThan(Date().timeIntervalSince(start), 5, "the run waited for the sleeps to end")
        XCTAssertFalse(pgrep(marker), "a sleep outlived its timeout")
        pkill(marker)
    }

    func testTimeoutKillsAGroupThatIgnoresTerm() async {
        let marker = "sleep 30.6419"
        let start = Date()
        let r = await ProcessRunner().run(["/bin/sh", "-c", "trap '' TERM; \(marker) & \(marker)"], cwd: nil, timeout: 0.5)
        XCTAssertEqual(r.code, 124)
        XCTAssertLessThan(Date().timeIntervalSince(start), 8)
        XCTAssertFalse(pgrep(marker), "a sleep that ignores SIGTERM outlived its timeout")
        pkill(marker)
    }

    func testAnOrphanHoldingThePipeDoesNotHoldTheResult() async {
        let marker = "sleep 12.3071"
        let start = Date()
        let r = await ProcessRunner().run(["/bin/sh", "-c", "(\(marker) &) ; echo done"], cwd: nil, timeout: 60)
        pkill(marker)
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out, "done\n")
        XCTAssertLessThan(Date().timeIntervalSince(start), 8)
    }
}
