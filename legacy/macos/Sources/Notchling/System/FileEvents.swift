import CoreServices
import Foundation

/// Tells which files changed under some folders, from FSEvents, so a scanner can look at just those
/// instead of walking every file. When FSEvents can't say (events were dropped, a watched folder moved),
/// the handler is told to rescan everything.
///
/// Paths come back as the caller gave the folders, even when a folder is reached through a symlink
/// (FSEvents reports resolved paths, such as /private/var for /var).
final class FileEvents {
    struct Batch {
        var paths: Set<String> = []
        /// The caller should walk everything again: FSEvents lost track.
        var rescan = false
    }

    private var stream: FSEventStreamRef?
    private let folders: [(given: String, resolved: String)]
    private let handler: (Batch) -> Void

    /// `handler` runs on `queue`. `latency`: FSEvents groups changes over this many seconds.
    init?(folders: [URL], latency: TimeInterval = 2, queue: DispatchQueue, handler: @escaping (Batch) -> Void) {
        let existing = folders.filter { FileManager.default.fileExists(atPath: $0.path) }
        guard !existing.isEmpty else { return nil }
        self.folders = existing.map { ($0.standardizedFileURL.path, Self.realPath($0.path)) }
        self.handler = handler

        var context = FSEventStreamContext(version: 0, info: nil, retain: nil, release: nil, copyDescription: nil)
        context.info = Unmanaged.passUnretained(self).toOpaque()
        let flags = UInt32(kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagUseCFTypes
                           | kFSEventStreamCreateFlagWatchRoot)
        let callback: FSEventStreamCallback = { _, info, count, paths, eventFlags, _ in
            guard let info else { return }
            let me = Unmanaged<FileEvents>.fromOpaque(info).takeUnretainedValue()
            let list = Unmanaged<CFArray>.fromOpaque(paths).takeUnretainedValue() as? [String] ?? []
            me.deliver(list, Array(UnsafeBufferPointer(start: eventFlags, count: count)))
        }
        guard let stream = FSEventStreamCreate(nil, callback, &context, self.folders.map(\.resolved) as CFArray,
                                               FSEventStreamEventId(kFSEventStreamEventIdSinceNow), latency, flags)
        else { return nil }
        self.stream = stream
        FSEventStreamSetDispatchQueue(stream, queue)
        FSEventStreamStart(stream)
    }

    deinit {
        guard let stream else { return }
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
    }

    private static let rescanFlags = UInt32(kFSEventStreamEventFlagMustScanSubDirs | kFSEventStreamEventFlagUserDropped
                                            | kFSEventStreamEventFlagKernelDropped | kFSEventStreamEventFlagRootChanged)

    private func deliver(_ paths: [String], _ flags: [FSEventStreamEventFlags]) {
        var batch = Batch()
        for (path, flag) in zip(paths, flags) {
            if flag & Self.rescanFlags != 0 { batch.rescan = true }
            if let given = given(path) { batch.paths.insert(given) }
        }
        if batch.rescan || !batch.paths.isEmpty { handler(batch) }
    }

    /// The path FSEvents will report. `URL.resolvingSymlinksInPath()` won't do: it turns /private/var
    /// back into /var.
    static func realPath(_ path: String) -> String {
        guard let resolved = realpath(path, nil) else { return path }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    /// The event's path under the folder as the caller named it.
    func given(_ resolved: String) -> String? {
        for folder in folders where resolved == folder.resolved || resolved.hasPrefix(folder.resolved + "/") {
            return folder.given + resolved.dropFirst(folder.resolved.count)
        }
        return nil
    }
}
