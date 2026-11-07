import XCTest
@testable import Notchling

/// Following a background job's transcript across /clear, and keeping the answer until the folder changes.
final class SuccessorTests: XCTestCase {
    var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-successor-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
    }

    func write(_ name: String, _ text: String) throws -> String {
        let url = dir.appendingPathComponent(name)
        try text.write(to: url, atomically: false, encoding: .utf8)
        return url.path
    }

    func cleared(by agent: String) -> String {
        #"{"type":"system","agentName":"\#(agent)","content":"SessionStart:clear"}"# + "\n"
    }

    /// Folder dates have sub-second resolution, but give the file system a moment so they surely differ.
    func tick() { Thread.sleep(forTimeInterval: 0.05) }

    func testFindsTheTranscriptStartedByClear() throws {
        let old = try write("old.jsonl", "{}\n")
        tick()
        let new = try write("new.jsonl", cleared(by: "worker"))
        XCTAssertEqual(TranscriptReader.successor(of: old, name: "worker"), new)
        XCTAssertNil(TranscriptReader.successor(of: old, name: "someone-else"))
    }

    func testANewFileIsSeenAfterACachedMiss() throws {
        let old = try write("old.jsonl", "{}\n")
        XCTAssertNil(TranscriptReader.successor(of: old, name: "worker"))
        tick()
        let new = try write("new.jsonl", cleared(by: "worker"))
        XCTAssertEqual(TranscriptReader.successor(of: old, name: "worker"), new, "creating a file must invalidate the cache")
    }

    func testAppendingDoesNotRescan() throws {
        let old = try write("old.jsonl", "{}\n")
        XCTAssertNil(TranscriptReader.successor(of: old, name: "worker"))
        let before = try FileManager.default.attributesOfItem(atPath: dir.path)[.modificationDate] as? Date
        let handle = try XCTUnwrap(FileHandle(forWritingAtPath: old))
        handle.seekToEndOfFile()
        handle.write(Data("{\"more\":1}\n".utf8))
        try handle.close()
        let after = try FileManager.default.attributesOfItem(atPath: dir.path)[.modificationDate] as? Date
        XCTAssertEqual(before, after, "the cache relies on appends leaving the folder date alone")
    }

    func testRemovedSuccessorIsDropped() throws {
        let old = try write("old.jsonl", "{}\n")
        tick()
        let new = try write("new.jsonl", cleared(by: "worker"))
        XCTAssertEqual(TranscriptReader.successor(of: old, name: "worker"), new)
        tick()
        try FileManager.default.removeItem(atPath: new)
        XCTAssertNil(TranscriptReader.successor(of: old, name: "worker"))
    }
}

final class ProjectFolderTests: XCTestCase {
    /// Checked against real folder names in ~/.claude/projects.
    func testFolderNames() {
        XCTAssertEqual(TranscriptReader.projectFolder(for: "/Users/sondoan/go/src/pp/notchling"), "-Users-sondoan-go-src-pp-notchling")
        XCTAssertEqual(TranscriptReader.projectFolder(for: "/Users/sondoan/.claude/jobs/45ee33db/tmp"), "-Users-sondoan--claude-jobs-45ee33db-tmp")
        XCTAssertEqual(TranscriptReader.projectFolder(for: "/private/tmp/claude-501/x_y"), "-private-tmp-claude-501-x-y")
    }
}
