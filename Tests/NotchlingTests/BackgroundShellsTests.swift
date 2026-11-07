import XCTest
@testable import Notchling

final class BackgroundShellsTests: XCTestCase {
    func testLaunchReadsIDAndPath() {
        let text = "Command running in background with ID: bj5rd6y90. Output is being written to: "
            + "/private/tmp/claude-501/-Users-x/38b0/tasks/bj5rd6y90.output. You will be notified when it completes."
        let launch = BackgroundShells.launch(in: text)
        XCTAssertEqual(launch?.id, "bj5rd6y90")
        XCTAssertEqual(launch?.path, "/private/tmp/claude-501/-Users-x/38b0/tasks/bj5rd6y90.output")
    }

    func testLaunchIgnoresOrdinaryOutput() {
        XCTAssertNil(BackgroundShells.launch(in: "hello\nworld"))
        XCTAssertNil(BackgroundShells.launch(in: "Command running in background with ID: abc."))
    }

    /// Real `lsof -F pca` output for a background `sleep` loop: zsh and sleep write; a reader and
    /// Claude Code itself must never be stopped.
    func testWritersSkipReadersAndClaude() {
        let text = "p4010\nczsh\nf1\naw\nf2\naw\np4011\ncsleep\nf1\naw\np500\ncclaude\nf9\naw\np600\nctail\nf3\nar\n"
        XCTAssertEqual(BackgroundShells.parseWriters(text), [4010, 4011])
    }

    func testStateUsesNotificationFirst() {
        let shell = BackgroundShell(id: "b1", command: "sleep 9", title: "t", outputPath: "/nonexistent/b1.output", started: nil)
        XCTAssertEqual(BackgroundShells.state(shell, finished: ["b1": "killed"]), .ended("killed"))
        XCTAssertEqual(BackgroundShells.state(shell, finished: [:]), .ended("ended"))
    }
}
