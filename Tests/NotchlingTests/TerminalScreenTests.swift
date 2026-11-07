import XCTest
@testable import Notchling

/// The terminal screen against bytes recorded from `claude attach` on 2.1.318 (30×100): the session,
/// then "/plugin" typed, Enter (the plugin menu), Down, Esc.
final class TerminalScreenTests: XCTestCase {
    func bytes(_ name: String) throws -> [UInt8] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "bin", subdirectory: "Fixtures/terminal"))
        return Array(try Data(contentsOf: url))
    }

    func screen(after phases: [String]) throws -> TerminalScreen {
        let screen = TerminalScreen(rows: 30, cols: 100)
        for phase in phases { screen.feed(try bytes(phase)) }
        return screen
    }

    func testSessionDraws() throws {
        let s = try screen(after: ["plugin-0-attach"])
        let text = s.lines.joined(separator: "\n")
        XCTAssertTrue(s.altScreen, "Claude Code runs full screen")
        XCTAssertTrue(text.contains("Claude Code v2.1.318"), text)
        XCTAssertTrue(text.contains("❯ Reply with just OK."), text)
        XCTAssertTrue(s.bracketedPaste)
        XCTAssertTrue(s.mouseTracking && s.sgrMouse)
        XCTAssertEqual(s.lines.count, 30)
    }

    func testPluginMenuDraws() throws {
        let s = try screen(after: ["plugin-0-attach", "plugin-1-typed", "plugin-2-enter"])
        let text = s.lines.joined(separator: "\n")
        XCTAssertTrue(text.contains("Discover plugins"), text)
        XCTAssertTrue(text.contains("Installed"), text)
        XCTAssertTrue(text.contains("⌕ Search…"), text)
        XCTAssertFalse(text.contains("Loading…"), "the spinner line was redrawn over")
    }

    func testEscapeLeavesTheMenu() throws {
        let s = try screen(after: ["plugin-0-attach", "plugin-1-typed", "plugin-2-enter", "plugin-3-down", "plugin-4-esc"])
        let text = s.lines.joined(separator: "\n")
        XCTAssertFalse(text.contains("Discover plugins"), text)
        XCTAssertTrue(text.contains("❯ /plugin"), text)
    }

    func testWrapEraseAndColor() {
        let s = TerminalScreen(rows: 3, cols: 5)
        s.feed(Array("abcdefg".utf8))
        XCTAssertEqual(s.lines, ["abcde", "fg", ""])
        s.feed(Array("\u{1b}[1;3H\u{1b}[K\u{1b}[38;2;1;2;3mX".utf8))
        XCTAssertEqual(s.lines[0], "abX")
        XCTAssertEqual(s.cells[0][2].style.fg, .rgb(1, 2, 3))
        s.feed(Array("\u{1b}[3;1H\nZ".utf8))
        XCTAssertEqual(s.lines, ["fg", "", "Z"], "a line feed on the last row scrolls")
    }

    func testWideAndCombiningCharacters() {
        let s = TerminalScreen(rows: 2, cols: 6)
        s.feed(Array("日本e\u{0301}x".utf8))
        XCTAssertEqual(s.cells[0][0].width, 2)
        XCTAssertEqual(s.cells[0][1].width, 0)
        XCTAssertEqual(s.cells[0][4].text, "e\u{0301}")
        XCTAssertEqual(s.lines[0], "日本éx")
    }

    func testAnswersCursorPositionButNotDeviceAttributes() {
        let s = TerminalScreen(rows: 5, cols: 10)
        s.feed(Array("\u{1b}[2;4H\u{1b}[6n".utf8))
        XCTAssertEqual(String(decoding: s.takeReplies(), as: UTF8.self), "\u{1b}[2;4R")
        s.feed(Array("\u{1b}[c\u{1b}[>0q\u{1b}[>c".utf8))
        XCTAssertTrue(s.takeReplies().isEmpty, "an answer here cost Claude Code its colors and landed in its input box")
    }

    /// A reply must not be typed into an open question: it would become the answer.
    func testSeesAnOpenQuestion() throws {
        let asking = TerminalScreen(rows: 50, cols: 200)
        asking.feed(try bytes("question-open"))
        XCTAssertTrue(ReplySender.dialogOpen(asking), asking.lines.joined(separator: "\n"))
        XCTAssertFalse(ReplySender.dialogOpen(try screen(after: ["plugin-0-attach"])), "the plain prompt takes replies")
    }

    func testResizeKeepsRowsInPlace() {
        let s = TerminalScreen(rows: 4, cols: 6)
        s.feed(Array("a\r\nb\r\nc\r\nd".utf8))
        s.resize(rows: 2, cols: 3)
        XCTAssertEqual(s.lines, ["a", "b"], "Claude Code repaints only what changed, so rows must not move")
    }
}

final class TerminalKeysTests: XCTestCase {
    func key(_ code: UInt16, _ chars: String = "", shift: Bool = false, control: Bool = false, option: Bool = false) -> TerminalKeys.Press {
        TerminalKeys.Press(keyCode: code, characters: chars, shift: shift, control: control, option: option)
    }

    func testMenuKeys() {
        XCTAssertEqual(TerminalKeys.bytes(key(126), appCursorKeys: false), "\u{1b}[A")
        XCTAssertEqual(TerminalKeys.bytes(key(125), appCursorKeys: true), "\u{1b}OB")
        XCTAssertEqual(TerminalKeys.bytes(key(36, "\r"), appCursorKeys: false), "\r")
        XCTAssertEqual(TerminalKeys.bytes(key(53), appCursorKeys: false), "\u{1b}")
        XCTAssertEqual(TerminalKeys.bytes(key(48, "\t", shift: true), appCursorKeys: false), "\u{1b}[Z")
        XCTAssertEqual(TerminalKeys.bytes(key(49, " "), appCursorKeys: false), " ", "Space toggles a plugin")
    }

    func testControlAndText() {
        XCTAssertEqual(TerminalKeys.bytes(key(8, "\u{3}", control: true), appCursorKeys: false), "\u{3}")
        XCTAssertEqual(TerminalKeys.bytes(key(1, "s", control: true), appCursorKeys: false), "\u{13}", "ctrl+s filters by marketplace")
        XCTAssertEqual(TerminalKeys.bytes(key(0, "ă"), appCursorKeys: false), "ă")
        XCTAssertEqual(TerminalKeys.bytes(key(51), appCursorKeys: false), "\u{7f}")
    }

    func testMouseReport() {
        XCTAssertEqual(TerminalKeys.mouse(button: 64, col: 0, row: 4), "\u{1b}[<64;1;5M")
        XCTAssertEqual(TerminalKeys.mouse(button: 0, col: 9, row: 2, released: true), "\u{1b}[<0;10;3m")
    }
}
