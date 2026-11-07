import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// A saved Artifact beside the conversation, opened from a link in it. Drag its left edge to resize;
/// Escape or × closes it.
struct PagePanel: View {
    @ObservedObject var model: ChatModel
    let page: PageLibrary.Entry
    let maxWidth: CGFloat

    var body: some View {
        SidePanel(maxWidth: maxWidth) {
                HStack(spacing: 8) {
                    Image(systemName: "doc.richtext").font(.system(size: 12, weight: .medium)).foregroundColor(KitInk.accentText)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(page.title).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                        Text("Saved copy · \(PageDates.short(page.updated))").kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                    }
                    Spacer(minLength: 4)
                    Button { _ = model.onOpenArtifact?(page.folder) } label: {
                        Image(systemName: "macwindow").font(.system(size: 12, weight: .medium))
                    }
                    .buttonStyle(.kit(.ghost)).help("Open in the Artifacts window")
                    if let link = URL(string: page.url), !page.url.isEmpty {
                        Button { NSWorkspace.shared.open(link) } label: {
                            Image(systemName: "safari").font(.system(size: 12, weight: .medium))
                        }
                        .buttonStyle(.kit(.ghost)).help("Open on claude.ai")
                    }
                    Button { withAnimation(KitMotion.appear) { model.page = nil } } label: {
                        Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
                    }
                    .buttonStyle(.kit(.ghost)).help("Close (Esc)")
                }
                .padding(.leading, 14).padding(.trailing, 8).frame(height: 48)
        } content: {
            PagePreview(page: page)
        }
    }
}
