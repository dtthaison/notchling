import XCTest
@testable import Notchling

/// Background jobs that run a shell command: finding where their output goes, and a shorter command line.
final class CommandJobTests: XCTestCase {
    func testOutputPath() {
        let sd = "cd /p && env HOME=/h PATH=/a:/b /venv/python3 /x/sd.py autopilot > /repo/.git/run.log 2>&1"
        XCTAssertEqual(CommandJob.outputPath(in: sd, cwd: "/p"), "/repo/.git/run.log")
        XCTAssertEqual(CommandJob.outputPath(in: "make test >> build.log", cwd: "/p"), "/p/build.log")
        XCTAssertEqual(CommandJob.outputPath(in: "run > 'my out.log' 2>&1", cwd: "/p"), "/p/my out.log")
        XCTAssertNil(CommandJob.outputPath(in: "run 2>&1", cwd: "/p"))
        XCTAssertNil(CommandJob.outputPath(in: "run > /dev/null", cwd: "/p"))
        XCTAssertNil(CommandJob.outputPath(in: "npm test", cwd: "/p"))
    }

    func testReadable() {
        XCTAssertEqual(CommandJob.readable("cd /p && env HOME=/h PATH=/a:/b python3 sd.py"), "cd /p && env … python3 sd.py")
    }

    func testTail() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("cmdjob-\(UUID().uuidString).log")
        defer { try? FileManager.default.removeItem(at: url) }
        try (1...300).map { "line \($0)" }.joined(separator: "\n").appending("\n").write(to: url, atomically: true, encoding: .utf8)
        let tail = try XCTUnwrap(CommandJob.tail(url.path, maxLines: 3))
        XCTAssertEqual(tail, "line 298\nline 299\nline 300")
    }
}
