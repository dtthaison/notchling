import XCTest
@testable import Notchling

/// The real `ProcessRunner`, with harmless local commands only: the environment a check runs in, and what a
/// timeout leaves behind.
final class MachinesRunnerTests: XCTestCase {
    /// Whether the process is running. A zombie (ended, waiting to be reaped) is not.
    private func isRunning(_ pid: pid_t) -> Bool {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/ps")
        p.arguments = ["-o", "stat=", "-p", String(pid)]
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = FileHandle.nullDevice
        do { try p.run() } catch { return false }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        let stat = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        return !stat.isEmpty && !stat.hasPrefix("Z")
    }

    /// The pids of `pids` still running after they had up to `seconds` to go.
    private func stillRunning(_ pids: [pid_t], within seconds: TimeInterval = 5) -> [pid_t] {
        let end = Date().addingTimeInterval(seconds)
        var alive = pids.filter(isRunning)
        while !alive.isEmpty && Date() < end {
            Thread.sleep(forTimeInterval: 0.1)
            alive = alive.filter(isRunning)
        }
        return alive
    }

    /// What a test script wrote about its own processes. The tests find their children by these pids and never by
    /// a process name, which another `swift test` run on the same machine can share.
    private struct Spawned {
        var shell: pid_t?
        var children: [(pid: pid_t, group: pid_t?)] = []
        var all: [pid_t] { (shell.map { [$0] } ?? []) + children.map(\.pid) }
    }

    /// `shell <pid>` and `child <pid> [<process group>]` lines, as the scripts below write them.
    private func spawned(_ file: URL) -> Spawned {
        var found = Spawned()
        let text = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
        for line in text.split(separator: "\n") {
            let words = line.split(separator: " ").map(String.init)
            if words.count == 2, words[0] == "shell", let pid = pid_t(words[1]) { found.shell = pid }
            if words.count >= 2, words[0] == "child", let pid = pid_t(words[1]) {
                found.children.append((pid, words.count > 2 ? pid_t(words[2]) : nil))
            }
        }
        return found
    }

    private func pidFile() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("runner-test-\(UUID().uuidString).pids")
    }

    /// Writes the shell's pid, then starts two sleeps in the background and writes each one's pid and process group.
    private func twoSleeps(_ file: URL, prefix: String = "") -> String {
        let f = "'\(file.path)'"
        let note = #"echo "child $! $(ps -o pgid= -p $! | tr -d ' ')""#
        return "\(prefix)echo \"shell $$\" >> \(f); sleep 30 & \(note) >> \(f); sleep 30 & \(note) >> \(f); wait"
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
        let file = pidFile()
        defer { try? FileManager.default.removeItem(at: file) }
        let start = Date()
        let r = await ProcessRunner().run(["/bin/sh", "-c", twoSleeps(file)], cwd: nil, timeout: 2)
        let seen = spawned(file)
        defer { seen.all.forEach { kill($0, SIGKILL) } }
        XCTAssertEqual(r.code, 124)
        XCTAssertLessThan(Date().timeIntervalSince(start), 10, "the run waited for the sleeps to end")
        XCTAssertEqual(seen.children.count, 2, "both sleeps started")
        // the sleeps are in the group the runner made for the shell, so a kill of the group reaches them
        XCTAssertNotNil(seen.shell)
        XCTAssertTrue(seen.children.allSatisfy { $0.group == seen.shell }, "a sleep is outside the shell's group: \(seen)")
        XCTAssertEqual(stillRunning(seen.all), [], "a process of the group outlived its timeout")
    }

    func testTimeoutKillsAGroupThatIgnoresTerm() async {
        let file = pidFile()
        defer { try? FileManager.default.removeItem(at: file) }
        let start = Date()
        let r = await ProcessRunner().run(["/bin/sh", "-c", twoSleeps(file, prefix: "trap '' TERM; ")], cwd: nil, timeout: 2)
        let seen = spawned(file)
        defer { seen.all.forEach { kill($0, SIGKILL) } }
        XCTAssertEqual(r.code, 124)
        XCTAssertLessThan(Date().timeIntervalSince(start), 12)
        XCTAssertEqual(seen.children.count, 2, "both sleeps started")
        XCTAssertNotNil(seen.shell)
        XCTAssertTrue(seen.children.allSatisfy { $0.group == seen.shell }, "a sleep is outside the shell's group: \(seen)")
        // SIGTERM is ignored by the shell and (inherited) by its sleeps: only the kill that follows ends them
        XCTAssertEqual(stillRunning(seen.all), [], "a process that ignores SIGTERM outlived its timeout")
    }

    func testAnOrphanHoldingThePipeDoesNotHoldTheResult() async {
        let file = pidFile()
        defer { try? FileManager.default.removeItem(at: file) }
        let start = Date()
        let r = await ProcessRunner().run(["/bin/sh", "-c", "(sleep 12 & echo \"child $!\" >> '\(file.path)') ; echo done"],
                                          cwd: nil, timeout: 60)
        spawned(file).all.forEach { kill($0, SIGKILL) }
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
        let file = pidFile()
        defer { try? FileManager.default.removeItem(at: file) }
        let r = await ProcessRunner().run(["/bin/sh", "-c", "sleep 30 & echo \"child $!\" >> '\(file.path)'; exit 0"],
                                          cwd: nil, timeout: 10)
        let seen = spawned(file)
        defer { seen.all.forEach { kill($0, SIGKILL) } }
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(seen.children.count, 1, "the sleep started")
        XCTAssertEqual(stillRunning(seen.all), [], "an orphan of a finished command is still running")
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
