import XCTest
@testable import Notchling

/// A background session that entered a git worktree goes on writing in the worktree's project folder,
/// while its state.json still names the file it started in.
final class JobTranscriptTests: XCTestCase {
    private let start = "/h/.claude/projects/-Users-me-app/abc.jsonl"
    private let moved = "/h/.claude/projects/-Users-me-app--claude-worktrees-music/abc.jsonl"

    func testFollowsTheSessionIntoItsWorktree() {
        let found = TranscriptReader.jobTranscript(linkScanPath: start, sessionID: "abc",
                                                   worktreePath: "/Users/me/app/.claude/worktrees/music",
                                                   modified: { $0 == self.moved ? Date() : nil })
        XCTAssertEqual(found, moved)
    }

    func testTheNewerFileWinsWhenBothExist() {
        let dates = [start: Date(timeIntervalSince1970: 100), moved: Date(timeIntervalSince1970: 200)]
        XCTAssertEqual(TranscriptReader.jobTranscript(linkScanPath: start, sessionID: "abc",
                                                      worktreePath: "/Users/me/app/.claude/worktrees/music",
                                                      modified: { dates[$0] }), moved)
        let older = [start: Date(timeIntervalSince1970: 300), moved: Date(timeIntervalSince1970: 200)]
        XCTAssertEqual(TranscriptReader.jobTranscript(linkScanPath: start, sessionID: "abc",
                                                      worktreePath: "/Users/me/app/.claude/worktrees/music",
                                                      modified: { older[$0] }), start)
    }

    func testWithoutAWorktreeItIsTheRecordedPath() {
        XCTAssertEqual(TranscriptReader.jobTranscript(linkScanPath: start, sessionID: "abc", worktreePath: nil,
                                                      modified: { _ in nil }), start)
        XCTAssertNil(TranscriptReader.jobTranscript(linkScanPath: nil, sessionID: "abc", worktreePath: "/w",
                                                    modified: { _ in Date() }))
    }
}
