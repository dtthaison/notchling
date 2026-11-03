import AppKit
import SwiftUI

/// Basic markdown for chat: **bold**, *italic*, `inline code`, links, and ``` fenced code blocks in
/// Geist Mono. Anything else shows as written.
struct MarkdownText: View {
    let text: String

    private enum Segment: Identifiable {
        case prose(Int, String)
        case code(Int, String)

        var id: Int {
            switch self {
            case .prose(let index, _), .code(let index, _): return index
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            ForEach(Self.segments(of: text)) { segment in
                switch segment {
                case .prose(_, let prose):
                    Text(Self.attributed(prose))
                        .font(TypeToken.bodyMd).lineSpacing(4)
                        .foregroundStyle(Token.onSurface)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                case .code(_, let code):
                    ScrollView(.horizontal, showsIndicators: false) {
                        Text(code)
                            .font(TypeToken.codeSm).lineSpacing(3)
                            .foregroundStyle(Token.onSurface)
                            .textSelection(.enabled)
                            .padding(Space.sm + 2)
                            .fixedSize(horizontal: true, vertical: false)
                    }
                    .background(Token.neutral, in: RoundedRectangle(cornerRadius: Radius.md))
                    .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(Token.border, lineWidth: 1))
                }
            }
        }
    }

    private static func segments(of text: String) -> [Segment] {
        var result: [Segment] = []
        var proseLines: [Substring] = []
        var codeLines: [Substring] = []
        var inCode = false
        func flushProse() {
            let joined = proseLines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            if !joined.isEmpty { result.append(.prose(result.count, joined)) }
            proseLines = []
        }
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                if inCode {
                    result.append(.code(result.count, codeLines.joined(separator: "\n")))
                    codeLines = []
                } else {
                    flushProse()
                }
                inCode.toggle()
            } else if inCode {
                codeLines.append(line)
            } else {
                proseLines.append(line)
            }
        }
        if inCode { result.append(.code(result.count, codeLines.joined(separator: "\n"))) } else { flushProse() }
        return result
    }

    private static func attributed(_ text: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace, failurePolicy: .returnPartiallyParsedIfPossible)
        guard var result = try? AttributedString(markdown: text, options: options) else { return AttributedString(text) }
        for run in result.runs {
            let intent = run.inlinePresentationIntent ?? []
            if intent.contains(.code) {
                result[run.range].font = TypeToken.codeSm
                result[run.range].backgroundColor = Token.neutral
            } else if intent.contains(.stronglyEmphasized) {
                result[run.range].font = TypeToken.bodyMdStrong
            }
            if run.link != nil { result[run.range].foregroundColor = Token.primary }
        }
        return result
    }
}
