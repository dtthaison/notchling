import SwiftUI
import AVKit
import ImageIO

/// Videos and images a message names by path, shown as cards under the text. Detection is
/// `ChatPaths`' (a token with a slash that exists on disk, read against the folders the message
/// names, then the session's), narrowed by extension.
enum ChatMedia {
    enum Kind { case video, image }

    struct Item: Equatable, Hashable {
        let path: String
        let kind: Kind
    }

    /// webm is tried too; a file AVFoundation can't play gets no card (the path link still opens it).
    static let videoExtensions: Set<String> = ["mp4", "mov", "m4v", "webm"]
    static let imageExtensions: Set<String> = ["png", "jpg", "jpeg", "gif", "heic", "webp"]
    /// Cards per message; more would bury the text.
    static let perMessage = 4

    static func kind(of path: String) -> Kind? {
        let ext = (path as NSString).pathExtension.lowercased()
        if videoExtensions.contains(ext) { return .video }
        if imageExtensions.contains(ext) { return .image }
        return nil
    }

    /// Media files the text names, in order, without repeats or anything in `skipping`.
    static func find(in text: String, bases: [String], skipping: Set<String> = [], limit: Int = perMessage,
                     exists: ChatPaths.Exists = ChatPaths.onDisk) -> [Item] {
        var seen = skipping
        var out: [Item] = []
        for (_, path) in ChatPaths.links(in: text, bases: bases, exists: exists) {
            guard out.count < limit, let kind = kind(of: path), seen.insert(path).inserted else { continue }
            out.append(Item(path: path, kind: kind))
        }
        return out
    }
}

/// A poster or thumbnail, with what the card shows beside it.
struct MediaThumb {
    let image: NSImage
    let pixelSize: CGSize
    let duration: Double?
    var aspect: CGFloat { pixelSize.height > 0 ? max(0.2, min(5, pixelSize.width / pixelSize.height)) : 16.0 / 9 }
}

/// Posters and thumbnails, made off the main thread and kept until the file changes.
enum MediaThumbs {
    private final class Box { let thumb: MediaThumb?; init(_ thumb: MediaThumb?) { self.thumb = thumb } }
    private static let cache: NSCache<NSString, Box> = {
        let c = NSCache<NSString, Box>()
        c.countLimit = 120
        return c
    }()
    private static let maxPixels: CGFloat = 960

    static func load(_ item: ChatMedia.Item) async -> MediaThumb? {
        let url = URL(fileURLWithPath: item.path)
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])
        let key = "\(item.path)|\(values?.fileSize ?? 0)|\(values?.contentModificationDate?.timeIntervalSince1970 ?? 0)" as NSString
        if let hit = cache.object(forKey: key) { return hit.thumb }
        let made = await Task.detached(priority: .utility) { () -> MediaThumb? in
            switch item.kind {
            case .video: return await poster(url)
            case .image: return thumbnail(url)
            }
        }.value
        cache.setObject(Box(made), forKey: key)
        return made
    }

    private static func poster(_ url: URL) async -> MediaThumb? {
        let asset = AVURLAsset(url: url)
        guard (try? await asset.load(.isPlayable)) == true else { return nil }
        let seconds = (try? await asset.load(.duration))?.seconds
        let duration = seconds.flatMap { $0.isFinite && $0 > 0 ? $0 : nil }
        let generator = AVAssetImageGenerator(asset: asset)
        generator.appliesPreferredTrackTransform = true
        generator.maximumSize = CGSize(width: maxPixels, height: maxPixels)
        // A little in, so a fade-in from black isn't the poster.
        let candidates = [min(1, (duration ?? 0) / 10), 0]
        for t in candidates {
            if let (cg, _) = try? await generator.image(at: CMTime(seconds: t, preferredTimescale: 600)) {
                let size = CGSize(width: cg.width, height: cg.height)
                return MediaThumb(image: NSImage(cgImage: cg, size: size), pixelSize: size, duration: duration)
            }
        }
        return nil
    }

    private static func thumbnail(_ url: URL) -> MediaThumb? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        var pixels = CGSize.zero
        if let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
           let w = props[kCGImagePropertyPixelWidth] as? CGFloat, let h = props[kCGImagePropertyPixelHeight] as? CGFloat {
            pixels = CGSize(width: w, height: h)
        }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixels,
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        let shown = CGSize(width: cg.width, height: cg.height)
        // Report the original size for the badge, but keep the thumbnail's orientation for the aspect.
        let original = pixels == .zero ? shown : (shown.width >= shown.height) == (pixels.width >= pixels.height) ? pixels : CGSize(width: pixels.height, height: pixels.width)
        return MediaThumb(image: NSImage(cgImage: cg, size: shown), pixelSize: original, duration: nil)
    }
}

/// The cards for a message's media, stacked.
struct MediaPreviews: View {
    let items: [ChatMedia.Item]

    var body: some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(items, id: \.self) { MediaPreviewCard(item: $0) }
            }
        }
    }
}

/// The media in a tool's output text, read against the session's folder.
struct OutputMediaPreviews: View {
    let text: String
    @Environment(\.chatFolder) private var folder

    var body: some View {
        MediaPreviews(items: ChatMedia.find(in: text, bases: ChatPaths.bases(in: text, cwd: folder)))
    }
}

/// A poster (or thumbnail) with the file name; a video plays in place on click, and a button opens
/// the file beside the chat.
struct MediaPreviewCard: View {
    let item: ChatMedia.Item
    @Environment(\.openURL) private var openURL
    @State private var thumb: MediaThumb?
    @State private var loaded = false
    @State private var playing = false

    private static let maxWidth: CGFloat = 480
    private static let maxHeight: CGFloat = 360

    private var url: URL { URL(fileURLWithPath: item.path) }
    private var name: String { url.lastPathComponent }

    var body: some View {
        Group {
            if loaded && thumb == nil {
                EmptyView()   // gone, or not something AVFoundation / ImageIO can read
            } else {
                card
            }
        }
        .task(id: item) {
            loaded = false
            playing = false
            thumb = await MediaThumbs.load(item)
            loaded = true
        }
    }

    private var card: some View {
        ZStack {
            if playing {
                InlinePlayer(url: url)
            } else if let thumb {
                Button { if item.kind == .video { playing = true } else { open() } } label: {
                    Image(nsImage: thumb.image).resizable().scaledToFit()
                        .overlay(alignment: .center) { if item.kind == .video { playBadge } }
                        .overlay(alignment: .bottom) { caption(thumb) }
                }
                .buttonStyle(.kitPlain)
                .help(item.kind == .video ? "Play" : "Open beside the chat")
            } else {
                KitInk.raised.overlay(ProgressView().controlSize(.small))
            }
        }
        .aspectRatio(thumb?.aspect ?? 16.0 / 9, contentMode: .fit)
        .frame(maxWidth: Self.maxWidth, maxHeight: Self.maxHeight)
        .background(KitInk.terminal)
        .clipShape(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).strokeBorder(KitInk.hairline, lineWidth: 1))
        .overlay(alignment: .topTrailing) {
            Button(action: open) {
                Image(systemName: "sidebar.right").font(.system(size: 11, weight: .medium))
                    .foregroundColor(KitInk.terminalText)
                    .frame(width: 24, height: 24)
                    .background(Circle().fill(KitInk.terminal.opacity(0.65)))
            }
            .buttonStyle(.kitPlain)
            .padding(6)
            .help("Open beside the chat")
        }
        .part(item.kind == .video ? "Video Preview" : "Image Preview")
    }

    private var playBadge: some View {
        Image(systemName: "play.fill").font(.system(size: 16, weight: .semibold))
            .foregroundColor(KitInk.terminalText)
            .frame(width: 44, height: 44)
            .background(Circle().fill(KitInk.terminal.opacity(0.65)))
    }

    private func caption(_ thumb: MediaThumb) -> some View {
        HStack(spacing: 8) {
            Text(name).kitType(.meta).foregroundColor(KitInk.terminalText).lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 4)
            if let badge = Self.badge(thumb, kind: item.kind) {
                Text(badge).kitType(.meta).monospacedDigit().foregroundColor(KitInk.terminalText)
                    .padding(.horizontal, 6).padding(.vertical, 2)
                    .background(Capsule().fill(KitInk.terminal.opacity(0.65)))
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 8)
        .background(LinearGradient(colors: [.clear, KitInk.terminal.opacity(0.7)], startPoint: .top, endPoint: .bottom))
        .allowsHitTesting(false)
    }

    private func open() { openURL(url) }

    /// "1:05" for a video, "1920×1080" for an image.
    static func badge(_ thumb: MediaThumb, kind: ChatMedia.Kind) -> String? {
        switch kind {
        case .video: return thumb.duration.map(duration)
        case .image: return thumb.pixelSize == .zero ? nil : "\(Int(thumb.pixelSize.width))×\(Int(thumb.pixelSize.height))"
        }
    }

    static func duration(_ seconds: Double) -> String {
        let total = Int(seconds.rounded())
        let h = total / 3600, m = (total % 3600) / 60, s = total % 60
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
    }
}

/// AVKit's inline controls; starts when it appears (the user just clicked play) and stops with the card.
private struct InlinePlayer: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> AVPlayerView {
        let view = AVPlayerView()
        view.controlsStyle = .inline
        view.showsFullScreenToggleButton = true
        let player = AVPlayer(url: url)
        view.player = player
        player.play()
        return view
    }

    func updateNSView(_ view: AVPlayerView, context: Context) {}

    static func dismantleNSView(_ view: AVPlayerView, coordinator: ()) {
        view.player?.pause()
        view.player = nil
    }
}
