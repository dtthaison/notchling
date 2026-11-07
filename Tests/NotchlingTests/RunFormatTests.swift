import XCTest
@testable import Notchling

final class RunFormatTests: XCTestCase {
    func testElapsedAndLimit() {
        XCTAssertEqual(RunFormat.duration(45), "45s")
        XCTAssertEqual(RunFormat.duration(32 * 60 + 10), "32m")
        XCTAssertEqual(RunFormat.duration(7200), "2h")
        XCTAssertEqual(RunFormat.duration(3600 + 5 * 60), "1h 5m")
        XCTAssertEqual(RunFormat.progress(elapsed: 32 * 60, limit: 7200), "32m / 2h")
        XCTAssertEqual(RunFormat.progress(elapsed: 32 * 60, limit: nil), "32m")
        XCTAssertEqual(RunFormat.progress(elapsed: -5, limit: nil), "0s")
    }

    func testExitText() {
        XCTAssertEqual(RunFormat.exit(0), "ok")
        XCTAssertEqual(RunFormat.exit(124), "stopped at limit")
        XCTAssertEqual(RunFormat.exit(2), "exit 2")
    }

    func testShortNameFromSDKeys() {
        XCTAssertEqual(RunFormat.shortName("caffeinate -i mise exec -- just --justfile apps/ios/justfile --set feature ios-voice-ux --set simulator platform=iOS Simulator,id=CC42 perf suite=coaching build=no"), "perf")
        XCTAssertEqual(RunFormat.shortName("just test-all"), "test-all")
        XCTAssertEqual(RunFormat.shortName("swift test --filter Foo"), "swift test")
        XCTAssertEqual(RunFormat.shortName("./scripts/check.sh --all"), "check.sh")
        XCTAssertEqual(RunFormat.shortName("  "), "run")
    }

    func testShortNameIsTruncated() {
        let long = RunFormat.shortName("just " + String(repeating: "x", count: 80))
        XCTAssertLessThanOrEqual(long.count, RunFormat.nameLimit)
        XCTAssertTrue(long.hasSuffix("…"))
    }

    func testLastLineStripsAnsiAndProgress() {
        let data = "building\n\u{1B}[32mPASS\u{1B}[0m 12 tests\nprogress 10%\rprogress 90%\r\n\n".data(using: .utf8)!
        XCTAssertEqual(RunLog.lastLine(in: data), "progress 90%")
        XCTAssertEqual(RunLog.lastLine(in: "a\n\u{1B}[32mPASS\u{1B}[0m ok\n".data(using: .utf8)!), "PASS ok")
    }

    func testLastLineIsCutAt80() {
        let line = String(repeating: "a", count: 200)
        let out = RunLog.lastLine(in: (line + "\n").data(using: .utf8)!)!
        XCTAssertEqual(out.count, 80)
        XCTAssertTrue(out.hasSuffix("…"))
    }

    func testLastLineOfNothing() {
        XCTAssertNil(RunLog.lastLine(in: Data()))
        XCTAssertNil(RunLog.lastLine(in: "\n  \n".data(using: .utf8)!))
    }

    func testLastLineReadsTheTailOfALargeFile() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("runlog-\(UUID().uuidString).log")
        defer { try? FileManager.default.removeItem(at: url) }
        let body = String(repeating: "filler line\n", count: 20_000) + "the end\n"
        try body.write(to: url, atomically: true, encoding: .utf8)
        XCTAssertEqual(RunLog.lastLine(of: url.path), "the end")
        XCTAssertNil(RunLog.lastLine(of: url.path + ".missing"))
    }

    // MARK: Git common dir

    var root: URL!
    override func setUp() {
        root = URL(fileURLWithPath: FileManager.default.temporaryDirectory.path).appendingPathComponent("gitcommon-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }
    override func tearDown() { try? FileManager.default.removeItem(at: root) }

    func mkdir(_ path: String) -> URL {
        let url = root.appendingPathComponent(path)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    func testCommonDirOfAPlainCheckout() {
        let repo = mkdir("repo")
        _ = mkdir("repo/.git")
        _ = mkdir("repo/apps/ios")
        XCTAssertEqual(GitCommonDir.resolve(cwd: repo.path + "/apps/ios"), repo.path + "/.git")
    }

    func testCommonDirOfAWorktreeFollowsCommondir() throws {
        let main = mkdir("main")
        _ = mkdir("main/.git/worktrees/wt")
        try "../..\n".write(to: main.appendingPathComponent(".git/worktrees/wt/commondir"), atomically: true, encoding: .utf8)
        let wt = mkdir("wt")
        try "gitdir: \(main.path)/.git/worktrees/wt\n".write(to: wt.appendingPathComponent(".git"), atomically: true, encoding: .utf8)
        XCTAssertEqual(GitCommonDir.resolve(cwd: wt.path), main.path + "/.git")
    }

    func testCommonDirOfAWorktreeWithoutCommondirFile() throws {
        let main = mkdir("m2")
        _ = mkdir("m2/.git/worktrees/wt")
        let wt = mkdir("wt2")
        try "gitdir: \(main.path)/.git/worktrees/wt".write(to: wt.appendingPathComponent(".git"), atomically: true, encoding: .utf8)
        XCTAssertEqual(GitCommonDir.resolve(cwd: wt.path), main.path + "/.git")
    }

    func testNoRepoNoCommonDir() {
        let plain = mkdir("plain/inner")
        XCTAssertNil(GitCommonDir.resolve(cwd: plain.path, stopAt: root.path))
        XCTAssertNil(GitCommonDir.resolve(cwd: ""))
    }
}
