import XCTest
@testable import Notchling

/// The reader keeps a bounded number of cursors and drops the one read longest ago, not all of them.
final class TranscriptCursorsTests: TranscriptFileTestCase {
    struct LineCount: TranscriptLineParser {
        var lines = 0
        mutating func consume(_ line: Data) { lines += 1 }
    }

    func makeFile(_ name: String, lines: Int = 1) throws -> String {
        let path = dir.appendingPathComponent(name).path
        try Data(String(repeating: "{}\n", count: lines).utf8).write(to: URL(fileURLWithPath: path))
        return path
    }

    func testFullCacheDropsOnlyTheLeastRecentlyRead() throws {
        let a = try makeFile("a.jsonl"), b = try makeFile("b.jsonl"), c = try makeFile("c.jsonl")
        var renders = 0
        let cursors = TranscriptCursors<LineCount, Int>(tailBytes: 1_000_000, limit: 2) { renders += 1; return $0.lines }

        XCTAssertEqual(cursors.read(a), 1)
        XCTAssertEqual(cursors.read(b), 1)
        XCTAssertEqual(cursors.read(a), 1)   // a is now newer than b
        XCTAssertEqual(renders, 2)
        XCTAssertEqual(cursors.read(c), 1)   // full: b goes, a stays
        XCTAssertEqual(renders, 3)
        XCTAssertEqual(cursors.read(a), 1)
        XCTAssertEqual(renders, 3, "a was kept")
        XCTAssertEqual(cursors.read(b), 1)
        XCTAssertEqual(renders, 4, "b was dropped and read again")
    }

    func testReplacingAFileInAFullCacheDropsNothingElse() throws {
        let a = try makeFile("a.jsonl"), b = try makeFile("b.jsonl")
        var renders = 0
        let cursors = TranscriptCursors<LineCount, Int>(tailBytes: 1_000_000, limit: 2) { renders += 1; return $0.lines }
        _ = cursors.read(a)
        _ = cursors.read(b)
        // A new file at a's path (new inode) is read from scratch under the same key.
        try FileManager.default.removeItem(atPath: a)
        _ = try makeFile("a.jsonl", lines: 2)
        XCTAssertEqual(cursors.read(a), 2)
        XCTAssertEqual(renders, 3)
        XCTAssertEqual(cursors.read(b), 1)
        XCTAssertEqual(renders, 3, "b was kept")
    }
}
