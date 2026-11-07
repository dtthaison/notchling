import Foundation

/// A photo from the phone, kept in a temp file the session's `claude` can read by path (the way a dragged-in image
/// reaches it in the terminal). Files are private to the user and pruned after a day.
public struct PhoneImageStore: Sendable {
    public let directory: URL
    public let keepFor: TimeInterval

    public init(directory: URL, keepFor: TimeInterval = 24 * 3600) {
        self.directory = directory
        self.keepFor = keepFor
    }

    public static let live = PhoneImageStore(
        directory: FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Caches/AgentConsole/phone-images"))

    /// The file extension for what the bytes are, by magic number; nil for anything that is not a picture claude reads.
    public static func detect(_ data: Data) -> String? {
        let b = [UInt8](data.prefix(12))
        if b.count >= 3, b[0] == 0xFF, b[1] == 0xD8, b[2] == 0xFF { return "jpg" }
        if b.count >= 8, Array(b[0..<8]) == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] { return "png" }
        if b.count >= 6, Array(b[0..<4]) == [0x47, 0x49, 0x46, 0x38] { return "gif" }
        if b.count >= 12, Array(b[0..<4]) == [0x52, 0x49, 0x46, 0x46], Array(b[8..<12]) == [0x57, 0x45, 0x42, 0x50] { return "webp" }
        return nil
    }

    @discardableResult
    public func save(_ data: Data, ext: String, now: Date = Date()) throws -> URL {
        let fm = FileManager.default
        try fm.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        for old in (try? fm.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.contentModificationDateKey])) ?? [] {
            if let date = (try? old.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate, now.timeIntervalSince(date) > keepFor {
                try? fm.removeItem(at: old)
            }
        }
        let url = directory.appendingPathComponent("phone-\(UUID().uuidString.prefix(8).lowercased()).\(ext)")
        try data.write(to: url, options: .atomic)
        try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        return url
    }

    /// The message that reaches claude: the caption, then the file's path (quoted when it has spaces), as dragging a file
    /// into the terminal types it.
    public static func message(caption: String, path: String) -> String {
        let quoted = path.contains(where: { $0 == " " || $0 == "'" || $0 == "\"" }) ? "\"\(path)\"" : path
        let trimmed = caption.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? quoted : trimmed + " " + quoted
    }
}

/// One photo being uploaded in pieces: the bridge's messages are capped at 256 KB, a photo is not.
struct PhoneUpload {
    static let maxBytes = 6 * 1024 * 1024
    static let maxChunkBytes = 160 * 1024
    static let expiresAfter: TimeInterval = 120

    let connectionId: UUID
    let sessionId: String
    let expectedBytes: Int
    let caption: String
    let started: Date
    var data = Data()
    var nextSequence = 0
}

/// Uploads in progress, one per connection. A new `begin` replaces the old one; stale ones are dropped.
final class PhoneUploads: @unchecked Sendable {
    private let lock = NSLock()
    private var uploads: [String: PhoneUpload] = [:]

    func begin(connection: UUID, sessionId: String, bytes: Int, caption: String, now: Date) -> String {
        lock.lock(); defer { lock.unlock() }
        uploads = uploads.filter { $0.value.connectionId != connection && now.timeIntervalSince($0.value.started) < PhoneUpload.expiresAfter }
        let id = UUID().uuidString
        uploads[id] = PhoneUpload(connectionId: connection, sessionId: sessionId, expectedBytes: bytes, caption: caption, started: now)
        return id
    }

    /// Appends the next piece. Throws a message for the phone when the piece is out of order, too big, or past the size.
    func append(_ id: String, connection: UUID, sequence: Int, chunk: Data) throws {
        lock.lock(); defer { lock.unlock() }
        guard var upload = uploads[id], upload.connectionId == connection else { throw PhoneUploadError.unknown }
        guard sequence == upload.nextSequence else { throw PhoneUploadError.outOfOrder }
        guard chunk.count <= PhoneUpload.maxChunkBytes, upload.data.count + chunk.count <= upload.expectedBytes else { throw PhoneUploadError.tooBig }
        upload.data.append(chunk)
        upload.nextSequence += 1
        uploads[id] = upload
    }

    func finish(_ id: String, connection: UUID) throws -> PhoneUpload {
        lock.lock(); defer { lock.unlock() }
        guard let upload = uploads[id], upload.connectionId == connection else { throw PhoneUploadError.unknown }
        uploads[id] = nil
        guard upload.data.count == upload.expectedBytes else { throw PhoneUploadError.incomplete }
        return upload
    }

    func drop(connection: UUID) {
        lock.lock(); defer { lock.unlock() }
        uploads = uploads.filter { $0.value.connectionId != connection }
    }
}

enum PhoneUploadError: Error {
    case unknown, outOfOrder, tooBig, incomplete
}
