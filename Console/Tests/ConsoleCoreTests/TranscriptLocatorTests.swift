import XCTest
@testable import ConsoleCore

final class TranscriptLocatorTests: XCTestCase {
    func testEncodingReplacesEveryNonAlphanumericCharacter() {
        XCTAssertEqual(TranscriptLocator.encode(cwd: "/Users/x/Documents/app"), "-Users-x-Documents-app")
        XCTAssertEqual(TranscriptLocator.encode(cwd: "/private/tmp/my.proj"), "-private-tmp-my-proj")
        XCTAssertEqual(TranscriptLocator.encode(cwd: "/Users/x/.hidden/a_b c"), "-Users-x--hidden-a-b-c")
    }

    func testTmpAndPrivateTmpResolveToTheSameDirectory() throws {
        let tree = try makeFixtures()
        let locator = TranscriptLocator(projectsDir: tree.paths.projectsDir)
        for cwd in ["/private/tmp/my.proj", "/tmp/my.proj"] {
            let url = locator.locate(sessionId: "sess-tmp", cwd: cwd)
            XCTAssertEqual(url?.deletingLastPathComponent().lastPathComponent, "-private-tmp-my-proj", cwd)
        }
    }

    func testDirectHitUsesTheEncodedCwd() throws {
        let tree = try makeFixtures()
        let locator = TranscriptLocator(projectsDir: tree.paths.projectsDir)
        let url = locator.locate(sessionId: "sess-busy", cwd: "/work/alpha")
        XCTAssertEqual(url?.deletingLastPathComponent().lastPathComponent, "-work-alpha")
    }

    func testFallsBackToScanningProjectDirectories() throws {
        let tree = try makeFixtures()
        let locator = TranscriptLocator(projectsDir: tree.paths.projectsDir)
        let url = locator.locate(sessionId: "sess-idle", cwd: "/work/beta")
        XCTAssertEqual(url?.deletingLastPathComponent().lastPathComponent, "-some-other-dir")
    }

    func testMissingTranscriptIsNilAndRetriedAfterTheInterval() throws {
        let tree = try makeFixtures()
        var clock = fixtureNow
        let locator = TranscriptLocator(projectsDir: tree.paths.projectsDir, now: { clock })
        XCTAssertNil(locator.locate(sessionId: "sess-late", cwd: "/work/late"))

        let directory = tree.paths.projectsDir.appendingPathComponent("-work-late")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try Data("{}\n".utf8).write(to: directory.appendingPathComponent("sess-late.jsonl"))

        XCTAssertNil(locator.locate(sessionId: "sess-late", cwd: "/work/late"), "still inside the retry interval")
        clock = fixtureNow.addingTimeInterval(6)
        XCTAssertNotNil(locator.locate(sessionId: "sess-late", cwd: "/work/late"))
    }
}
