import AppKit
import ConsoleCore
import SwiftUI

/// Decodes and scales pictures off the main thread and keeps the results, up to a cap.
final class PictureCache: @unchecked Sendable {
    static let shared = PictureCache()
    private let cache = NSCache<NSString, NSImage>()

    init() { cache.totalCostLimit = 96 * 1024 * 1024 }

    func image(for ref: ImageRef, transcript: URL?, maxWidth: Int) async -> NSImage? {
        let key = "\(transcript?.path ?? ""):\(ref.id):\(maxWidth)" as NSString
        if let hit = cache.object(forKey: key) { return hit }
        let image: NSImage? = await Task.detached(priority: .utility) {
            do {
                let original = try ChatImages.data(for: ref, transcript: transcript ?? URL(fileURLWithPath: "/dev/null"))
                let jpeg = try ChatImages.jpeg(from: original, maxWidth: maxWidth)
                return NSImage(data: jpeg)
            } catch {
                ConsoleLog.error("PictureCache.image", ref.id, "\(error)")
                return nil
            }
        }.value
        if let image { cache.setObject(image, forKey: key, cost: Int(image.size.width * image.size.height * 4)) }
        return image
    }
}

/// The pictures of a message as thumbnails (at most 240 pt wide, aspect kept); a click opens the full-size viewer.
struct ChatPictures: View {
    let images: [ImageRef]
    let transcript: URL?
    @State private var viewing: ImageRef?

    var body: some View {
        if !images.isEmpty {
            VStack(alignment: .leading, spacing: Space.sm) {
                ForEach(images, id: \.id) { ref in
                    Thumbnail(ref: ref, transcript: transcript) { viewing = ref }
                }
            }
            .sheet(item: Binding(get: { viewing.map(Viewing.init) }, set: { viewing = $0?.ref })) { item in
                PictureViewer(ref: item.ref, transcript: transcript) { viewing = nil }
            }
        }
    }

    private struct Viewing: Identifiable { let ref: ImageRef; var id: String { ref.id } }
}

private struct Thumbnail: View {
    let ref: ImageRef
    let transcript: URL?
    let open: () -> Void
    @State private var image: NSImage?
    @State private var failed = false

    var body: some View {
        Button(action: open) {
            Group {
                if let image {
                    Image(nsImage: image).resizable().scaledToFit()
                } else {
                    RoundedRectangle(cornerRadius: Radius.md).fill(Token.surface)
                        .overlay { if failed { Image(systemName: "photo.badge.exclamationmark").foregroundStyle(Token.muted) } else { ProgressView().controlSize(.small) } }
                        .aspectRatio(CGFloat(ref.width ?? 4) / CGFloat(max(ref.height ?? 3, 1)), contentMode: .fit)
                }
            }
            .frame(maxWidth: 240)
            .clipShape(RoundedRectangle(cornerRadius: Radius.md))
            .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(Token.border, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Ảnh, mở bản lớn")
        .task(id: ref.id) {
            image = await PictureCache.shared.image(for: ref, transcript: transcript, maxWidth: 480)
            failed = image == nil
        }
    }
}

private struct PictureViewer: View {
    let ref: ImageRef
    let transcript: URL?
    let close: () -> Void
    @State private var image: NSImage?

    var body: some View {
        VStack(spacing: Space.md) {
            HStack {
                Text(ref.path.map { ($0 as NSString).lastPathComponent } ?? "Ảnh").font(TypeToken.titleMd).foregroundStyle(Token.onSurface)
                Spacer()
                Button("Đóng", action: close).buttonStyle(SecondaryButtonStyle()).keyboardShortcut(.cancelAction)
            }
            Group {
                if let image {
                    Image(nsImage: image).resizable().scaledToFit()
                } else {
                    ProgressView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(Space.lg)
        .frame(minWidth: 640, minHeight: 480)
        .background(Token.surface)
        .preferredColorScheme(.dark)
        .task { image = await PictureCache.shared.image(for: ref, transcript: transcript, maxWidth: 1600) }
    }
}
