import AppKit
import SwiftUI

/// What a key press sends to the terminal, xterm style.
enum TerminalKeys {
    struct Press {
        var keyCode: UInt16
        var characters: String
        var shift = false
        var control = false
        var option = false
    }

    static func bytes(_ key: Press, appCursorKeys: Bool) -> String? {
        let arrow = appCursorKeys ? "\u{1b}O" : "\u{1b}["
        switch key.keyCode {
        case 36, 76: return "\r"
        case 48: return key.shift ? "\u{1b}[Z" : "\t"
        case 51: return key.option ? "\u{1b}\u{7f}" : "\u{7f}"
        case 53: return "\u{1b}"
        case 117: return "\u{1b}[3~"
        case 123: return key.option ? "\u{1b}b" : arrow + "D"
        case 124: return key.option ? "\u{1b}f" : arrow + "C"
        case 125: return arrow + "B"
        case 126: return arrow + "A"
        case 115: return "\u{1b}[H"
        case 119: return "\u{1b}[F"
        case 116: return "\u{1b}[5~"
        case 121: return "\u{1b}[6~"
        default: break
        }
        if key.control, let scalar = key.characters.lowercased().unicodeScalars.first {
            switch scalar.value {
            case 0x61...0x7A: return String(UnicodeScalar(UInt8(scalar.value - 0x60)))
            case 0x20: return "\u{0}"
            default: break
            }
        }
        return key.characters.isEmpty ? nil : key.characters
    }

    /// An SGR mouse report ("ESC [ < button ; col ; row M"), 1-based cells.
    static func mouse(button: Int, col: Int, row: Int, released: Bool = false) -> String {
        "\u{1b}[<\(button);\(col + 1);\(row + 1)\(released ? "m" : "M")"
    }
}

/// Draws a `TerminalSession`'s screen and sends it the keys, clicks and scrolls.
final class TerminalCanvas: NSView {
    let session: TerminalSession
    private let font = NSFont.monospacedSystemFont(ofSize: KitMetric.terminalFont, weight: .regular)
    private let boldFont = NSFont.monospacedSystemFont(ofSize: KitMetric.terminalFont, weight: .bold)
    private lazy var cellWidth: CGFloat = ("M" as NSString).size(withAttributes: [.font: font]).width
    private lazy var cellHeight: CGFloat = ceil(font.ascender - font.descender + font.leading) + 1
    private let inset: CGFloat = 10
    private var scrollAccumulator: CGFloat = 0
    private let background = NSColor(KitInk.terminal)
    private let foreground = NSColor(KitInk.terminalText)

    init(session: TerminalSession) {
        self.session = session
        super.init(frame: .zero)
        session.onChange = { [weak self] in self?.needsDisplay = true }
    }

    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    override func layout() {
        super.layout()
        let cols = Int((bounds.width - inset * 2) / cellWidth)
        let rows = Int((bounds.height - inset * 2) / cellHeight)
        guard rows > 2, cols > 10 else { return }
        session.begin(rows: rows, cols: cols)
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        background.setFill()
        bounds.fill()
        let screen = session.screen
        for (r, row) in screen.cells.enumerated() {
            let y = inset + CGFloat(r) * cellHeight
            // Backgrounds first, one rect per run of a color, on whole pixels so cells don't show seams.
            var c = 0
            while c < row.count {
                guard let bg = colors(row[c].style).1 else { c += 1; continue }
                var end = c + 1
                while end < row.count, colors(row[end].style).1 == bg { end += 1 }
                bg.setFill()
                NSRect(x: x(c), y: y, width: x(end) - x(c), height: cellHeight).fill()
                c = end
            }
            c = 0
            while c < row.count {
                let cell = row[c]
                let fg = colors(cell.style).0
                let span = max(1, Int(cell.width))
                // ASCII in one style draws as a run; anything else on its own, at its cell.
                if cell.width == 1, cell.text.utf8.count == 1, cell.text != " " {
                    var run = cell.text
                    var end = c + 1
                    while end < row.count, row[end].style == cell.style, row[end].width == 1,
                          row[end].text.utf8.count == 1 {
                        run += row[end].text
                        end += 1
                    }
                    drawText(run, at: c, y: y, style: cell.style, color: fg)
                    c = end
                    continue
                }
                if cell.width != 0, cell.text != " ", !cell.text.isEmpty {
                    let rect = NSRect(x: x(c), y: y, width: x(c + span) - x(c), height: cellHeight)
                    if !drawShape(cell.text, in: rect, color: fg) {
                        drawText(cell.text, at: c, y: y, style: cell.style, color: fg)
                    }
                }
                c += span
            }
        }
        if screen.cursorVisible, !session.ended {
            foreground.withAlphaComponent(0.6).setFill()
            NSRect(x: inset + CGFloat(screen.cursorCol) * cellWidth, y: inset + CGFloat(screen.cursorRow) * cellHeight,
                   width: cellWidth, height: cellHeight).fill()
        }
    }

    /// A cell edge on a whole pixel.
    private func x(_ col: Int) -> CGFloat { (inset + CGFloat(col) * cellWidth).rounded() }

    /// Box-drawing and block characters drawn as shapes that fill the cell, so lines join across
    /// cells; the font's own glyphs are narrower than a cell and left the borders dashed.
    private func drawShape(_ text: String, in rect: NSRect, color: NSColor) -> Bool {
        guard text.unicodeScalars.count == 1, let v = text.unicodeScalars.first?.value,
              (0x2500...0x259F).contains(v) else { return false }
        color.setFill()
        color.setStroke()
        let line: CGFloat = 1
        let midX = (rect.midX).rounded(.down) + 0.5, midY = (rect.midY).rounded(.down) + 0.5
        func h(_ from: CGFloat, _ to: CGFloat) { NSRect(x: from, y: midY - 0.5, width: to - from, height: line).fill() }
        func vert(_ from: CGFloat, _ to: CGFloat) { NSRect(x: midX - 0.5, y: from, width: line, height: to - from).fill() }
        func arc(_ start: NSPoint, _ corner: NSPoint, _ end: NSPoint) {
            let path = NSBezierPath()
            path.lineWidth = line
            path.move(to: start)
            path.curve(to: end, controlPoint1: corner, controlPoint2: corner)
            path.stroke()
        }
        func block(_ x0: CGFloat, _ y0: CGFloat, _ x1: CGFloat, _ y1: CGFloat) {
            NSRect(x: rect.minX + rect.width * x0, y: rect.minY + rect.height * y0,
                   width: rect.width * (x1 - x0), height: rect.height * (y1 - y0)).fill()
        }
        switch v {
        case 0x2500, 0x2501: h(rect.minX, rect.maxX)
        case 0x2502, 0x2503: vert(rect.minY, rect.maxY)
        case 0x250C: h(midX, rect.maxX); vert(midY, rect.maxY)
        case 0x2510: h(rect.minX, midX); vert(midY, rect.maxY)
        case 0x2514: h(midX, rect.maxX); vert(rect.minY, midY)
        case 0x2518: h(rect.minX, midX); vert(rect.minY, midY)
        case 0x251C: vert(rect.minY, rect.maxY); h(midX, rect.maxX)
        case 0x2524: vert(rect.minY, rect.maxY); h(rect.minX, midX)
        case 0x252C: h(rect.minX, rect.maxX); vert(midY, rect.maxY)
        case 0x2534: h(rect.minX, rect.maxX); vert(rect.minY, midY)
        case 0x253C: h(rect.minX, rect.maxX); vert(rect.minY, rect.maxY)
        case 0x256D: arc(NSPoint(x: rect.maxX, y: midY), NSPoint(x: midX, y: midY), NSPoint(x: midX, y: rect.maxY))
        case 0x256E: arc(NSPoint(x: rect.minX, y: midY), NSPoint(x: midX, y: midY), NSPoint(x: midX, y: rect.maxY))
        case 0x256F: arc(NSPoint(x: rect.minX, y: midY), NSPoint(x: midX, y: midY), NSPoint(x: midX, y: rect.minY))
        case 0x2570: arc(NSPoint(x: rect.maxX, y: midY), NSPoint(x: midX, y: midY), NSPoint(x: midX, y: rect.minY))
        case 0x2580: block(0, 0, 1, 0.5)
        case 0x2581: block(0, 0.875, 1, 1)
        case 0x2584: block(0, 0.5, 1, 1)
        case 0x2588: block(0, 0, 1, 1)
        case 0x258C: block(0, 0, 0.5, 1)
        case 0x2590: block(0.5, 0, 1, 1)
        case 0x2594: block(0, 0, 1, 0.125)
        case 0x2596: block(0, 0.5, 0.5, 1)
        case 0x2597: block(0.5, 0.5, 1, 1)
        case 0x2598: block(0, 0, 0.5, 0.5)
        case 0x259D: block(0.5, 0, 1, 0.5)
        case 0x2599: block(0, 0, 0.5, 1); block(0.5, 0.5, 1, 1)
        case 0x259B: block(0, 0, 1, 0.5); block(0, 0.5, 0.5, 1)
        case 0x259C: block(0, 0, 1, 0.5); block(0.5, 0.5, 1, 1)
        case 0x259F: block(0.5, 0, 1, 1); block(0, 0.5, 0.5, 1)
        default: return false
        }
        return true
    }

    private func drawText(_ text: String, at col: Int, y: CGFloat, style: TermStyle, color: NSColor) {
        var attributes: [NSAttributedString.Key: Any] = [.font: style.bold ? boldFont : font, .foregroundColor: color]
        if style.italic {
            attributes[.font] = NSFontManager.shared.convert(style.bold ? boldFont : font, toHaveTrait: .italicFontMask)
        }
        if style.underline { attributes[.underlineStyle] = NSUnderlineStyle.single.rawValue }
        (text as NSString).draw(at: NSPoint(x: x(col), y: y), withAttributes: attributes)
    }

    private func colors(_ style: TermStyle) -> (NSColor, NSColor?) {
        var fg = style.fg.map(nsColor) ?? foreground
        var bg = style.bg.map(nsColor)
        if style.inverse {
            let swapped = bg ?? background
            bg = fg
            fg = swapped
        }
        if style.dim { fg = fg.withAlphaComponent(0.55) }
        return (fg, bg)
    }

    private func nsColor(_ color: TermColor) -> NSColor {
        switch color {
        case .rgb(let r, let g, let b):
            return NSColor(srgbRed: CGFloat(r) / 255, green: CGFloat(g) / 255, blue: CGFloat(b) / 255, alpha: 1)
        case .indexed(let i):
            let (r, g, b) = Self.palette(Int(i))
            return NSColor(srgbRed: CGFloat(r) / 255, green: CGFloat(g) / 255, blue: CGFloat(b) / 255, alpha: 1)
        }
    }

    /// xterm's 256 colors: 16 named, a 6×6×6 cube, 24 grays.
    private static func palette(_ i: Int) -> (Int, Int, Int) {
        let named = [(0, 0, 0), (205, 49, 49), (13, 188, 121), (229, 229, 16), (36, 114, 200), (188, 63, 188), (17, 168, 205), (229, 229, 229),
                     (102, 102, 102), (241, 76, 76), (35, 209, 139), (245, 245, 67), (59, 142, 234), (214, 112, 214), (41, 184, 219), (255, 255, 255)]
        if i < 16 { return named[i] }
        if i < 232 {
            let n = i - 16, steps = [0, 95, 135, 175, 215, 255]
            return (steps[n / 36], steps[(n / 6) % 6], steps[n % 6])
        }
        let gray = 8 + (i - 232) * 10
        return (gray, gray, gray)
    }

    // MARK: Input

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags
        let press = TerminalKeys.Press(keyCode: event.keyCode, characters: event.characters ?? "",
                                       shift: flags.contains(.shift), control: flags.contains(.control),
                                       option: flags.contains(.option))
        if let bytes = TerminalKeys.bytes(press, appCursorKeys: session.screen.appCursorKeys) { session.send(bytes) }
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self, event.modifierFlags.contains(.command) else { return false }
        if event.charactersIgnoringModifiers == "v", let text = NSPasteboard.general.string(forType: .string) {
            session.paste(text)
            return true
        }
        return false
    }

    override func scrollWheel(with event: NSEvent) {
        guard session.screen.mouseTracking, session.screen.sgrMouse else { return }
        scrollAccumulator += event.scrollingDeltaY * (event.hasPreciseScrollingDeltas ? 1 : cellHeight)
        let (row, col) = cell(at: event)
        while abs(scrollAccumulator) >= cellHeight {
            let up = scrollAccumulator > 0
            session.send(TerminalKeys.mouse(button: up ? 64 : 65, col: col, row: row))
            scrollAccumulator += up ? -cellHeight : cellHeight
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard session.screen.mouseTracking, session.screen.sgrMouse else { return }
        let (row, col) = cell(at: event)
        session.send(TerminalKeys.mouse(button: 0, col: col, row: row))
    }

    override func mouseUp(with event: NSEvent) {
        guard session.screen.mouseTracking, session.screen.sgrMouse else { return }
        let (row, col) = cell(at: event)
        session.send(TerminalKeys.mouse(button: 0, col: col, row: row, released: true))
    }

    private func cell(at event: NSEvent) -> (Int, Int) {
        let p = convert(event.locationInWindow, from: nil)
        let row = min(max(0, Int((p.y - inset) / cellHeight)), session.screen.rows - 1)
        let col = min(max(0, Int((p.x - inset) / cellWidth)), session.screen.cols - 1)
        return (row, col)
    }
}

struct TerminalPanel: NSViewRepresentable {
    let session: TerminalSession

    func makeNSView(context: Context) -> TerminalCanvas { TerminalCanvas(session: session) }
    func updateNSView(_ view: TerminalCanvas, context: Context) { view.needsDisplay = true }
}

/// The session in a terminal, over Chat: for commands that open a menu (/plugin, /mcp, /agents…),
/// which Chat's own reply path can't show.
struct TerminalSheet: View {
    @ObservedObject var session: TerminalSession
    let close: () -> Void

    private var status: String {
        guard session.script != nil else { return session.ended ? "Detached" : "Terminal" }
        guard session.ended else { return "Running" }
        return session.exitStatus == 0 ? "Finished" : "Exited with \(session.exitStatus ?? -1)"
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: KitIcon.terminal.symbol).foregroundColor(KitInk.textMuted)
                Text(session.title).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                Text(status).kitType(.meta).foregroundColor(KitInk.textMuted)
                Spacer()
                Text(session.script == nil ? "Keys go to the session. Done leaves it running."
                     : session.ended ? "" : "Ctrl+C or Done stops it.")
                    .kitType(.meta).foregroundColor(KitInk.textFaint)
                Button("Done", action: close)
                    .buttonStyle(.kit(.secondary))
                    .keyboardShortcut("w", modifiers: .command)
                    .pointerCursor()
                    .part("Terminal Done Button", .bottom)
            }
            .padding(.horizontal, 14)
            .frame(height: KitMetric.titleBar + 6)
            .background(KitInk.card)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            ZStack {
                TerminalPanel(session: session).part("Terminal", .topTrailing, inside: true)
                if let failure = session.failure {
                    Text(failure).kitType(.body).foregroundColor(KitInk.terminalText)
                }
            }
            .background(KitInk.terminal)
        }
        .frame(width: KitMetric.terminalSheet.width, height: KitMetric.terminalSheet.height)
        .onDisappear { session.close() }
    }
}
