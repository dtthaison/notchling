import XCTest
@testable import ConsoleCore

final class SpawnTests: XCTestCase {
    func testScrubbingRemovesEveryClaudeCodeVariable() {
        let scrubbed = SpawnEnvironment.scrubbed([
            "PATH": "/usr/bin", "HOME": "/Users/x", "ANTHROPIC_API_KEY": "k",
            "CLAUDE_CODE_CHILD_SESSION": "1", "CLAUDE_CODE_ENTRYPOINT": "cli", "CLAUDE_CODE_SSE_PORT": "1234",
            "CLAUDECODE": "1", "CLAUDE_CONFIG_DIR": "/cfg",
        ])
        XCTAssertFalse(scrubbed.keys.contains { $0.hasPrefix("CLAUDE_CODE_") })
        XCTAssertNil(scrubbed["CLAUDECODE"])
        XCTAssertEqual(scrubbed["PATH"], "/usr/bin")
        XCTAssertEqual(scrubbed["HOME"], "/Users/x")
        XCTAssertEqual(scrubbed["ANTHROPIC_API_KEY"], "k")
        XCTAssertEqual(scrubbed["CLAUDE_CONFIG_DIR"], "/cfg", "only the CLAUDE_CODE_ prefix is scrubbed")
        XCTAssertEqual(scrubbed["TERM"], "xterm-256color")
    }

    func testParsingNulSeparatedEnvironment() {
        let data = Data("A=1\0B=two=2\0\0NOEQUALS\0".utf8)
        XCTAssertEqual(SpawnEnvironment.parse(data), ["A": "1", "B": "two=2"])
    }

    func testSpawnedChildRunsInTheFolderWithScrubbedEnvironment() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("spawn-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }

        let environment = SpawnEnvironment.scrubbed(["PATH": "/usr/bin:/bin", "CLAUDE_CODE_CHILD_SESSION": "1"])
        let pty = try PTYSession.spawn(executable: URL(fileURLWithPath: "/bin/sh"),
                                       arguments: ["-c", "pwd -P; env | grep -c CLAUDE_CODE_ || true"],
                                       cwd: directory, model: nil, environment: environment)
        let collected = Collected()
        let finished = expectation(description: "exit")
        pty.addListener { collected.append($0) }
        pty.onExit { finished.fulfill() }
        wait(for: [finished], timeout: 5)

        let output = collected.text
        XCTAssertTrue(output.contains(directory.resolvingSymlinksInPath().path), output)
        XCTAssertTrue(output.contains("0"), "no CLAUDE_CODE_ variable reaches the child: \(output)")
    }

    func testClaudeExecutableLookupFailsClearlyWhenMissing() {
        let empty = FileManager.default.temporaryDirectory.appendingPathComponent("no-home-\(UUID().uuidString)")
        XCTAssertThrowsError(try ClaudeExecutable.resolve(home: empty)) { error in
            guard case SendError.failed = error else { return XCTFail("\(error)") }
        }
    }
}
