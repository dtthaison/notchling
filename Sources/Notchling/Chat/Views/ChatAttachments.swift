import AppKit
import SwiftUI

/// Files dropped on the reply box, as small thumbnails above the text instead of their paths. The paths
/// still go to the session, after the text, when the reply is sent (`ChatModel.composed`).
struct AttachmentStrip: View {
    @ObservedObject var model: ChatModel
    let id: String
    let urls: [URL]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(urls, id: \.self) { url in
                    AttachmentThumb(url: url,
                                    open: { withAnimation(KitMotion.appear) { model.file = url } },
                                    remove: { model.detach(url, for: id) })
                }
            }
            .padding(.horizontal, 12).padding(.top, 10)
        }
        .part("Attachments", .topTrailing)
    }
}

private struct AttachmentThumb: View {
    let url: URL
    let open: () -> Void
    let remove: () -> Void
    @State private var thumb: NSImage?
    @State private var hover = false

    static let side: CGFloat = 52

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Button(action: open) {
                Group {
                    if let thumb {
                        Image(nsImage: thumb).resizable().aspectRatio(contentMode: .fill)
                    } else {
                        VStack(spacing: 2) {
                            Image(nsImage: NSWorkspace.shared.icon(forFile: url.path)).resizable().frame(width: 26, height: 26)
                            Text(url.pathExtension.uppercased()).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                        }
                    }
                }
                .frame(width: Self.side, height: Self.side)
                .background(KitInk.raised)
                .clipShape(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).strokeBorder(KitInk.hairline))
            }
            .buttonStyle(.kitPlain)
            .help(url.lastPathComponent)
            .accessibilityLabel("Attached \(url.lastPathComponent)")
            Button(action: remove) {
                Image(systemName: "xmark").font(.system(size: 7, weight: .bold))
                    .foregroundColor(KitInk.text)
                    .frame(width: 16, height: 16)
                    .background(Circle().fill(KitInk.card))
                    .overlay(Circle().strokeBorder(KitInk.hairline))
            }
            .buttonStyle(.kitPlain)
            .offset(x: 5, y: -5)
            .opacity(hover ? 1 : 0)
            .help("Remove")
            .accessibilityLabel("Remove \(url.lastPathComponent)")
        }
        .padding(.top, 5).padding(.trailing, 5)
        .onHover { hover = $0 }
        .animation(KitMotion.hover, value: hover)
        .task(id: url) {
            guard let kind = ChatMedia.kind(of: url.path) else { return }
            thumb = await MediaThumbs.load(ChatMedia.Item(path: url.path, kind: kind))?.image
        }
    }
}
