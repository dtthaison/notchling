import Foundation

/// The text of recent conversations, for ⌘⇧P: Claude Code transcripts (~/.claude and every lane's
/// projects folder), Codex rollouts and Antigravity CLI titles. Everything runs off the main thread.
/// A refresh lists the files, reads only those whose modification time or size changed, and drops
/// the ones that aged out; the result is cached in Application Support/Notchling/search-index.json so
/// the next launch starts from it. Searching takes a snapshot under a lock and never waits for a refresh.
final class SessionIndex {
    static let shared = SessionIndex()

    /// Called on the main thread when the index gained or lost sessions.
    var onChange: (() -> Void)?

    private let work = DispatchQueue(label: "notchling.session-index", qos: .utility)
    private let lock = NSLock()
    private var sessions: [String: IndexedSession] = [:]
    private var refreshing = false
    private var refreshedAt = Date.distantPast
    private var loaded = false

    /// A transcript larger than this is read from its start (title, folder) and its end (the newest text).
    private static let wholeFileBytes: Int64 = 4_000_000
    private static let headBytes = 200_000
    private static let tailBytes = 3_500_000

    private static let cacheVersion = 1
    private static var cacheURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Notchling/search-index.json")
    }

    var isRefreshing: Bool { lock.lock(); defer { lock.unlock() }; return refreshing }
    var count: Int { lock.lock(); defer { lock.unlock() }; return sessions.count }

    /// Starts a refresh unless one is running or the last one was under `minInterval` seconds ago.
    func refresh(minInterval: TimeInterval = 8) {
        lock.lock()
        if refreshing || Date().timeIntervalSince(refreshedAt) < minInterval { lock.unlock(); return }
        refreshing = true
        lock.unlock()
        work.async { [self] in
            if !loaded { loadCache() }
            run()
            lock.lock()
            refreshing = false
            refreshedAt = Date()
            lock.unlock()
            notify()
        }
    }

    /// The best sessions for the words, newest changes first among equals. `completion` runs on the main thread.
    func search(_ words: [String], limit: Int = 30, completion: @escaping ([SessionHit]) -> Void) {
        let snapshot = all()
        DispatchQueue.global(qos: .userInitiated).async {
            let hits = words.isEmpty ? Self.latest(snapshot, limit: limit) : SessionSearch.rank(words, in: snapshot, limit: limit)
            DispatchQueue.main.async { completion(hits) }
        }
    }

    /// The most recently changed sessions, with no snippet (for an empty query).
    static func latest(_ sessions: [IndexedSession], limit: Int) -> [SessionHit] {
        sessions.sorted { $0.meta.modified > $1.meta.modified }.prefix(limit)
            .map { SessionHit(meta: $0.meta, snippet: nil, score: 0) }
    }

    private func all() -> [IndexedSession] {
        lock.lock(); defer { lock.unlock() }
        return Array(sessions.values)
    }

    private func notify() {
        DispatchQueue.main.async { [weak self] in self?.onChange?() }
    }

    // MARK: Refresh

    private struct Candidate {
        var meta: SessionMeta
        var tool: SessionTool
    }

    private func run() {
        let now = Date()
        var candidates = claudeCandidates(now: now) + codexCandidates(now: now)
        candidates.sort { $0.meta.modified > $1.meta.modified }
        var changed = false
        var batch = 0
        var live = Set<String>()
        for candidate in candidates {
            live.insert(candidate.meta.key)
            lock.lock()
            let existing = sessions[candidate.meta.key]?.meta
            lock.unlock()
            if let existing, existing.modified == candidate.meta.modified, existing.size == candidate.meta.size { continue }
            guard let built = autoreleasepool(invoking: { Self.read(candidate.meta, tool: candidate.tool) }) else { continue }
            lock.lock(); sessions[built.meta.key] = built; lock.unlock()
            changed = true
            batch += 1
            // Search works while the first scan is still going.
            if batch % 25 == 0 { notify() }
        }
        for row in AntigravitySessions.rows(now: now) {
            let meta = SessionMeta(key: "agy:" + row.id, toolKey: SessionTool.antigravity.key, sessionID: row.id,
                                   title: row.title, folder: row.folder, path: "", configDir: nil,
                                   modified: row.modified.timeIntervalSince1970)
            live.insert(meta.key)
            lock.lock(); let existing = sessions[meta.key]?.meta; lock.unlock()
            if existing == meta { continue }
            lock.lock(); sessions[meta.key] = IndexedSession(meta: meta, text: row.title); lock.unlock()
            changed = true
        }
        lock.lock()
        let gone = sessions.keys.filter { !live.contains($0) }
        for key in gone { sessions[key] = nil }
        lock.unlock()
        if changed || !gone.isEmpty { saveCache() }
    }

    private func claudeCandidates(now: Date) -> [Candidate] {
        let fm = FileManager.default
        var seenFiles = Set<String>()
        var out: [Candidate] = []
        for (dir, lane) in LaneStore.configDirs() {
            let projects = dir.appendingPathComponent("projects")
            guard let folders = try? fm.contentsOfDirectory(atPath: projects.path) else { continue }
            for folder in folders {
                let base = projects.appendingPathComponent(folder)
                guard let names = try? fm.contentsOfDirectory(atPath: base.path) else { continue }
                for name in names where name.hasSuffix(".jsonl") {
                    let url = base.appendingPathComponent(name)
                    // A lane's projects folder is usually a symlink to ~/.claude/projects.
                    guard seenFiles.insert(url.resolvingSymlinksInPath().path).inserted,
                          let attrs = try? fm.attributesOfItem(atPath: url.path),
                          let modified = attrs[.modificationDate] as? Date,
                          now.timeIntervalSince(modified) < SessionSearch.recent else { continue }
                    let id = String(name.dropLast(".jsonl".count))
                    let key = "claude:" + id
                    // The same session can sit in two lane folders (moved between accounts); the newer wins.
                    if let at = out.firstIndex(where: { $0.meta.key == key }) {
                        if out[at].meta.modified >= modified.timeIntervalSince1970 { continue }
                        out.remove(at: at)
                    }
                    out.append(Candidate(meta: SessionMeta(
                        key: key, toolKey: SessionTool.claude.key, sessionID: id, title: "", folder: "", path: url.path,
                        configDir: lane?.path, modified: modified.timeIntervalSince1970,
                        size: (attrs[.size] as? NSNumber)?.int64Value ?? 0), tool: .claude))
                }
            }
        }
        return out
    }

    private func codexCandidates(now: Date) -> [Candidate] {
        let fm = FileManager.default
        var out: [Candidate] = []
        for day in 0..<Int(CodexSessions.recent / 86400) + 1 {
            let date = now.addingTimeInterval(-Double(day) * 86400)
            let parts = Calendar.current.dateComponents([.year, .month, .day], from: date)
            guard let y = parts.year, let m = parts.month, let d = parts.day else { continue }
            let dir = CodexSessions.root.appendingPathComponent(String(format: "%04d/%02d/%02d", y, m, d))
            guard let names = try? fm.contentsOfDirectory(atPath: dir.path) else { continue }
            for name in names where name.hasPrefix("rollout-") && name.hasSuffix(".jsonl") {
                let path = dir.appendingPathComponent(name).path
                guard let attrs = try? fm.attributesOfItem(atPath: path),
                      let modified = attrs[.modificationDate] as? Date,
                      now.timeIntervalSince(modified) < SessionSearch.recent else { continue }
                let id = Self.codexID(fileName: name)
                out.append(Candidate(meta: SessionMeta(
                    key: "codex:" + id, toolKey: SessionTool.codex.key, sessionID: id, title: "", folder: "", path: path,
                    configDir: nil, modified: modified.timeIntervalSince1970,
                    size: (attrs[.size] as? NSNumber)?.int64Value ?? 0), tool: .codex))
            }
        }
        return out
    }

    /// The session id at the end of `rollout-<time>-<id>.jsonl` (a 36-character UUID).
    static func codexID(fileName: String) -> String {
        String(fileName.dropLast(".jsonl".count).suffix(36))
    }

    // MARK: Reading a file

    /// Indexes one file.
    private static func read(_ candidate: SessionMeta, tool: SessionTool) -> IndexedSession? {
        guard let data = contents(of: candidate.path, size: candidate.size) else { return nil }
        var builder = SessionTextBuilder()
        var meta = candidate
        switch tool {
        case .claude:
            forEachLine(in: data) { if let line = TranscriptText.claude($0) { builder.add(line) } }
        case .codex:
            guard let info = CodexSessions.meta(candidate.path) else { return nil }
            meta.folder = info.cwd
            forEachLine(in: data) { if let line = TranscriptText.codex($0) { builder.add(line) } }
        case .antigravity:
            return nil
        }
        let text = builder.text
        guard !text.isEmpty else { return nil }
        if tool == .claude { meta.folder = builder.cwd ?? "" }
        meta.title = tool == .codex ? (CodexSessions.meta(candidate.path)?.title ?? builder.title) : builder.title
        return IndexedSession(meta: meta, text: text)
    }

    /// The whole file when it's small, else its start and its end joined at line boundaries.
    private static func contents(of path: String, size: Int64) -> Data? {
        guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
        defer { try? handle.close() }
        if size <= wholeFileBytes { return try? handle.readToEnd() }
        let head = handle.readData(ofLength: headBytes)
        try? handle.seek(toOffset: UInt64(max(0, size - Int64(tailBytes))))
        guard var tail = try? handle.readToEnd() else { return head }
        // The head ends and the tail starts in the middle of a line: drop the partial ones.
        var out = head
        if let cut = out.lastIndex(of: 0x0A) { out = out.prefix(through: cut) }
        if let cut = tail.firstIndex(of: 0x0A) { tail = tail.suffix(from: tail.index(after: cut)) }
        out.append(tail)
        return out
    }

    private static func forEachLine(in data: Data, _ body: (Data) -> Void) {
        for line in data.split(separator: 0x0A, omittingEmptySubsequences: true) where line.count <= TranscriptText.maxLineBytes {
            body(line)
        }
    }

    // MARK: Cache

    private struct CacheFile: Codable {
        var version: Int
        var sessions: [Record]
    }

    private struct Record: Codable {
        var meta: SessionMeta
        var text: String
    }

    private func loadCache() {
        loaded = true
        guard let data = try? Data(contentsOf: Self.cacheURL),
              let file = try? JSONDecoder().decode(CacheFile.self, from: data), file.version == Self.cacheVersion else { return }
        lock.lock()
        for record in file.sessions { sessions[record.meta.key] = IndexedSession(meta: record.meta, text: record.text) }
        lock.unlock()
        notify()
    }

    private func saveCache() {
        lock.lock()
        let records = sessions.values.map { Record(meta: $0.meta, text: $0.text) }
        lock.unlock()
        guard let data = try? JSONEncoder().encode(CacheFile(version: Self.cacheVersion, sessions: records)) else { return }
        let url = Self.cacheURL
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }
}
