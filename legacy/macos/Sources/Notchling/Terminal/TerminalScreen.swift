import Foundation

enum TermColor: Equatable {
    case indexed(UInt8)
    case rgb(UInt8, UInt8, UInt8)
}

struct TermStyle: Equatable {
    var fg: TermColor?
    var bg: TermColor?
    var bold = false
    var dim = false
    var italic = false
    var underline = false
    var inverse = false
}

struct TermCell: Equatable {
    var text = " "
    var style = TermStyle()
    /// 2 for the left half of a wide character, 0 for its right half.
    var width: UInt8 = 1
}

/// A terminal's screen: a grid of cells fed with the bytes a program writes. It covers what Claude
/// Code's interface sends through `claude attach` (recorded on 2.1.318: cursor moves, erases, 24-bit
/// color, the alternate screen, mouse and paste modes) and skips everything else.
final class TerminalScreen {
    private(set) var rows: Int
    private(set) var cols: Int
    private(set) var cells: [[TermCell]]
    private(set) var cursorRow = 0
    private(set) var cursorCol = 0
    private(set) var cursorVisible = true
    private(set) var appCursorKeys = false
    private(set) var bracketedPaste = false
    private(set) var mouseTracking = false
    private(set) var sgrMouse = false
    var altScreen: Bool { savedMain != nil }
    /// Answers the program asked for (device attributes, cursor position), to write back to it.
    private(set) var replies: [UInt8] = []

    private var style = TermStyle()
    private var wrapPending = false
    private var top = 0
    private var bottom: Int
    private var savedCursor: (row: Int, col: Int, style: TermStyle)?
    private var savedMain: [[TermCell]]?

    private enum State { case ground, escape, escapeIntermediate, csi, string, stringEscape }
    private var state = State.ground
    private var params: [UInt8] = []
    private var utf8: [UInt8] = []
    private var utf8Need = 0
    private var joinNext = false

    init(rows: Int, cols: Int) {
        self.rows = max(1, rows)
        self.cols = max(1, cols)
        bottom = self.rows - 1
        cells = Array(repeating: Array(repeating: TermCell(), count: self.cols), count: self.rows)
    }

    func takeReplies() -> [UInt8] {
        defer { replies = [] }
        return replies
    }

    /// Each row as plain text, trailing spaces dropped.
    var lines: [String] {
        cells.map { row in
            var s = row.map(\.text).joined()
            while s.hasSuffix(" ") { s.removeLast() }
            return s
        }
    }

    func resize(rows newRows: Int, cols newCols: Int) {
        let newRows = max(1, newRows), newCols = max(1, newCols)
        guard newRows != rows || newCols != cols else { return }
        func fit(_ grid: [[TermCell]]) -> [[TermCell]] {
            // Rows stay where they were: Claude Code redraws only what changed from its last frame,
            // so moving rows up left pieces of the old frame on screen.
            let kept = grid.prefix(newRows)
            var out = kept.map { row -> [TermCell] in
                var r = Array(row.prefix(newCols))
                if r.count < newCols { r += Array(repeating: TermCell(), count: newCols - r.count) }
                return r
            }
            while out.count < newRows { out.append(Array(repeating: TermCell(), count: newCols)) }
            return out
        }
        cells = fit(cells)
        savedMain = savedMain.map(fit)
        rows = newRows
        cols = newCols
        top = 0
        bottom = rows - 1
        cursorRow = min(cursorRow, rows - 1)
        cursorCol = min(cursorCol, cols - 1)
        wrapPending = false
    }

    // MARK: - Parsing

    func feed(_ bytes: [UInt8]) {
        for b in bytes { step(b) }
    }

    private func step(_ b: UInt8) {
        switch state {
        case .ground:
            ground(b)
        case .escape:
            escape(b)
        case .escapeIntermediate:
            state = .ground
        case .csi:
            switch b {
            case 0x20...0x3F: params.append(b)
            case 0x40...0x7E: state = .ground; csi(final: b)
            case 0x1B: state = .escape
            case 0x00..<0x20: control(b)
            default: break
            }
        case .string:
            // OSC, DCS and the like: titles, links, queries. Skipped up to BEL or ESC \.
            if b == 0x07 { state = .ground } else if b == 0x1B { state = .stringEscape }
        case .stringEscape:
            state = b == 0x5C ? .ground : .string
        }
    }

    private func ground(_ b: UInt8) {
        if utf8Need > 0 {
            if b & 0xC0 == 0x80 {
                utf8.append(b)
                utf8Need -= 1
                if utf8Need == 0, let s = String(bytes: utf8, encoding: .utf8)?.unicodeScalars.first { put(s) }
                return
            }
            utf8Need = 0
        }
        switch b {
        case 0x00..<0x20: control(b)
        case 0x7F: break
        case 0x80..<0xC0: break
        case 0xC0..<0xE0: utf8 = [b]; utf8Need = 1
        case 0xE0..<0xF0: utf8 = [b]; utf8Need = 2
        case 0xF0..<0xF8: utf8 = [b]; utf8Need = 3
        case 0xF8...0xFF: break
        default: put(Unicode.Scalar(b))
        }
    }

    private func control(_ b: UInt8) {
        switch b {
        case 0x08:
            wrapPending = false
            cursorCol = max(0, cursorCol - 1)
        case 0x09:
            cursorCol = min(cols - 1, (cursorCol / 8 + 1) * 8)
        case 0x0A, 0x0B, 0x0C:
            lineFeed()
        case 0x0D:
            wrapPending = false
            cursorCol = 0
        case 0x1B:
            state = .escape
        default:
            break
        }
    }

    private func escape(_ b: UInt8) {
        state = .ground
        switch b {
        case UInt8(ascii: "["): params = []; state = .csi
        case UInt8(ascii: "]"), UInt8(ascii: "P"), UInt8(ascii: "_"), UInt8(ascii: "^"), UInt8(ascii: "X"): state = .string
        case UInt8(ascii: "("), UInt8(ascii: ")"), UInt8(ascii: "*"), UInt8(ascii: "+"), UInt8(ascii: "#"), UInt8(ascii: "%"):
            state = .escapeIntermediate
        case UInt8(ascii: "7"): saveCursor()
        case UInt8(ascii: "8"): restoreCursor()
        case UInt8(ascii: "D"): lineFeed()
        case UInt8(ascii: "E"): cursorCol = 0; lineFeed()
        case UInt8(ascii: "M"): reverseIndex()
        case UInt8(ascii: "c"): reset()
        default: break
        }
    }

    // MARK: - Writing

    private func put(_ scalar: Unicode.Scalar) {
        if joinNext || Self.isCombining(scalar) {
            joinNext = scalar.value == 0x200D
            appendToPrevious(scalar)
            return
        }
        let width = Self.isWide(scalar) ? 2 : 1
        if wrapPending {
            wrapPending = false
            cursorCol = 0
            lineFeed()
        }
        if width == 2 && cursorCol == cols - 1 {
            cells[cursorRow][cursorCol] = blank()
            cursorCol = 0
            lineFeed()
        }
        cells[cursorRow][cursorCol] = TermCell(text: String(scalar), style: style, width: UInt8(width))
        if width == 2, cursorCol + 1 < cols {
            cells[cursorRow][cursorCol + 1] = TermCell(text: "", style: style, width: 0)
        }
        cursorCol += width
        if cursorCol >= cols {
            cursorCol = cols - 1
            wrapPending = true
        }
    }

    private func appendToPrevious(_ scalar: Unicode.Scalar) {
        var col = wrapPending ? cursorCol : cursorCol - 1
        guard col >= 0 else { return }
        if cells[cursorRow][col].width == 0, col > 0 { col -= 1 }
        cells[cursorRow][col].text.unicodeScalars.append(scalar)
    }

    private func blank() -> TermCell {
        TermCell(text: " ", style: TermStyle(bg: style.bg), width: 1)
    }

    private func blankRow() -> [TermCell] {
        Array(repeating: blank(), count: cols)
    }

    private func lineFeed() {
        wrapPending = false
        if cursorRow == bottom {
            scrollUp(1)
        } else if cursorRow < rows - 1 {
            cursorRow += 1
        }
    }

    private func reverseIndex() {
        wrapPending = false
        if cursorRow == top { scrollDown(1) } else if cursorRow > 0 { cursorRow -= 1 }
    }

    private func scrollUp(_ n: Int) {
        let n = min(n, bottom - top + 1)
        guard n > 0 else { return }
        cells.removeSubrange(top..<(top + n))
        cells.insert(contentsOf: Array(repeating: blankRow(), count: n), at: bottom - n + 1)
    }

    private func scrollDown(_ n: Int) {
        let n = min(n, bottom - top + 1)
        guard n > 0 else { return }
        cells.removeSubrange((bottom - n + 1)...bottom)
        cells.insert(contentsOf: Array(repeating: blankRow(), count: n), at: top)
    }

    private func saveCursor() { savedCursor = (cursorRow, cursorCol, style) }

    private func restoreCursor() {
        guard let saved = savedCursor else { return }
        cursorRow = min(saved.row, rows - 1)
        cursorCol = min(saved.col, cols - 1)
        style = saved.style
        wrapPending = false
    }

    private func reset() {
        style = TermStyle()
        cells = Array(repeating: Array(repeating: TermCell(), count: cols), count: rows)
        savedMain = nil
        cursorRow = 0
        cursorCol = 0
        top = 0
        bottom = rows - 1
        cursorVisible = true
        wrapPending = false
    }

    // MARK: - Control sequences

    private func csi(final: UInt8) {
        var raw = params
        var prefix: UInt8?
        if let first = raw.first, (0x3C...0x3F).contains(first) { prefix = first; raw.removeFirst() }
        let numbers = String(decoding: raw.filter { $0 != 0x20 }, as: UTF8.self)
            .replacingOccurrences(of: ":", with: ";")
            .split(separator: ";", omittingEmptySubsequences: false)
            .map { Int($0) }
        func arg(_ i: Int, _ fallback: Int) -> Int {
            guard i < numbers.count, let v = numbers[i], v > 0 else { return fallback }
            return v
        }
        let n = arg(0, 1)
        let final = Character(Unicode.Scalar(final))

        if prefix == UInt8(ascii: "?") {
            if final == "h" || final == "l" { setModes(numbers.compactMap { $0 }, on: final == "h") }
            return
        }
        // Device-attribute queries (CSI c, CSI > c, CSI > q) go unanswered, as in a plain pty: answered,
        // Claude Code 2.1.318 dropped its colors and its line layout, and the answer landed in its input box.
        if prefix != nil { return }

        switch final {
        case "A": moveTo(row: max(cursorRow >= top ? top : 0, cursorRow - n), col: cursorCol)
        case "B", "e": moveTo(row: min(cursorRow <= bottom ? bottom : rows - 1, cursorRow + n), col: cursorCol)
        case "C", "a": moveTo(row: cursorRow, col: cursorCol + n)
        case "D": moveTo(row: cursorRow, col: cursorCol - n)
        case "E": moveTo(row: cursorRow + n, col: 0)
        case "F": moveTo(row: cursorRow - n, col: 0)
        case "G", "`": moveTo(row: cursorRow, col: n - 1)
        case "d": moveTo(row: n - 1, col: cursorCol)
        case "H", "f": moveTo(row: arg(0, 1) - 1, col: arg(1, 1) - 1)
        case "J": eraseDisplay(numbers.first.flatMap { $0 } ?? 0)
        case "K": eraseLine(numbers.first.flatMap { $0 } ?? 0)
        case "X":
            for c in cursorCol..<min(cols, cursorCol + n) { cells[cursorRow][c] = blank() }
        case "P":
            let count = min(n, cols - cursorCol)
            cells[cursorRow].removeSubrange(cursorCol..<(cursorCol + count))
            cells[cursorRow] += Array(repeating: blank(), count: count)
        case "@":
            let count = min(n, cols - cursorCol)
            cells[cursorRow].insert(contentsOf: Array(repeating: blank(), count: count), at: cursorCol)
            cells[cursorRow].removeLast(count)
        case "L":
            guard (top...bottom).contains(cursorRow) else { return }
            let count = min(n, bottom - cursorRow + 1)
            cells.removeSubrange((bottom - count + 1)...bottom)
            cells.insert(contentsOf: Array(repeating: blankRow(), count: count), at: cursorRow)
        case "M":
            guard (top...bottom).contains(cursorRow) else { return }
            let count = min(n, bottom - cursorRow + 1)
            cells.removeSubrange(cursorRow..<(cursorRow + count))
            cells.insert(contentsOf: Array(repeating: blankRow(), count: count), at: bottom - count + 1)
        case "S": scrollUp(n)
        case "T": scrollDown(n)
        case "m": sgr(numbers)
        case "r":
            let t = arg(0, 1) - 1, b = arg(1, rows) - 1
            guard t < b, b < rows else { return }
            top = t
            bottom = b
            moveTo(row: 0, col: 0)
        case "s": saveCursor()
        case "u": restoreCursor()
        case "n":
            if n == 6 { replies += Array("\u{1b}[\(cursorRow + 1);\(cursorCol + 1)R".utf8) }
            if n == 5 { replies += Array("\u{1b}[0n".utf8) }
        default:
            break
        }
    }

    private func moveTo(row: Int, col: Int) {
        wrapPending = false
        cursorRow = min(max(0, row), rows - 1)
        cursorCol = min(max(0, col), cols - 1)
    }

    private func eraseDisplay(_ mode: Int) {
        switch mode {
        case 0:
            eraseLine(0)
            for r in (cursorRow + 1)..<max(cursorRow + 1, rows) { cells[r] = blankRow() }
        case 1:
            for r in 0..<cursorRow { cells[r] = blankRow() }
            eraseLine(1)
        default:
            cells = Array(repeating: blankRow(), count: rows)
        }
    }

    private func eraseLine(_ mode: Int) {
        let range: Range<Int>
        switch mode {
        case 0: range = cursorCol..<cols
        case 1: range = 0..<(cursorCol + 1)
        default: range = 0..<cols
        }
        for c in range { cells[cursorRow][c] = blank() }
        wrapPending = false
    }

    private func setModes(_ modes: [Int], on: Bool) {
        for mode in modes {
            switch mode {
            case 1: appCursorKeys = on
            case 25: cursorVisible = on
            case 2004: bracketedPaste = on
            case 1000, 1002, 1003: mouseTracking = on
            case 1006: sgrMouse = on
            case 47, 1047, 1049:
                if on, savedMain == nil {
                    if mode == 1049 { saveCursor() }
                    savedMain = cells
                    cells = Array(repeating: Array(repeating: TermCell(), count: cols), count: rows)
                } else if !on, let main = savedMain {
                    cells = main
                    savedMain = nil
                    if mode == 1049 { restoreCursor() }
                }
            default:
                break
            }
        }
    }

    private func sgr(_ numbers: [Int?]) {
        let values = numbers.isEmpty ? [0] : numbers.map { $0 ?? 0 }
        var i = 0
        func color() -> TermColor? {
            guard i + 1 < values.count else { return nil }
            if values[i + 1] == 5, i + 2 < values.count {
                defer { i += 2 }
                return .indexed(UInt8(clamping: values[i + 2]))
            }
            if values[i + 1] == 2, i + 4 < values.count {
                defer { i += 4 }
                return .rgb(UInt8(clamping: values[i + 2]), UInt8(clamping: values[i + 3]), UInt8(clamping: values[i + 4]))
            }
            return nil
        }
        while i < values.count {
            let v = values[i]
            switch v {
            case 0: style = TermStyle()
            case 1: style.bold = true
            case 2: style.dim = true
            case 3: style.italic = true
            case 4: style.underline = true
            case 7: style.inverse = true
            case 22: style.bold = false; style.dim = false
            case 23: style.italic = false
            case 24: style.underline = false
            case 27: style.inverse = false
            case 30...37: style.fg = .indexed(UInt8(v - 30))
            case 38: style.fg = color()
            case 39: style.fg = nil
            case 40...47: style.bg = .indexed(UInt8(v - 40))
            case 48: style.bg = color()
            case 49: style.bg = nil
            case 90...97: style.fg = .indexed(UInt8(v - 90 + 8))
            case 100...107: style.bg = .indexed(UInt8(v - 100 + 8))
            default: break
            }
            i += 1
        }
    }

    // MARK: - Character widths

    private static func isCombining(_ s: Unicode.Scalar) -> Bool {
        switch s.properties.generalCategory {
        case .nonspacingMark, .enclosingMark, .spacingMark, .format: return true
        default: return (0xFE00...0xFE0F).contains(s.value) || (0x1F3FB...0x1F3FF).contains(s.value)
        }
    }

    private static func isWide(_ s: Unicode.Scalar) -> Bool {
        switch s.value {
        case 0x1100...0x115F, 0x2E80...0x303E, 0x3041...0x33FF, 0x3400...0x4DBF, 0x4E00...0x9FFF,
             0xA000...0xA4CF, 0xAC00...0xD7A3, 0xF900...0xFAFF, 0xFE30...0xFE4F, 0xFF00...0xFF60,
             0xFFE0...0xFFE6, 0x1F300...0x1F64F, 0x1F900...0x1F9FF, 0x1FA70...0x1FAFF, 0x20000...0x3FFFD:
            return true
        default:
            return false
        }
    }
}
