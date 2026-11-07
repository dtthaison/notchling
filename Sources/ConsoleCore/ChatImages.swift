import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// A picture a chat message refers to. The chat only ever carries these descriptions; the bytes are read on demand
/// (a base64 block out of the transcript line, or a file the message names) and sent as a downscaled JPEG.
public struct ImageRef: Equatable, Sendable {
    public enum Origin: String, Sendable { case user, toolResult, file }

    /// Stable within one transcript: where the picture lives (`b-<line offset>-<block>` or `f-<hash of the path>`).
    public let id: String
    public let mediaType: String
    public let width: Int?
    public let height: Int?
    public let origin: Origin
    /// Size of the original, for the viewer ("1,2 MB").
    public let byteCount: Int
    /// For a transcript block: the line's offset in the file and the picture's index among the line's pictures.
    public let lineOffset: UInt64?
    public let blockIndex: Int?
    /// For a file named in the text.
    public let path: String?
}

public enum ChatImages {
    public static let allowedExtensions: Set<String> = ["png", "jpg", "jpeg", "gif", "webp", "heic"]

    /// A line with a picture in it is the usual reason a line is big; the base64 is cut out before the JSON is read, so
    /// the picture is never held in memory by the chat parser. Returns the line without the pictures' data and the
    /// pictures' sizes in the order they appear.
    static func stripBase64(from line: Substring) -> (text: Substring, blocks: [(base64Length: Int, size: (Int, Int)?)]) {
        guard line.contains("\"type\":\"base64\"") else { return (line, []) }
        let bytes = Array(line.utf8)
        let marker = Array("\"type\":\"base64\",\"media_type\":\"".utf8)
        let dataKey = Array("\",\"data\":\"".utf8)
        var out: [UInt8] = []
        out.reserveCapacity(min(bytes.count, 200_000))
        var blocks: [(Int, (Int, Int)?)] = []
        var index = 0
        while index < bytes.count {
            guard let found = find(marker, in: bytes, from: index) else { break }
            // media type up to the closing quote, then `","data":"`
            var cursor = found + marker.count
            while cursor < bytes.count, bytes[cursor] != 0x22 { cursor += 1 }
            guard cursor < bytes.count, hasPrefix(dataKey, in: bytes, at: cursor) else {
                out.append(contentsOf: bytes[index..<(found + marker.count)])
                index = found + marker.count
                continue
            }
            let dataStart = cursor + dataKey.count
            var dataEnd = dataStart
            while dataEnd < bytes.count, bytes[dataEnd] != 0x22 { dataEnd += 1 }
            out.append(contentsOf: bytes[index..<dataStart])
            let head = String(decoding: bytes[dataStart..<min(dataEnd, dataStart + 64)], as: UTF8.self)
            blocks.append((dataEnd - dataStart, pngSize(base64Head: head)))
            index = dataEnd
        }
        if index < bytes.count { out.append(contentsOf: bytes[index...]) }
        return (Substring(String(decoding: out, as: UTF8.self)), blocks.map { (base64Length: $0.0, size: $0.1) })
    }

    private static func find(_ needle: [UInt8], in haystack: [UInt8], from: Int) -> Int? {
        guard haystack.count >= needle.count, from <= haystack.count - needle.count else { return nil }
        var i = from
        let last = haystack.count - needle.count
        while i <= last {
            if haystack[i] == needle[0], hasPrefix(needle, in: haystack, at: i) { return i }
            i += 1
        }
        return nil
    }

    private static func hasPrefix(_ needle: [UInt8], in haystack: [UInt8], at index: Int) -> Bool {
        guard index + needle.count <= haystack.count else { return false }
        for offset in 0..<needle.count where haystack[index + offset] != needle[offset] { return false }
        return true
    }

    /// Width and height from the start of a PNG (the IHDR chunk is in the first 24 bytes), when that is what it is.
    static func pngSize(base64Head: String) -> (Int, Int)? {
        var head = base64Head
        head = String(head.prefix(32 / 3 * 4 + 4))
        while head.count % 4 != 0 { head.removeLast() }
        guard let data = Data(base64Encoded: head), data.count >= 24, data.prefix(8) == Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) else { return nil }
        func int(_ at: Int) -> Int { Int(data[at]) << 24 | Int(data[at + 1]) << 16 | Int(data[at + 2]) << 8 | Int(data[at + 3]) }
        return (int(16), int(20))
    }

    /// The picture blocks of a message's content, in the order they appear (also inside tool results), with whether
    /// each one is a tool's result.
    static func orderedImages(in content: Any?) -> [(block: [String: Any], inToolResult: Bool)] {
        var found: [(block: [String: Any], inToolResult: Bool)] = []
        func walk(_ value: Any?, nested: Bool) {
            guard let blocks = value as? [[String: Any]] else { return }
            for block in blocks {
                if block["type"] as? String == "image", (block["source"] as? [String: Any])?["type"] as? String == "base64" {
                    found.append((block, nested))
                } else if block["type"] as? String == "tool_result" {
                    walk(block["content"], nested: true)
                }
            }
        }
        walk(content, nested: false)
        return found
    }

    static func imageBlocks(in content: Any?) -> [[String: Any]] { orderedImages(in: content).map(\.block) }

    /// References for the pictures in [content] (a message's content or a queued command's prompt) of the line at
    /// [lineOffset]. [stripped] are the sizes the line's base64 had before it was cut out.
    static func refs(in content: Any?, lineOffset: UInt64, stripped: [(base64Length: Int, size: (Int, Int)?)]) -> [(ref: ImageRef, inToolResult: Bool)] {
        orderedImages(in: content).enumerated().map { index, item in
            let source = item.block["source"] as? [String: Any]
            let info = stripped.indices.contains(index) ? stripped[index] : nil
            return (ImageRef(id: "b-\(lineOffset)-\(index)", mediaType: source?["media_type"] as? String ?? "image/png",
                             width: info?.size?.0, height: info?.size?.1, origin: item.inToolResult ? .toolResult : .user,
                             byteCount: (info?.base64Length ?? 0) * 3 / 4, lineOffset: lineOffset, blockIndex: index, path: nil), item.inToolResult)
        }
    }

    // MARK: - Files named in the text

    private static let markerPattern = try! NSRegularExpression(pattern: #"\[Image: source: ([^\]\n]+)\]"#)

    /// Whether a path from a chat message may be shown: an image file that exists, under the home folder or /tmp.
    public static func isAllowedFile(_ path: String, home: URL = FileManager.default.homeDirectoryForCurrentUser) -> Bool {
        guard path.hasPrefix("/") else { return false }
        let url = URL(fileURLWithPath: path).standardizedFileURL.resolvingSymlinksInPath()
        guard allowedExtensions.contains(url.pathExtension.lowercased()) else { return false }
        let resolved = url.path
        let homePath = home.standardizedFileURL.resolvingSymlinksInPath().path
        let roots = [homePath, "/tmp", "/private/tmp"].map { $0.hasSuffix("/") ? $0 : $0 + "/" }
        guard roots.contains(where: { resolved.hasPrefix($0) }) else { return false }
        guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey]), values.isRegularFile == true else { return false }
        return true
    }

    /// "[Image: source: /path/x.png]" markers in [text]: the text without the ones that point at a shown file, and
    /// the references. Markers whose path is not allowed stay in the text as written.
    static func fileMarkers(in text: String, home: URL = FileManager.default.homeDirectoryForCurrentUser) -> (text: String, refs: [ImageRef]) {
        let range = NSRange(text.startIndex..., in: text)
        var refs: [ImageRef] = []
        var result = text
        for match in markerPattern.matches(in: text, range: range).reversed() {
            guard let whole = Range(match.range, in: text), let pathRange = Range(match.range(at: 1), in: text) else { continue }
            let path = text[pathRange].trimmingCharacters(in: .whitespaces)
            guard isAllowedFile(path, home: home) else { continue }
            let size = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? NSNumber)?.intValue ?? 0
            let type = UTType(filenameExtension: (path as NSString).pathExtension)?.preferredMIMEType ?? "image/png"
            // Without the real size the phone draws a 4:3 box and crops a tall screenshot to its middle.
            let pixels = pixelSize(ofFile: path)
            refs.append(ImageRef(id: "f-" + BridgeCrypto.sha256Hex(path).prefix(16), mediaType: type, width: pixels?.0, height: pixels?.1, origin: .file,
                                 byteCount: size, lineOffset: nil, blockIndex: nil, path: path))
            result.replaceSubrange(whole, with: "")
        }
        return (result.trimmingCharacters(in: .whitespacesAndNewlines), refs.reversed())
    }

    /// Width and height of an image file, read from its header only (the pixels are not decoded).
    static func pixelSize(ofFile path: String) -> (Int, Int)? {
        guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width > 0, height > 0 else { return nil }
        return (width, height)
    }

    // MARK: - Reading the bytes

    /// The original bytes of a picture. For a transcript block the line is read again and the block decoded.
    public static func data(for ref: ImageRef, transcript: URL, home: URL = FileManager.default.homeDirectoryForCurrentUser) throws -> Data {
        if let path = ref.path {
            // Checked again now: the file may have changed or been replaced by a link since the chat was read.
            guard isAllowedFile(path, home: home) else { throw ImageError.notAllowed }
            return try Data(contentsOf: URL(fileURLWithPath: path))
        }
        guard let offset = ref.lineOffset, let index = ref.blockIndex else { throw ImageError.notFound }
        let handle = try FileHandle(forReadingFrom: transcript)
        defer { try? handle.close() }
        try handle.seek(toOffset: offset)
        var line = Data()
        // A line is read in pieces until its newline; pictures are at most a few megabytes.
        while line.count < 64 * 1024 * 1024 {
            guard let piece = try handle.read(upToCount: 1 << 20), !piece.isEmpty else { break }
            if let newline = piece.firstIndex(of: 0x0A) {
                line.append(piece[piece.startIndex..<newline])
                break
            }
            line.append(piece)
        }
        guard let object = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any] else { throw ImageError.notFound }
        let content = (object["message"] as? [String: Any])?["content"] ?? (object["attachment"] as? [String: Any])?["prompt"]
        let blocks = imageBlocks(in: content)
        guard blocks.indices.contains(index), let base64 = (blocks[index]["source"] as? [String: Any])?["data"] as? String,
              let decoded = Data(base64Encoded: base64) else { throw ImageError.notFound }
        return decoded
    }

    /// A JPEG (quality 0.8) at most `maxWidth` pixels wide, from the bytes of any picture format ImageIO reads.
    public static func jpeg(from data: Data, maxWidth: Int) throws -> Data {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { throw ImageError.unreadable }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: max(16, min(maxWidth, 1600)),
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { throw ImageError.unreadable }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, UTType.jpeg.identifier as CFString, 1, nil) else { throw ImageError.unreadable }
        // JPEG has no transparency: flatten onto white first, or a transparent screenshot turns black.
        let flat = flatten(image)
        CGImageDestinationAddImage(destination, flat, [kCGImageDestinationLossyCompressionQuality: 0.8] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw ImageError.unreadable }
        return output as Data
    }

    private static func flatten(_ image: CGImage) -> CGImage {
        guard image.alphaInfo != .none, image.alphaInfo != .noneSkipFirst, image.alphaInfo != .noneSkipLast,
              let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: 0,
                                      space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return image }
        context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: image.width, height: image.height))
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return context.makeImage() ?? image
    }
}

public enum ImageError: Error, Equatable, LocalizedError {
    case notFound, notAllowed, unreadable

    public var errorDescription: String? {
        switch self {
        case .notFound: return "Không thấy ảnh này trong transcript."
        case .notAllowed: return "Ảnh này nằm ngoài các thư mục cho phép."
        case .unreadable: return "Không đọc được ảnh."
        }
    }
}
