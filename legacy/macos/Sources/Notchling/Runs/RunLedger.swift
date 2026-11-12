import Foundation

/// Reads a JSONL file that only grows: each call returns the complete lines added since the last one.
/// A partial last line waits for its newline. A file that shrank (or went away) starts over from the top.
/// The first read takes only the last `firstReadCap` bytes, since a busy repo's `runs.jsonl` grows for months.
struct JSONLTail {
    struct Result: Equatable {
        var lines: [String] = []
        /// The file was truncated or replaced; the caller should drop what it built from the old content.
        var reset = false
    }

    private(set) var offset: UInt64 = 0
    var firstReadCap: UInt64 = 1 << 20
    private var started = false

    mutating func read(_ url: URL) -> Result {
        guard let handle = try? FileHandle(forReadingFrom: url) else {
            let had = started && offset > 0
            offset = 0
            started = false
            return Result(lines: [], reset: had)
        }
        defer { try? handle.close() }
        guard let size = try? handle.seekToEnd() else { return Result() }

        var result = Result()
        var skipFirst = false
        if started && size < offset {
            result.reset = true
            started = false
        }
        if !started {
            started = true
            if size > firstReadCap {
                offset = size - firstReadCap
                skipFirst = true  // the cut lands inside a line
            } else {
                offset = 0
            }
        }
        guard size > offset else { return result }
        try? handle.seek(toOffset: offset)
        guard let data = try? handle.read(upToCount: Int(size - offset)), !data.isEmpty,
              let lastNewline = data.lastIndex(of: 0x0A) else { return result }
        offset += UInt64(lastNewline - data.startIndex + 1)
        var lines = String(decoding: data[data.startIndex..<lastNewline], as: UTF8.self)
            .split(separator: "\n", omittingEmptySubsequences: true).map(String.init)
        if skipFirst, !lines.isEmpty { lines.removeFirst() }
        result.lines = lines
        return result
    }
}

/// One run that has ended, with the start record it matched (nil for runs from before sd wrote starts).
struct RunEnd: Equatable {
    var start: RunStart?
    var finish: RunFinish
    /// Made up by Notchling: the run's process is gone and sd wrote no finish line.
    var lost: Bool { finish.rc == nil }
    /// The log plus the finish time, so a log path sd reuses still tells its runs apart. A lost run has its own
    /// kind of id, from its start, so a finish line that turns up later is a different end.
    var id: String {
        if lost, let start { return "lost:" + finish.log + "@" + String(Int(start.startedAt.timeIntervalSince1970)) }
        return finish.log + "@" + String(Int(finish.at.timeIntervalSince1970))
    }
    var key: String { start?.key ?? finish.key }
    /// sd writes `{"key":"precheck: …","log":"<log>.precheck"}` before each main run; it is no run of its own.
    var isPrecheck: Bool { finish.key.hasPrefix("precheck: ") }
}

/// What the `runs.jsonl` of one repository says: which runs are going and which have ended.
struct RunLedger {
    /// How long a start with no finish record can count as active; sd's longest limit is far below this.
    static let staleAfter: TimeInterval = 2 * 86400
    /// Ended runs are kept this long (the notch shows them for 10 minutes, a wake can wait a while).
    static let keepEnds: TimeInterval = 3600
    /// A run with a limit counts as active this long past it, then no more: sd would have stopped it.
    static let limitSlack: TimeInterval = 600

    private var starts: [String: RunStart] = [:]
    private var lastFinish: [String: Date] = [:]
    private var ends: [String: RunEnd] = [:]
    /// Runs (by log and start time) that have been seen alive in this process.
    private var seenAlive = Set<String>()

    mutating func apply(lines: [String]) {
        for line in lines { if let record = RunLine.parse(line) { apply(record) } }
    }

    mutating func apply(_ record: RunLine) {
        switch record {
        case .start(let s):
            if let old = starts[s.log], old.startedAt > s.startedAt { return }
            starts[s.log] = s
        case .finish(let f):
            var matched: RunStart?
            if let s = starts[f.log], s.startedAt <= f.at { matched = s }
            let end = RunEnd(start: matched, finish: f)
            ends[end.id] = end
            if let matched { ends["lost:" + matched.log + "@" + Self.stamp(matched)] = nil }
            lastFinish[f.log] = max(lastFinish[f.log] ?? .distantPast, f.at)
        }
    }

    private static func stamp(_ start: RunStart) -> String { String(Int(start.startedAt.timeIntervalSince1970)) }

    mutating func reset() {
        self = RunLedger()
    }

    /// Runs with a start, no later finish for their log, a live process (`pidAlive` gets the start record, to check
    /// the pid is still that run) and, with a limit, not long past it.
    func active(now: Date, pidAlive: (RunStart) -> Bool) -> [RunStart] {
        starts.values.filter { s in
            guard now.timeIntervalSince(s.startedAt) < Self.staleAfter else { return false }
            if let limit = s.limit, now.timeIntervalSince(s.startedAt) > limit + Self.limitSlack { return false }
            return processLives(s, pidAlive: pidAlive)
        }.sorted { $0.startedAt < $1.startedAt }
    }

    /// No finish line for the run's log yet, and its process is still that run.
    private func processLives(_ s: RunStart, pidAlive: (RunStart) -> Bool) -> Bool {
        guard (lastFinish[s.log] ?? .distantPast) < s.startedAt, s.pid != nil else { return false }
        return pidAlive(s)
    }

    /// Notes which runs are alive now, and returns those that were alive on an earlier call and are not any more
    /// (process gone or reused) with no finish line yet. A run past its limit whose process lives is not lost.
    /// Each is returned once; the caller reads `runs.jsonl` again, since sd may have written the finish just now,
    /// and then calls `markLost`.
    mutating func disappeared(now: Date, pidAlive: (RunStart) -> Bool) -> [RunStart] {
        let aliveNow = Set(starts.values.filter { processLives($0, pidAlive: pidAlive) }.map { $0.log + "@" + Self.stamp($0) })
        var lost: [RunStart] = []
        for id in seenAlive.subtracting(aliveNow) {
            if let s = starts.values.first(where: { $0.log + "@" + Self.stamp($0) == id }),
               (lastFinish[s.log] ?? .distantPast) < s.startedAt { lost.append(s) }
        }
        seenAlive = aliveNow
        return lost.sorted { $0.startedAt < $1.startedAt }
    }

    /// Ends `runs` that still have no finish line, with no result.
    mutating func markLost(_ runs: [RunStart], at: Date) {
        for s in runs where (lastFinish[s.log] ?? .distantPast) < s.startedAt {
            let end = RunEnd(start: s, finish: RunFinish(key: s.key, seconds: max(0, at.timeIntervalSince(s.startedAt)), rc: nil, log: s.log, at: at))
            ends[end.id] = end
            lastFinish[s.log] = max(lastFinish[s.log] ?? .distantPast, at)
        }
    }

    /// Runs that ended at or after `cutoff`, oldest first.
    func finished(since cutoff: Date) -> [RunEnd] {
        ends.values.filter { $0.finish.at >= cutoff }.sorted { $0.finish.at < $1.finish.at }
    }

    /// Forgets what nobody will ask for again.
    mutating func prune(now: Date) {
        ends = ends.filter { now.timeIntervalSince($0.value.finish.at) < Self.keepEnds }
        starts = starts.filter { _, s in
            if now.timeIntervalSince(s.startedAt) >= Self.staleAfter { return false }
            if let done = lastFinish[s.log], done >= s.startedAt { return now.timeIntervalSince(done) < Self.keepEnds }
            return true
        }
        lastFinish = lastFinish.filter { now.timeIntervalSince($0.value) < Self.keepEnds }
    }
}
