import XCTest
@testable import Notchling

/// Paths and commands in a message, against a made-up disk.
final class ChatPathsTests: XCTestCase {
    let home = FileManager.default.homeDirectoryForCurrentUser.path
    var edit: String { home + "/Downloads/aptusfit-reel/edit" }
    var disk: Set<String> { [edit, edit + "/src", edit + "/src/timeline.ts", edit + "/src/index.ts", "/repo", "/repo/Sources"] }
    var exists: ChatPaths.Exists { { self.disk.contains($0) } }

    let message = """
    The Remotion project is in ~/Downloads/aptusfit-reel/edit/. To change the timing or copy, edit `src/timeline.ts`, then re-render from that folder.
    """

    func testFoldersNamedInTheMessageComeBeforeTheSessions() {
        XCTAssertEqual(ChatPaths.bases(in: message, cwd: "/repo", isFolder: exists), [edit, "/repo"])
    }

    func testPathsThatExistBecomeLinks() {
        let links = ChatPaths.links(in: message, bases: [edit, "/repo"], exists: exists)
        XCTAssertEqual(links.map(\.1), [edit, edit + "/src/timeline.ts"], "the trailing period isn't part of the path")
        let ns = message as NSString
        XCTAssertEqual(ns.substring(with: links[0].0), "~/Downloads/aptusfit-reel/edit/")
        XCTAssertEqual(ns.substring(with: links[1].0), "src/timeline.ts")
    }

    func testMissingPathsAndURLsStayText() {
        let text = "Wrote out/aptusfit-reel-v2.mp4, see https://claude.ai/artifact/x and/or later."
        XCTAssertTrue(ChatPaths.links(in: text, bases: [edit], exists: exists).isEmpty)
    }

    func testCommandRunsWhereItsFilesAre() {
        let cmd = "npx remotion render src/index.ts AptusReel out/aptusfit-reel-v2.mp4 --codec h264 --crf 16 --overwrite"
        XCTAssertEqual(ChatPaths.directory(for: cmd, bases: [edit, "/repo"], exists: exists), edit)
        XCTAssertEqual(ChatPaths.directory(for: "swift test", bases: [edit, "/repo"], exists: exists), "/repo")
        XCTAssertEqual(ChatPaths.directory(for: "cd src && ls", bases: [edit, "/repo"], exists: exists), "/repo")
    }

    /// A code block's Run: the user's zsh in a pty, in the chosen folder, with its exit status.
    func testRunsACommandInAFolder() {
        let session = TerminalSession(run: "pwd; echo made-by-run; exit 3", in: "/private/tmp", jobID: "")
        session.begin(rows: 20, cols: 80)
        let deadline = Date().addingTimeInterval(20)
        while session.exitStatus == nil, Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.05)) }
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        let text = session.screen.lines.joined(separator: "\n")
        XCTAssertEqual(session.exitStatus, 3, text)
        XCTAssertTrue(text.contains("$ pwd; echo made-by-run"), text)
        XCTAssertTrue(text.contains("/private/tmp"), text)
        XCTAssertTrue(session.screen.lines.contains("made-by-run"), text)
        session.close()
    }

    func testInlineCodeCommands() throws {
        let program: (String) -> Bool = { $0 == "npx" }
        let cmd = "npx remotion render src/index.ts AptusReel out/aptusfit-reel-v2.mp4 --codec h264 --crf 16 --overwrite"
        XCTAssertEqual(ChatPaths.inlineCommand(cmd, isProgram: program), cmd)
        XCTAssertNil(ChatPaths.inlineCommand("npx", isProgram: program), "a bare name is a mention, not a command")
        XCTAssertNil(ChatPaths.inlineCommand("src/timeline.ts", isProgram: program))
        let link = try XCTUnwrap(ChatPaths.runLink(cmd + " && echo 'a&b?'", folder: edit))
        let back = try XCTUnwrap(ChatPaths.parseRunLink(link))
        XCTAssertEqual(back.command, cmd + " && echo 'a&b?'")
        XCTAssertEqual(back.folder, edit)
    }

    func testWhichBlocksAreCommands() {
        let program: (String) -> Bool = { ["npx", "git"].contains($0) }
        XCTAssertEqual(ChatPaths.command(language: "bash", text: "$ npx remotion render\n", isProgram: program), "npx remotion render")
        XCTAssertEqual(ChatPaths.command(language: "", text: "npx remotion render src/index.ts", isProgram: program), "npx remotion render src/index.ts")
        XCTAssertNil(ChatPaths.command(language: "", text: "let x = 1", isProgram: program))
        XCTAssertNil(ChatPaths.command(language: "swift", text: "git status", isProgram: program))
        XCTAssertNil(ChatPaths.command(language: "console", text: "total 8\ndrwxr-xr-x", isProgram: program), "output, not a command")
    }
}
