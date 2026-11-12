import XCTest
@testable import Notchling

/// The checkouts on `machines.json` that the Run Watcher looks at. Reading them must not touch the file:
/// `MachineRegistry.load` migrates a v1 file (rewrites it and keeps a backup), which is the Machines window's business.
final class RunRepositoriesTests: XCTestCase {
    var dir: URL!
    var url: URL { dir.appendingPathComponent("machines.json") }

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("runrepos-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDown() { try? FileManager.default.removeItem(at: dir) }

    private let v1 = """
    [{"name": "m1", "host": "m1", "user": "sondoan", "python": "python3", "sdPath": "/x/sd.py", "fileVault": true,
      "repos": [{"github": "o/a", "localPath": "/Users/me/a", "checkCommand": "just check"},
                {"github": "o/b", "localPath": "/Users/me/b", "checkCommand": "just check"}]}]
    """

    private let v2 = """
    {"version": 2, "machines": [{"name": "vps", "host": "vps", "user": "agent", "os": "linux",
      "jobs": [{"kind": "sd-worker", "sdWorker": {"python": "python3", "sdPath": "/x/sd.py", "kinds": ["backend"],
        "issuesSecret": "vps-github-issues",
        "repos": [{"github": "o/c", "localPath": "/Users/me/c", "checkCommand": "just check"}]}}]}]}
    """

    private func files() -> [String] {
        ((try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []).sorted()
    }

    func testAV1FileIsReadAsItIsAndNeverMigrated() throws {
        try v1.write(to: url, atomically: true, encoding: .utf8)
        let before = try Data(contentsOf: url)
        XCTAssertEqual(RunWatcher.machineRepositories(at: url), ["/Users/me/a", "/Users/me/b"])
        XCTAssertEqual(try Data(contentsOf: url), before, "the file is byte for byte as it was")
        XCTAssertEqual(files(), ["machines.json"], "no v1-backup and no other file appears")
    }

    func testAV2FileIsReadAndLeftAlone() throws {
        try v2.write(to: url, atomically: true, encoding: .utf8)
        let before = try Data(contentsOf: url)
        XCTAssertEqual(RunWatcher.machineRepositories(at: url), ["/Users/me/c"])
        XCTAssertEqual(try Data(contentsOf: url), before)
        XCTAssertEqual(files(), ["machines.json"])
    }

    func testAMissingOrUnreadableFileHasNoRepositories() throws {
        XCTAssertEqual(RunWatcher.machineRepositories(at: url), [])
        try "{ not json".write(to: url, atomically: true, encoding: .utf8)
        XCTAssertEqual(RunWatcher.machineRepositories(at: url), [])
        XCTAssertEqual(files(), ["machines.json"])
    }

    func testPeekDecodesBothVersionsWithoutWriting() throws {
        try v1.write(to: url, atomically: true, encoding: .utf8)
        XCTAssertEqual(MachineRegistry.peek(at: url).map(\.name), ["m1"])
        XCTAssertEqual(files(), ["machines.json"])
        try v2.write(to: url, atomically: true, encoding: .utf8)
        XCTAssertEqual(MachineRegistry.peek(at: url).map(\.name), ["vps"])
        XCTAssertEqual(MachineRegistry.peek(at: dir.appendingPathComponent("nope.json")), [])
    }
}
