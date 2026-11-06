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

    // MARK: stdin, streaming, the cap, a lost status, orphans of a normal exit

    private final class Chunks: @unchecked Sendable {
        private let lock = NSLock()
        private var items: [(Date, String)] = []
        func add(_ s: String) { lock.withLock { items.append((Date(), s)) } }
        var all: [(Date, String)] { lock.withLock { items } }
    }

    func testStdinReachesTheCommand() async {
        let r = await ProcessRunner().run(["/bin/cat"], cwd: nil, env: [:], stdin: Data("line one\nline two".utf8),
                                          timeout: 5, onOutput: nil)
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out, "line one\nline two")
    }

    func testAChildThatExitsWithoutReadingStdinDoesNotHang() async {
        let start = Date()
        let r = await ProcessRunner().run(["/usr/bin/true"], cwd: nil, env: [:],
                                          stdin: Data(count: 4 * 1024 * 1024), timeout: 20, onOutput: nil)
        XCTAssertEqual(r.code, 0)
        XCTAssertLessThan(Date().timeIntervalSince(start), 5)
    }

    func testAFullStdoutPipeDoesNotBlockTheStdinWrite() async {
        // cat echoes 4 MiB back while we are still writing it; the output is capped at 2 MiB
        let big = Data(repeating: 0x61, count: 4 * 1024 * 1024)
        let r = await ProcessRunner().run(["/bin/cat"], cwd: nil, env: [:], stdin: big, timeout: 20, onOutput: nil)
        XCTAssertEqual(r.code, 0)
        XCTAssertTrue(r.out.hasPrefix("[earlier output cut]\n"))
        XCTAssertTrue(r.out.hasSuffix("aaaa"))
    }

    func testOutputStreamsWhileTheCommandRuns() async {
        let chunks = Chunks()
        let r = await ProcessRunner().run(["/bin/sh", "-c", "echo one; sleep 1; echo two"], cwd: nil, env: [:],
                                          stdin: nil, timeout: 10, onOutput: { chunks.add($0) })
        let ended = Date()
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(r.out, "one\ntwo\n")
        let first = chunks.all.first { $0.1.contains("one") }
        XCTAssertNotNil(first)
        if let first { XCTAssertGreaterThanOrEqual(ended.timeIntervalSince(first.0), 0.7) }
        XCTAssertEqual(chunks.all.map(\.1).joined(), "one\ntwo\n")
    }

    func testAMultiByteCharacterSplitAcrossReadsIsNotBroken() async {
        // "é" is two bytes; the shell writes the first byte, waits, then the second
        let chunks = Chunks()
        let r = await ProcessRunner().run(["/bin/sh", "-c", "printf '\\303'; sleep 0.5; printf '\\251'"], cwd: nil, env: [:],
                                          stdin: nil, timeout: 10, onOutput: { chunks.add($0) })
        XCTAssertEqual(r.out, "\u{e9}")
        XCTAssertEqual(chunks.all.map(\.1).joined(), "\u{e9}")
    }

    func testOutputIsCappedAtTwoMiBPerPipe() async {
        let r = await ProcessRunner().run(["/bin/sh", "-c", "head -c 3000000 /dev/zero | tr \"\\000\" a"], cwd: nil,
                                          timeout: 20)
        XCTAssertLessThanOrEqual(r.out.utf8.count, 2 * 1024 * 1024 + 32)
        XCTAssertTrue(r.out.hasPrefix("[earlier output cut]\n"))
        XCTAssertTrue(r.out.hasSuffix("aaaa"))
    }

    func testOrphansOfANormalExitAreEnded() async {
        let marker = "30.6481"
        let r = await ProcessRunner().run(["/bin/sh", "-c", "sleep \(marker) & exit 0"], cwd: nil, timeout: 10)
        let leftover = pgrep(marker)
        pkill(marker)
        XCTAssertEqual(r.code, 0)
        XCTAssertFalse(leftover, "an orphan of a finished command is still running")
    }

    func testALostExitStatusIsNotSuccess() async {
        // with SIGCHLD ignored the kernel reaps the child itself, and waitpid answers ECHILD
        let old = signal(SIGCHLD, SIG_IGN)
        defer { signal(SIGCHLD, old) }
        let r = await ProcessRunner().run(["/usr/bin/true"], cwd: nil, timeout: 5)
        XCTAssertEqual(r.code, ProcessRunner.lostStatus)
        XCTAssertEqual(r.code, 125)
        XCTAssertTrue(r.err.contains("lost the exit status"), r.err)
    }

    private struct OldRunner: CommandRunning {
        func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
            CommandResult(code: 0, out: "old", err: "")
        }
    }

    func testARunnerWithoutStdinSupportRefusesInsteadOfDroppingIt() async {
        let withStdin = await OldRunner().run(["x"], cwd: nil, env: [:], stdin: Data("secret".utf8), timeout: 1, onOutput: nil)
        XCTAssertEqual(withStdin.code, 127)
        XCTAssertFalse(withStdin.out.contains("secret") || withStdin.err.contains("secret"))
        let withStream = await OldRunner().run(["x"], cwd: nil, env: [:], stdin: nil, timeout: 1, onOutput: { _ in })
        XCTAssertEqual(withStream.code, 127)
        let plain = await OldRunner().run(["x"], cwd: nil, env: [:], stdin: nil, timeout: 1, onOutput: nil)
        XCTAssertEqual(plain.code, 0)
    }

    func testAStdinValueIsNotInArgvEnvironmentOrResult() async {
        let secret = "s3cr3t-value-xyz"
        let r = await ProcessRunner().run(["/bin/sh", "-c", "cat >/dev/null; env; echo \"$0 $*\""], cwd: nil, env: [:],
                                          stdin: Data(secret.utf8), timeout: 5, onOutput: nil)
        XCTAssertEqual(r.code, 0)
        XCTAssertFalse(r.out.contains(secret))
        XCTAssertFalse(r.err.contains(secret))
    }
}
