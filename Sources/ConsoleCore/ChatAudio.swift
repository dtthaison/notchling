import Foundation

/// An audio file a chat message names. Like a file picture, only the description travels; the bytes are read in
/// pieces on demand (`chat.audio`) and the Mac path never leaves the Mac.
public struct AudioRef: Equatable, Sendable {
    /// `a-<first 16 hex of sha256(path)>`.
    public let id: String
    public let path: String
    public let mediaType: String
    public let byteCount: Int
    public var name: String { (path as NSString).lastPathComponent }
}

public enum ChatAudio {
    public static let audioExtensions: Set<String> = ["mp3", "wav", "m4a", "aac", "ogg", "opus", "flac", "caf"]

    /// Raw bytes per `chat.audio` call: base64 grows it by a third and a bridge message is capped at 256 KB.
    public static let maxChunk = 160 * 1024

    /// Audio files named per message; more would bury the text.
    public static let perMessage = 12

    public typealias Check = (String) -> Bool

    public static func isAudioName(_ token: String) -> Bool {
        audioExtensions.contains((token as NSString).pathExtension.lowercased())
    }

    public static func id(for path: String) -> String { "a-" + BridgeCrypto.sha256Hex(path).prefix(16) }

    public static func mediaType(forExtension ext: String) -> String {
        switch ext.lowercased() {
        case "mp3": return "audio/mpeg"
        case "wav": return "audio/wav"
        case "m4a": return "audio/mp4"
        case "aac": return "audio/aac"
        case "ogg", "opus": return "audio/ogg"
        case "flac": return "audio/flac"
        case "caf": return "audio/x-caf"
        default: return "application/octet-stream"
        }
    }

    /// Whether a path from a chat message may be played: an audio file that exists, under the home folder or /tmp.
    /// Same rule as `ChatImages.isAllowedFile`, with audio extensions.
    public static func isAllowedFile(_ path: String, home: URL = FileManager.default.homeDirectoryForCurrentUser) -> Bool {
        guard path.hasPrefix("/") else { return false }
        let url = URL(fileURLWithPath: path).standardizedFileURL.resolvingSymlinksInPath()
        guard audioExtensions.contains(url.pathExtension.lowercased()) else { return false }
        let resolved = url.path
        let homePath = home.standardizedFileURL.resolvingSymlinksInPath().path
        let roots = [homePath, "/tmp", "/private/tmp"].map { $0.hasSuffix("/") ? $0 : $0 + "/" }
        guard roots.contains(where: { resolved.hasPrefix($0) }) else { return false }
        guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey]), values.isRegularFile == true else { return false }
        return true
    }

    private static let isFolderOnDisk: Check = { path in
        var directory: ObjCBool = false
        return FileManager.default.fileExists(atPath: path, isDirectory: &directory) && directory.boolValue
    }

    /// Words of a message that could be paths: split at spaces, quotes, brackets and backticks.
    private static let tokenPattern = try! NSRegularExpression(pattern: "[^\\s()\\[\\]<>\"'`,;|*]+")

    private static func tokens(in text: String) -> [String] {
        let ns = text as NSString
        return tokenPattern.matches(in: text, range: NSRange(location: 0, length: ns.length)).map { trim(ns.substring(with: $0.range)) }
    }

    private static func trim(_ token: String) -> String {
        var trimmed = token
        while let last = trimmed.last, ".:!?".contains(last) { trimmed.removeLast() }
        return trimmed
    }

    private static func expand(_ path: String) -> String {
        ((path as NSString).expandingTildeInPath as NSString).standardizingPath
    }

    /// Absolute paths of the audio files [text] names, in order, without repeats. A name resolves as written when it
    /// is absolute or starts with `~`; otherwise against every folder the message names (absolute, `~` or relative to
    /// [cwd]), then against [cwd]. That is how "`Desktop/x/`" followed by "`line.mp3`" finds Desktop/x/line.mp3.
    public static func files(in text: String, cwd: String?, home: URL = FileManager.default.homeDirectoryForCurrentUser,
                             isFile: Check? = nil, isFolder: Check? = nil) -> [String] {
        let isFile = isFile ?? { isAllowedFile($0, home: home) }
        let isFolder = isFolder ?? isFolderOnDisk
        let words = tokens(in: text)
        let cwd = cwd.flatMap { $0.isEmpty ? nil : $0 }

        var folders: [String] = []
        for word in words where word.contains("/") && !word.contains("://") && !isAudioName(word) {
            let candidate: String
            if word.hasPrefix("/") || word.hasPrefix("~") { candidate = expand(word) }
            else if let cwd { candidate = ((cwd as NSString).appendingPathComponent(word) as NSString).standardizingPath }
            else { continue }
            if !folders.contains(candidate), isFolder(candidate) { folders.append(candidate) }
        }
        if let cwd, !folders.contains(cwd) { folders.append(cwd) }

        var found: [String] = []
        for word in words where isAudioName(word) && !word.contains("://") {
            var resolved: String?
            if word.hasPrefix("/") || word.hasPrefix("~") {
                let path = expand(word)
                if isFile(path) { resolved = path }
            } else {
                for folder in folders {
                    let path = ((folder as NSString).appendingPathComponent(word) as NSString).standardizingPath
                    if isFile(path) { resolved = path; break }
                }
            }
            if let resolved, !found.contains(resolved) { found.append(resolved) }
            if found.count >= perMessage { break }
        }
        return found
    }

    /// References for the files [text] names, with their sizes.
    public static func refs(in text: String, cwd: String?, home: URL = FileManager.default.homeDirectoryForCurrentUser) -> [AudioRef] {
        // Most messages name no audio: skip the folder checks for them.
        let lowered = text.lowercased()
        guard audioExtensions.contains(where: { lowered.contains("." + $0) }) else { return [] }
        return files(in: text, cwd: cwd, home: home).map { path in
            let size = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? NSNumber)?.intValue ?? 0
            return AudioRef(id: id(for: path), path: path, mediaType: mediaType(forExtension: (path as NSString).pathExtension),
                            byteCount: size)
        }
    }

    // MARK: - Reading the bytes

    public struct Chunk: Equatable, Sendable {
        public let data: Data
        public let offset: Int
        public let total: Int
        public let mediaType: String
    }

    /// Up to [length] bytes (at most `maxChunk`) of the file from [offset]. The file is checked again now: it may
    /// have been replaced by a link since the chat was read.
    public static func chunk(of ref: AudioRef, offset: Int, length: Int,
                             home: URL = FileManager.default.homeDirectoryForCurrentUser) throws -> Chunk {
        guard isAllowedFile(ref.path, home: home) else { throw AudioError.notAllowed }
        guard offset >= 0, length > 0 else { throw AudioError.badRange }
        let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: ref.path))
        defer { try? handle.close() }
        let total = Int(try handle.seekToEnd())
        guard offset <= total else { throw AudioError.badRange }
        try handle.seek(toOffset: UInt64(offset))
        let data = try handle.read(upToCount: min(length, maxChunk)) ?? Data()
        return Chunk(data: data, offset: offset, total: total, mediaType: ref.mediaType)
    }
}

public enum AudioError: Error, Equatable, LocalizedError {
    case notFound, notAllowed, badRange

    public var errorDescription: String? {
        switch self {
        case .notFound: return "Không thấy tệp âm thanh này trong cuộc trò chuyện."
        case .notAllowed: return "Tệp âm thanh này nằm ngoài các thư mục cho phép."
        case .badRange: return "Khoảng đọc không hợp lệ."
        }
    }
}
