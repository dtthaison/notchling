import XCTest
@testable import Notchling

/// Skipped unless asked for:
/// `BENCH_TRANSCRIPT=<a large .jsonl> swift test -c release --filter ZBenchmarkTests`
/// November 2026, a 5.6 MB transcript: 24.8 ms per refresh re-reading the tail, 0.39 ms appending.
final class ZBenchmarkTests: XCTestCase {
    func testFullReadVersusAppend() throws {
        guard let source = ProcessInfo.processInfo.environment["BENCH_TRANSCRIPT"] else { throw XCTSkip("set BENCH_TRANSCRIPT") }
        let fm = FileManager.default
        let dir = fm.temporaryDirectory.appendingPathComponent("bench-\(UUID().uuidString)")
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: dir) }
        let rounds = 20

        // Before: every change re-read the 2 MB tail. A path no cursor has seen does the same.
        var full = 0.0
        for i in 0..<rounds {
            let copy = dir.appendingPathComponent("full-\(i).jsonl").path
            try fm.copyItem(atPath: source, toPath: copy)
            let start = Date()
            _ = TranscriptReader.snapshot(atPath: copy)
            full += Date().timeIntervalSince(start)
        }

        // Now: one new line, read from the cursor.
        let live = dir.appendingPathComponent("live.jsonl").path
        try fm.copyItem(atPath: source, toPath: live)
        _ = TranscriptReader.snapshot(atPath: live)
        let line = Data(#"{"type":"assistant","message":{"content":[{"type":"text","text":"tick"}]}}"#.utf8 + [10])
        var append = 0.0
        for _ in 0..<rounds {
            let handle = try XCTUnwrap(FileHandle(forWritingAtPath: live))
            handle.seekToEndOfFile()
            handle.write(line)
            try handle.close()
            let start = Date()
            _ = TranscriptReader.snapshot(atPath: live)
            append += Date().timeIntervalSince(start)
        }
        print(String(format: "BENCH full re-read: %.2f ms per refresh, append-only: %.3f ms per refresh",
                     full / Double(rounds) * 1000, append / Double(rounds) * 1000))
    }
}
