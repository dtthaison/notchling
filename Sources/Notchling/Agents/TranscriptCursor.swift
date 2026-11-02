import Foundation

/// A parser that takes a transcript one JSONL line at a time and keeps its own state between lines.
protocol TranscriptLineParser {
    init()
    mutating func consume(_ line: Data)
}

/// Reads transcripts incrementally: each file keeps a cursor (where it was read up to, and the parser's
/// state there), so a call parses only the lines appended since the last one. The first call, and a file
/// that was replaced, truncated or grew by more than `tailBytes` at once, starts from the last
/// `tailBytes` of the file.
final class TranscriptCursors<Parser: TranscriptLineParser, Output> {
    private struct Cursor {
        let inode: UInt64
        /// Just past the last complete line parsed; a line still being written is read next time.
        var offset: UInt64
        var modified: Date
        var size: UInt64
        var parser: Parser
        var output: Output
    }

    private let tailBytes: UInt64
    private let limit: Int
    private let render: (Parser) -> Output
    private var cursors: [String: Cursor] = [:]
    /// Held while a cursor reads and parses, so two callers never advance the same one.
    private let lock = NSLock()

    /// `render` turns the parser's state into what callers get; it runs only when the file changed.
    init(tailBytes: UInt64, limit: Int = 300, render: @escaping (Parser) -> Output) {
        self.tailBytes = tailBytes
        self.limit = limit
        self.render = render
    }

    /// Nil when the file can't be read.
    func read(_ path: String) -> Output? {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: path),
              let modified = attrs[.modificationDate] as? Date,
              let size = (attrs[.size] as? NSNumber)?.uint64Value else { return nil }
        let inode = (attrs[.systemFileNumber] as? NSNumber)?.uint64Value ?? 0

        lock.lock(); defer { lock.unlock() }
        let reusable: Bool
        if let cached = cursors[path], cached.inode == inode, cached.offset <= size, size - cached.offset <= tailBytes {
            if cached.modified == modified, cached.size == size { return cached.output }
            reusable = true
        } else {
            reusable = false
        }
        // Taken out of the dictionary, so the parser's arrays have one owner and grow in place instead
        // of being copied on every read.
        if reusable, var cursor = cursors.removeValue(forKey: path) {
            if size > cursor.offset, let chunk = Self.lines(path, from: cursor.offset, to: size) {
                cursor.offset += chunk.consumed
                for line in chunk.lines { cursor.parser.consume(line) }
            }
            cursor.modified = modified
            cursor.size = size
            cursor.output = render(cursor.parser)
            cursors[path] = cursor
            return cursor.output
        }

        let start = size > tailBytes ? size - tailBytes : 0
        guard let chunk = Self.lines(path, from: start, to: size, skipFirstLine: start > 0) else { return nil }
        var parser = Parser()
        for line in chunk.lines { parser.consume(line) }
        let output = render(parser)
        if cursors.count > limit { cursors.removeAll() }
        cursors[path] = Cursor(inode: inode, offset: start + chunk.consumed, modified: modified, size: size,
                               parser: parser, output: output)
        return output
    }

    /// The complete lines in `start..<end` and how many bytes they span. `skipFirstLine`: `start` may be
    /// mid-line, so everything up to the first newline is dropped.
    static func lines(_ path: String, from start: UInt64, to end: UInt64,
                      skipFirstLine: Bool = false) -> (lines: [Data], consumed: UInt64)? {
        guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
        defer { try? handle.close() }
        try? handle.seek(toOffset: start)
        guard let data = try? handle.read(upToCount: Int(end - start)) else { return ([], 0) }
        let newline = UInt8(ascii: "\n")
        guard let last = data.lastIndex(of: newline) else { return ([], 0) }
        var from = data.startIndex
        if skipFirstLine, let first = data.firstIndex(of: newline) { from = first + 1 }
        let lines = from <= last ? data[from..<last].split(separator: newline, omittingEmptySubsequences: true) : []
        return (lines.map { Data($0) }, UInt64(last - data.startIndex + 1))
    }
}
