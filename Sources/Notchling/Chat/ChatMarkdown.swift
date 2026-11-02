import AppKit
import SwiftUI

/// Claude's markdown as blocks: headings, paragraphs, lists, quotes, rules, fenced code and tables.
/// Inline styles (bold, italics, `code`, links) come from Foundation's markdown parser per block.
enum MarkdownBlock: Equatable {
    case heading(level: Int, text: String)
    case paragraph(String)
    case list([ListItem])
    case quote(String)
    case code(language: String, text: String)
    case table(header: [String], rows: [[String]])
    case rule

    struct ListItem: Equatable {
        let marker: String
        var text: String
        let depth: Int
    }
}

enum MarkdownParser {
    private static var cache: [String: [MarkdownBlock]] = [:]
    private static let lock = NSLock()

    static func blocks(_ text: String) -> [MarkdownBlock] {
        lock.lock()
        if let hit = cache[text] { lock.unlock(); return hit }
        lock.unlock()
        let result = parse(text)
        lock.lock()
        if cache.count > 2000 { cache.removeAll() }
        cache[text] = result
        lock.unlock()
        return result
    }

    private static func parse(_ text: String) -> [MarkdownBlock] {
        let lines = text.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        var out: [MarkdownBlock] = []
        var paragraph: [String] = []
        var list: [MarkdownBlock.ListItem] = []
        var quote: [String] = []

        func flush() {
            if !paragraph.isEmpty { out.append(.paragraph(paragraph.joined(separator: "\n"))); paragraph = [] }
            if !list.isEmpty { out.append(.list(list)); list = [] }
            if !quote.isEmpty { out.append(.quote(quote.joined(separator: "\n"))); quote = [] }
        }

        var i = 0
        while i < lines.count {
            let line = lines[i]
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            // Fenced code; an unclosed fence (a reply still streaming) runs to the end.
            if trimmed.hasPrefix("```") || trimmed.hasPrefix("~~~") {
                flush()
                let fence = String(trimmed.prefix(3))
                let language = String(trimmed.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                var body: [String] = []
                i += 1
                while i < lines.count, !lines[i].trimmingCharacters(in: .whitespaces).hasPrefix(fence) {
                    body.append(lines[i]); i += 1
                }
                out.append(.code(language: language, text: body.joined(separator: "\n")))
                i += 1
                continue
            }

            // Tables: a run of two or more lines with cells. The |---| row is optional, as people
            // paste tables without it; the first row is the header either way.
            if isTableLine(trimmed), i + 1 < lines.count, isTableLine(lines[i + 1].trimmingCharacters(in: .whitespaces)) {
                flush()
                var rows: [[String]] = []
                let firstRaw = trimmed
                while i < lines.count, isTableLine(lines[i].trimmingCharacters(in: .whitespaces)) {
                    let row = lines[i].trimmingCharacters(in: .whitespaces)
                    if !isSeparatorRow(row) { rows.append(cells(row)) }
                    i += 1
                }
                // A copied table often drags in the end of the sentence before it ("like | You want |
                // How |"): a header without the leading pipe, one cell wider than every row below.
                if rows.count > 1, !firstRaw.hasPrefix("|"), rows[0].count > 1,
                   rows.dropFirst().allSatisfy({ $0.count == rows[0].count - 1 }) {
                    out.append(.paragraph(rows[0][0]))
                    rows[0].removeFirst()
                }
                if !rows.isEmpty {
                    let width = rows.map(\.count).max() ?? 0
                    let padded = rows.map { $0 + Array(repeating: "", count: width - $0.count) }
                    // A column that's empty everywhere (a stray trailing pipe) is dropped.
                    let keep = (0..<width).filter { col in padded.contains { !$0[col].isEmpty } }
                    let kept = padded.map { row in keep.map { row[$0] } }
                    out.append(.table(header: kept[0], rows: Array(kept.dropFirst())))
                }
                continue
            }

            if trimmed.isEmpty {
                flush()
            } else if let h = trimmed.range(of: #"^#{1,6} +"#, options: .regularExpression) {
                flush()
                let level = trimmed[h].filter { $0 == "#" }.count
                out.append(.heading(level: level, text: String(trimmed[h.upperBound...])))
            } else if trimmed.range(of: #"^(-{3,}|\*{3,}|_{3,})$"#, options: .regularExpression) != nil {
                flush()
                out.append(.rule)
            } else if trimmed.hasPrefix(">") {
                if !paragraph.isEmpty || !list.isEmpty { let q = quote; flush(); quote = q }
                quote.append(String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces))
            } else if let item = listItem(line) {
                if !paragraph.isEmpty || !quote.isEmpty { let l = list; flush(); list = l }
                list.append(item)
            } else if !list.isEmpty, line.hasPrefix("  ") || line.hasPrefix("\t") {
                // A wrapped list item continues on an indented line.
                list[list.count - 1].text += " " + trimmed
            } else {
                if !list.isEmpty || !quote.isEmpty { flush() }
                paragraph.append(line)
            }
            i += 1
        }
        flush()
        return out
    }

    private static func listItem(_ line: String) -> MarkdownBlock.ListItem? {
        guard let r = line.range(of: #"^(\s*)([-*+•]|\d{1,3}[.)])\s+"#, options: .regularExpression) else { return nil }
        let prefix = line[r]
        let indent = prefix.prefix { $0 == " " || $0 == "\t" }.reduce(0) { $0 + ($1 == "\t" ? 4 : 1) }
        let marker = prefix.trimmingCharacters(in: .whitespaces)
        let shown = ["-", "*", "+", "•"].contains(marker) ? "•" : marker
        return MarkdownBlock.ListItem(marker: shown, text: String(line[r.upperBound...]), depth: min(indent / 2, 4))
    }

    private static func isTableLine(_ s: String) -> Bool {
        s.filter { $0 == "|" }.count >= 2 && !s.hasPrefix("```")
    }

    private static func isSeparatorRow(_ s: String) -> Bool {
        s.contains("-") && s.allSatisfy { "|-: ".contains($0) }
    }

    private static func cells(_ row: String) -> [String] {
        var parts = row.components(separatedBy: "|").map { $0.trimmingCharacters(in: .whitespaces) }
        if row.hasPrefix("|"), !parts.isEmpty { parts.removeFirst() }
        if row.hasSuffix("|"), !parts.isEmpty { parts.removeLast() }
        return parts
    }

    private static let inlineCache = NSCache<NSString, NSAttributedString>()

    /// Bold, italics, `code` and links, keeping line breaks.
    static func inline(_ text: String) -> AttributedString {
        if let hit = inlineCache.object(forKey: text as NSString) { return AttributedString(hit) }
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        var result = (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
        // Inline code reads as code: the kit's mono face on a faint chip.
        for run in result.runs where run.inlinePresentationIntent?.contains(.code) == true {
            result[run.range].font = .system(size: 12, design: .monospaced)
            result[run.range].backgroundColor = KitInk.raised
        }
        inlineCache.setObject(NSAttributedString(result), forKey: text as NSString)
        return result
    }
}

/// Renders markdown blocks with the kit's type and colors.
struct MarkdownView: View {
    let text: String

    /// Runs of headings, paragraphs and lists share one text view, so a drag selects across them;
    /// quotes, code and tables are their own parts.
    private enum Segment {
        case prose([MarkdownBlock])
        case block(MarkdownBlock)
    }

    private var segments: [Segment] {
        var out: [Segment] = []
        var prose: [MarkdownBlock] = []
        for block in MarkdownParser.blocks(text) {
            switch block {
            case .heading, .paragraph, .list: prose.append(block)
            default:
                if !prose.isEmpty { out.append(.prose(prose)); prose = [] }
                out.append(.block(block))
            }
        }
        if !prose.isEmpty { out.append(.prose(prose)) }
        return out
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(segments.enumerated()), id: \.offset) { _, segment in
                switch segment {
                case .prose(let blocks): ProseText(blocks: blocks)
                case .block(let block): blockView(block)
                }
            }
        }
        .tint(KitInk.accentText)
        .textSelection(.enabled)
    }

    @ViewBuilder private func blockView(_ block: MarkdownBlock) -> some View {
        switch block {
        case .heading(let level, let text):
            Text(MarkdownParser.inline(text))
                .font(.system(size: level == 1 ? 18 : level == 2 ? 16 : level == 3 ? 14 : 13, weight: level <= 2 ? .bold : .semibold))
                .foregroundColor(KitInk.text)
                .padding(.top, level <= 2 ? 4 : 2)
        case .paragraph(let text):
            Text(MarkdownParser.inline(text))
                .kitType(.body).foregroundColor(KitInk.text)
                .lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
        case .list(let items):
            VStack(alignment: .leading, spacing: 5) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(item.marker)
                            .font(.system(size: 13, weight: item.marker == "•" ? .bold : .semibold))
                            .foregroundColor(item.marker == "•" ? KitInk.accent : KitInk.textMuted)
                            .frame(minWidth: 14, alignment: .trailing)
                        Text(MarkdownParser.inline(item.text))
                            .kitType(.body).foregroundColor(KitInk.text)
                            .lineSpacing(3)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.leading, CGFloat(item.depth) * 18)
                }
            }
        case .quote(let text):
            HStack(alignment: .top, spacing: 10) {
                RoundedRectangle(cornerRadius: 2).fill(KitInk.accent.opacity(0.6)).frame(width: 3)
                ProseText(blocks: [.paragraph(text)], muted: true)
            }
            .fixedSize(horizontal: false, vertical: true)
        case .code(let language, let text):
            CodeBlockView(language: language, text: text)
        case .table(let header, let rows):
            TableBlockView(header: header, rows: rows)
        case .rule:
            Rectangle().fill(KitInk.hairline).frame(height: 1).padding(.vertical, 4)
        }
    }
}

/// Fenced code with its language and a Copy button.
struct CodeBlockView: View {
    let language: String
    let text: String
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(language.isEmpty ? "code" : language).kitType(.meta).foregroundColor(KitInk.textMuted)
                Spacer()
                CopyButton(text: text)
            }
            .padding(.leading, 12).padding(.trailing, 6).frame(height: 28)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            ScrollView(.horizontal, showsIndicators: false) {
                Text(text).kitType(.mono).foregroundColor(KitInk.text)
                    .textSelection(.enabled)
                    .padding(12)
            }
        }
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).fill(KitInk.raised.opacity(0.7)))
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).strokeBorder(KitInk.hairline, lineWidth: 1))
    }
}

/// A real table: header row, row lines, cells that wrap.
struct TableBlockView: View {
    let header: [String]
    let rows: [[String]]

    var body: some View {
        // Fits the message width and wraps cells; a horizontal scroll view measured each cell as one
        // line, so wrapped text spilled over the next row.
        Grid(alignment: .topLeading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(Array(header.enumerated()), id: \.offset) { _, cell in
                        cellView(cell, header: true, fill: KitInk.raised)
                    }
                }
                ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                    Rectangle().fill(KitInk.hairline).frame(height: 1).gridCellUnsizedAxes(.horizontal)
                    GridRow {
                        ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                            cellView(cell, header: false, fill: index % 2 == 1 ? KitInk.raised.opacity(0.35) : Color.clear)
                        }
                    }
                }
            }
            .background(KitInk.card)
            .clipShape(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).strokeBorder(KitInk.hairline, lineWidth: 1))
            .fixedSize(horizontal: false, vertical: true)
    }

    /// Each cell fills its row's height, so a short cell's shading reaches the row line.
    private func cellView(_ text: String, header: Bool, fill: Color) -> some View {
        Text(MarkdownParser.inline(text))
            .font(.system(size: 12.5, weight: header ? .semibold : .regular))
            .foregroundColor(header ? KitInk.text : KitInk.text)
            .lineSpacing(2)
            .fixedSize(horizontal: false, vertical: true)
            .frame(minWidth: 60, maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 12).padding(.vertical, 8)
            .frame(maxHeight: .infinity, alignment: .topLeading)
            .background(fill)
    }
}

/// Copies text; says Copied for a moment.
struct CopyButton: View {
    let text: String
    var label = "Copy"
    @State private var copied = false

    var body: some View {
        Button {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            copied = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.4) { copied = false }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: copied ? "checkmark" : "doc.on.doc").font(.system(size: 10, weight: .medium))
                Text(copied ? "Copied" : label)
            }
        }
        .buttonStyle(.kit(.ghost))
        .controlSize(.small)
        .help("Copy to the clipboard")
    }
}

/// Headings, paragraphs and lists as one native text view: selectable across lines and blocks like
/// any Mac text (SwiftUI's selectable Text stops at its own edges), with clickable links that go
/// through the view's `openURL`.
struct ProseText: NSViewRepresentable {
    let blocks: [MarkdownBlock]
    var muted = false

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ProseTextView {
        let view = ProseTextView(usingTextLayoutManager: false)
        view.isEditable = false
        view.isSelectable = true
        view.isRichText = true
        view.drawsBackground = false
        view.textContainerInset = .zero
        view.textContainer?.lineFragmentPadding = 0
        view.textContainer?.widthTracksTextView = false
        view.isVerticallyResizable = false
        view.isHorizontallyResizable = false
        view.linkTextAttributes = [.foregroundColor: NSColor(KitInk.accentText), .cursor: NSCursor.pointingHand]
        view.delegate = context.coordinator
        return view
    }

    func updateNSView(_ view: ProseTextView, context: Context) {
        context.coordinator.openURL = context.environment.openURL
        // Only new text replaces the old: a refresh mustn't drop what you've selected.
        guard context.coordinator.shown != blocks || context.coordinator.muted != muted else { return }
        context.coordinator.shown = blocks
        context.coordinator.muted = muted
        view.textStorage?.setAttributedString(Self.text(blocks, muted: muted))
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView view: ProseTextView, context: Context) -> CGSize? {
        var width = proposal.width ?? 560
        if !width.isFinite { width = 560 }
        let used = view.layout(width: width)
        // Short text hugs its width (a one-line reply bubble); long text wraps at the offered width.
        return CGSize(width: min(width, ceil(used.width) + 1), height: ceil(used.height))
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var shown: [MarkdownBlock]?
        var muted = false
        var openURL: OpenURLAction?

        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            let url = (link as? URL) ?? (link as? String).flatMap(URL.init(string:))
            guard let url, let openURL else { return false }
            openURL(url)
            return true
        }
    }

    // MARK: Building the text

    private static func text(_ blocks: [MarkdownBlock], muted: Bool) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let color = NSColor(muted ? KitInk.textMuted : KitInk.text)
        for (index, block) in blocks.enumerated() {
            let last = index == blocks.count - 1
            switch block {
            case .heading(let level, let text):
                let size: CGFloat = level == 1 ? 18 : level == 2 ? 16 : level == 3 ? 14 : 13
                let font = NSFont.systemFont(ofSize: size, weight: level <= 2 ? .bold : .semibold)
                let style = NSMutableParagraphStyle()
                style.paragraphSpacingBefore = index == 0 ? 0 : (level <= 2 ? 8 : 4)
                style.paragraphSpacing = 6
                out.append(inline(text, font: font, color: color, style: style))
            case .paragraph(let text):
                let style = NSMutableParagraphStyle()
                style.lineSpacing = 3
                style.paragraphSpacing = 10
                out.append(inline(text, font: .systemFont(ofSize: 13), color: color, style: style))
            case .list(let items):
                for (i, item) in items.enumerated() {
                    let indent = CGFloat(item.depth) * 18
                    let style = NSMutableParagraphStyle()
                    style.lineSpacing = 3
                    style.paragraphSpacing = i == items.count - 1 ? 10 : 5
                    style.firstLineHeadIndent = indent
                    style.headIndent = indent + 22
                    style.tabStops = [NSTextTab(textAlignment: .left, location: indent + 22)]
                    let bullet = item.marker == "•"
                    out.append(NSAttributedString(string: item.marker + "\t", attributes: [
                        .font: NSFont.systemFont(ofSize: 13, weight: bullet ? .bold : .semibold),
                        .foregroundColor: NSColor(bullet ? KitInk.accent : KitInk.textMuted),
                        .paragraphStyle: style,
                    ]))
                    out.append(inline(item.text, font: .systemFont(ofSize: 13), color: color, style: style))
                    if i < items.count - 1 { out.append(NSAttributedString(string: "\n", attributes: [.paragraphStyle: style])) }
                }
            default:
                continue
            }
            if !last { out.append(NSAttributedString(string: "\n", attributes: [.font: NSFont.systemFont(ofSize: 13)])) }
        }
        return out
    }

    /// Bold, italics, `code`, strikethrough and links from Foundation's inline markdown parser.
    private static func inline(_ text: String, font: NSFont, color: NSColor, style: NSParagraphStyle) -> NSAttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        let parsed = (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
        let out = NSMutableAttributedString()
        for run in parsed.runs {
            let piece = String(parsed[run.range].characters)
            var runFont = font
            var attrs: [NSAttributedString.Key: Any] = [.foregroundColor: color, .paragraphStyle: style]
            if let intent = run.inlinePresentationIntent {
                if intent.contains(.stronglyEmphasized) { runFont = NSFontManager.shared.convert(runFont, toHaveTrait: .boldFontMask) }
                if intent.contains(.emphasized) { runFont = NSFontManager.shared.convert(runFont, toHaveTrait: .italicFontMask) }
                if intent.contains(.strikethrough) { attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
                if intent.contains(.code) {
                    runFont = .monospacedSystemFont(ofSize: font.pointSize - 1, weight: .regular)
                    attrs[.backgroundColor] = NSColor(KitInk.raised)
                }
            }
            if let link = run.link { attrs[.link] = link }
            attrs[.font] = runFont
            out.append(NSAttributedString(string: piece, attributes: attrs))
        }
        return out
    }
}

/// The text view behind `ProseText`: lays out at the width SwiftUI gives it and lets go of its
/// selection when another one takes focus, so only one block shows a selection at a time.
final class ProseTextView: NSTextView {
    /// Lays the text out at `width` and returns the space it uses.
    func layout(width: CGFloat) -> CGSize {
        guard let container = textContainer, let manager = layoutManager else { return .zero }
        if container.containerSize.width != width {
            container.containerSize = NSSize(width: width, height: .greatestFiniteMagnitude)
        }
        manager.ensureLayout(for: container)
        return manager.usedRect(for: container).size
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        textContainer?.containerSize = NSSize(width: newSize.width, height: .greatestFiniteMagnitude)
    }

    /// A drag selects even while the window is in the background.
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func resignFirstResponder() -> Bool {
        let done = super.resignFirstResponder()
        if done { setSelectedRange(NSRange(location: selectedRange().location, length: 0)) }
        return done
    }
}
