import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// Claude's message: markdown on a card, with Copy and the time on hover, sitting just above the card's
/// top-right corner so they never cover the text.
struct AssistantMessage: View {
    let text: String
    let date: Date?
    /// The conversation item's id, which a Second Opinion on this message hangs its card under.
    var anchor: String? = nil
    @State private var overCard = false
    @State private var overBar = false
    @State private var shown = false
    @State private var asking = false

    var body: some View {
        // Full width, no avatar: the card and its place on the left already say who wrote it.
        HStack(alignment: .top, spacing: 0) {
            MarkdownView(text: text)
                .padding(.horizontal, 14).padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .kitCard()
                .overlay(alignment: .topTrailing) {
                    if shown {
                        HStack(spacing: 2) {
                            if let date { Text(ChatFormat.time(date)).kitType(.meta).foregroundColor(KitInk.textFaint).padding(.horizontal, 4) }
                            if let anchor { SecondOpinionButton(subject: .message(text), anchor: anchor, isOpen: $asking) }
                            CopyButton(text: text)
                        }
                        .fixedSize()
                        .padding(2)
                        .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm).fill(KitInk.card))
                        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusSm).stroke(KitInk.hairline, lineWidth: 1))
                        // Its bottom edge on the card's top border, at the right.
                        .alignmentGuide(.top) { $0[.bottom] + 2 }
                        .padding(.trailing, 10)
                        .onHover { overBar = $0; update() }
                        .transition(.opacity)
                    }
                }
        }
        .onHover { overCard = $0; update() }
        .onChange(of: asking) { _ in update() }
    }

    /// Moving from the card up to the bar leaves one before entering the other: hide only once the
    /// pointer has been off both for a moment.
    private func update() {
        if overCard || overBar || asking { shown = true; return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            if !overCard && !overBar && !asking { shown = false }
        }
    }
}

struct UserBubble: View {
    let text: String
    let images: Int
    let date: Date?
    let note: String?
    var onEdit: (() -> Void)? = nil
    @State private var hovering = false

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Spacer(minLength: 120)
            if hovering, note == nil {
                VStack(alignment: .trailing, spacing: 2) {
                    CopyButton(text: text)
                    if let date { Text(ChatFormat.time(date)).kitType(.meta).foregroundColor(KitInk.textFaint) }
                }
            }
            VStack(alignment: .trailing, spacing: 4) {
                CappedWidth(max: 720) {
                VStack(alignment: .leading, spacing: 6) {
                    if images > 0 {
                        Label(images == 1 ? "1 image" : "\(images) images", systemImage: "photo")
                            .kitType(.label).foregroundColor(KitInk.accentText)
                    }
                    if text.hasPrefix("! ") {
                        // A shell command: "!" in the shell color, the command in mono.
                        (Text("! ").foregroundColor(KitInk.shell) + Text(text.dropFirst(2)).foregroundColor(KitInk.text))
                            .kitType(.mono)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    } else if Self.isPlain(text) {
                        // Hugs its text, so a short message is a short bubble at full window width.
                        Text(text).kitType(.body).foregroundColor(KitInk.text)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    } else if !text.isEmpty {
                        // Your own messages can hold tables and code too (pastes), so they get markdown.
                        MarkdownView(text: text)
                    }
                }
                }
                .padding(.horizontal, 14).padding(.vertical, 10)
                .background(RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(text.hasPrefix("! ") ? KitInk.shellWash : KitInk.accentWash))
                if let note {
                    HStack(spacing: 8) {
                        Text(note).kitType(.meta).foregroundColor(KitInk.textFaint)
                        if let onEdit {
                            Button("Edit", action: onEdit)
                                .buttonStyle(.kitPlain)
                                .kitType(.meta).foregroundColor(KitInk.accentText)
                                .help("Take it back to the reply box to change it (↑). Send it again when it's right.")
                                .part("Edit Queued Button", .top)
                        }
                    }
                }
            }
            // Gets its width before the spacer, so a short reply stays on one line.
            .layoutPriority(1)
        }
        .onHover { hovering = $0 }
    }

    /// One paragraph with nothing markdown would draw differently.
    static func isPlain(_ text: String) -> Bool {
        !text.isEmpty && !text.contains(where: { "\n`*_[|#>".contains($0) })
    }
}

/// Offers its content at most `max` points and takes only what the content uses, so a short text
/// stays short (`.frame(maxWidth:)` would always grow to the full `max`).
private struct CappedWidth: Layout {
    let max: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = Swift.min(proposal.width ?? max, max)
        return subviews.first?.sizeThatFits(ProposedViewSize(width: width, height: proposal.height)) ?? .zero
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: bounds.height))
    }
}
