import ConsoleCore
import SwiftUI

/// The question or permission a Notchling hook is holding open, above the input. Answers go through
/// HookResponder into Notchling's responses folder. Number keys 1-9 pick options.
struct PendingCard: View {
    let prompt: PendingPrompt
    let error: String?
    let onAllow: () -> Void
    let onDeny: () -> Void
    let onAnswer: ([String]) -> Void

    @State private var step = 0
    @State private var picks: [String] = []

    var body: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            HStack(spacing: Space.sm) {
                Image(systemName: "exclamationmark.bubble.fill").font(.system(size: 11)).foregroundStyle(Token.statusInput)
                    .accessibilityHidden(true)
                Text(title).font(TypeToken.labelSm).foregroundStyle(Token.statusInput)
                Spacer()
            }
            switch prompt.kind {
            case .permission: permissionBody
            case .question(let questions): questionBody(questions)
            }
            if let error {
                Text(error).font(TypeToken.bodySm).foregroundStyle(Token.statusError)
            }
        }
        .padding(Space.md)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Token.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.lg))
        .overlay(alignment: .leading) {
            Token.statusInput.frame(width: 2).clipShape(RoundedRectangle(cornerRadius: 1))
        }
        .overlay(RoundedRectangle(cornerRadius: Radius.lg).stroke(Token.border, lineWidth: 1))
        .panelEntrance()
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Đang chờ bạn: \(prompt.summary)")
    }

    private var title: String {
        switch prompt.kind {
        case .permission: return "Cần permission: \(prompt.toolName)"
        case .question: return "Câu hỏi từ Claude"
        }
    }

    private var permissionBody: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            Text(prompt.summary)
                .font(TypeToken.codeSm).foregroundStyle(Token.onSurface)
                .lineLimit(4).textSelection(.enabled)
            HStack(spacing: Space.sm) {
                Button("Cho phép", action: onAllow).buttonStyle(PrimaryButtonStyle())
                    .accessibilityLabel("Cho phép \(prompt.toolName)")
                Button("Từ chối", action: onDeny).buttonStyle(SecondaryButtonStyle())
                    .accessibilityLabel("Từ chối \(prompt.toolName)")
            }
        }
    }

    @ViewBuilder
    private func questionBody(_ questions: [PromptQuestion]) -> some View {
        if questions.contains(where: \.multiSelect) {
            Text("Câu hỏi này cho chọn nhiều đáp án. Trả lời trên terminal.")
                .font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
            Text(questions[0].question).font(TypeToken.bodySm).foregroundStyle(Token.muted)
        } else if step < questions.count {
            let question = questions[step]
            VStack(alignment: .leading, spacing: Space.sm) {
                if !question.header.isEmpty {
                    Text(question.header).font(TypeToken.labelSm).foregroundStyle(Token.muted)
                }
                Text(question.question).bodyMdStyle().fixedSize(horizontal: false, vertical: true)
                FlowOptions(options: Array(question.options.prefix(9).enumerated()).map { ($0.offset, $0.element) }) { index, label in
                    pick(label, of: questions.count)
                }
            }
        }
    }

    private func pick(_ label: String, of total: Int) {
        let next = picks + [label]
        if next.count == total {
            onAnswer(next)
            step = 0
            picks = []
        } else {
            picks = next
            step += 1
        }
    }
}

/// Option buttons in a wrapping row. Each shows its number key.
private struct FlowOptions: View {
    let options: [(Int, String)]
    let pick: (Int, String) -> Void

    var body: some View {
        WrappingHStack(spacing: Space.sm) {
            ForEach(options, id: \.0) { index, label in
                Button { pick(index, label) } label: {
                    HStack(spacing: 6) {
                        Text("\(index + 1)").font(TypeToken.codeSm).foregroundStyle(Token.muted)
                        Text(label)
                    }
                }
                .buttonStyle(SecondaryButtonStyle())
                .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: [])
                .accessibilityLabel("Lựa chọn \(index + 1): \(label)")
            }
        }
    }
}

struct WrappingHStack: Layout {
    var spacing: CGFloat = Space.sm

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(width: proposal.width ?? .infinity, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = arrange(width: bounds.width, subviews: subviews)
        for (index, origin) in result.origins.enumerated() {
            subviews[index].place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        var origins: [CGPoint] = []
        var x: CGFloat = 0, y: CGFloat = 0, rowHeight: CGFloat = 0, maxX: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width { x = 0; y += rowHeight + spacing; rowHeight = 0 }
            origins.append(CGPoint(x: x, y: y))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
            maxX = max(maxX, x - spacing)
        }
        return (CGSize(width: maxX, height: y + rowHeight), origins)
    }
}
