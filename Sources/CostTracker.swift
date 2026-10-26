import Foundation

/// What Claude Code's token use would have cost at API prices, from the `usage` of every assistant
/// message in ~/.claude/projects over the last 30 days.
///
/// A month of transcripts can be several GB, so each file is read once and then only from where the last
/// scan stopped. Running totals, read offsets and the ids already counted are kept in Application Support.
final class CostTracker: ObservableObject {
    struct Summary: Equatable {
        /// The last 14 days, oldest first.
        var days: [(day: Date, cost: Double)] = []
        var today = 0.0, week = 0.0, month = 0.0
        /// The last 30 days, most expensive first.
        var models: [(name: String, cost: Double)] = []
        var projects: [(name: String, cost: Double)] = []

        static func == (a: Summary, b: Summary) -> Bool {
            a.month == b.month && a.today == b.today && a.days.map(\.cost) == b.days.map(\.cost)
        }
    }

    @Published private(set) var summary = Summary()
    @Published private(set) var scanned: Date?
    /// Called on the main thread when the summary changes.
    var changed: (() -> Void)?

    private let queue = DispatchQueue(label: "local.notchling.cost", qos: .utility)
    private var scanning = false
    private var cache: Cache?
    private var seen = Set<UInt64>()

    private static let projectsDir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude/projects")
    private static let cacheURL = HookBridge.dir.appendingPathComponent("cost-cache.json")
    private static let keepDays = 35

    /// Reads whatever was added to the transcripts since the last scan.
    func refresh() {
        guard !scanning else { return }
        scanning = true
        queue.async {
            if self.cache == nil { self.load() }
            let dirty = self.scan()
            self.prune()
            if dirty { self.persist() }
            let summary = self.summarize()
            DispatchQueue.main.async {
                self.scanning = false
                self.scanned = Date()
                if summary != self.summary {
                    self.summary = summary
                    self.changed?()
                }
            }
        }
    }

    // MARK: Cache

    private struct Mark: Codable { var size: UInt64; var offset: UInt64 }

    private struct Cache: Codable {
        var version = 1
        var files: [String: Mark] = [:]
        /// Day ("2026-10-26", local time) → model id → dollars.
        var models: [String: [String: Double]] = [:]
        var projects: [String: [String: Double]] = [:]
        /// Day → hashes of the message ids counted on that day, as packed UInt64s.
        var seen: [String: Data] = [:]
    }

    private var seenToday: [String: [UInt64]] = [:]

    private func load() {
        if let data = try? Data(contentsOf: Self.cacheURL), let saved = try? JSONDecoder().decode(Cache.self, from: data),
           saved.version == 1 {
            cache = saved
            for (day, packed) in saved.seen {
                let hashes = packed.withUnsafeBytes { Array($0.bindMemory(to: UInt64.self)) }
                seenToday[day] = hashes
                seen.formUnion(hashes)
            }
        } else {
            cache = Cache()
        }
    }

    private func persist() {
        guard var cache else { return }
        cache.seen = seenToday.mapValues { hashes in hashes.withUnsafeBufferPointer { Data(buffer: $0) } }
        try? FileManager.default.createDirectory(at: HookBridge.dir, withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(cache) { try? data.write(to: Self.cacheURL, options: .atomic) }
    }

    private func prune() {
        let cutoff = Self.dayKey(Date().addingTimeInterval(-Double(Self.keepDays) * 86400))
        for day in seenToday.keys where day < cutoff {
            seen.subtract(seenToday[day] ?? [])
            seenToday[day] = nil
            cache?.models[day] = nil
            cache?.projects[day] = nil
        }
    }

    // MARK: Scanning

    /// True when anything new was counted or a read offset moved.
    private func scan() -> Bool {
        guard cache != nil else { return false }
        let since = Date().addingTimeInterval(-31 * 86400)
        let keys: [URLResourceKey] = [.contentModificationDateKey, .fileSizeKey, .isRegularFileKey]
        guard let walk = FileManager.default.enumerator(at: Self.projectsDir, includingPropertiesForKeys: keys) else { return false }
        var dirty = false
        var present = Set<String>()
        for case let url as URL in walk where url.pathExtension == "jsonl" {
            guard let values = try? url.resourceValues(forKeys: Set(keys)), values.isRegularFile == true,
                  let modified = values.contentModificationDate, modified > since,
                  let size = values.fileSize.map(UInt64.init) else { continue }
            present.insert(url.path)
            var mark = cache?.files[url.path] ?? Mark(size: 0, offset: 0)
            if mark.size == size { continue }
            // A file that shrank was rewritten; read it again (ids already counted are skipped).
            if size < mark.offset { mark.offset = 0 }
            mark.offset = read(url, from: mark.offset)
            mark.size = size
            cache?.files[url.path] = mark
            dirty = true
        }
        let gone = cache?.files.keys.filter { !present.contains($0) } ?? []
        if !gone.isEmpty {
            gone.forEach { cache?.files[$0] = nil }
            dirty = true
        }
        return dirty
    }

    /// Counts every complete line from `offset` on and returns where the last complete line ends.
    private func read(_ url: URL, from offset: UInt64) -> UInt64 {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return offset }
        defer { try? handle.close() }
        try? handle.seek(toOffset: offset)
        let project = Self.projectName(url)
        // File offset of the first byte in `carry`: the start of a line not finished yet.
        var lineStart = offset
        var carry = Data()
        while let chunk = try? handle.read(upToCount: 8 << 20), !chunk.isEmpty {
            var buffer = carry
            buffer.append(chunk)
            let consumed = buffer.withUnsafeBytes { raw -> Int in
                guard let base = raw.baseAddress else { return 0 }
                var start = 0
                while start < raw.count, let nl = memchr(base + start, 0x0A, raw.count - start) {
                    let end = base.distance(to: UnsafeRawPointer(nl))
                    if end > start { line(UnsafeRawBufferPointer(rebasing: raw[start..<end]), project: project) }
                    start = end + 1
                }
                return start
            }
            lineStart += UInt64(consumed)
            carry = buffer.subdata(in: consumed..<buffer.count)
        }
        return lineStart
    }

    private static let marker = Array(#""usage":{"#.utf8)

    private func line(_ bytes: UnsafeRawBufferPointer, project: String) {
        guard let base = bytes.baseAddress,
              Self.marker.withUnsafeBytes({ memmem(base, bytes.count, $0.baseAddress, $0.count) }) != nil,
              let object = try? JSONSerialization.jsonObject(with: Data(bytes)) as? [String: Any],
              object["type"] as? String == "assistant",
              let message = object["message"] as? [String: Any],
              let model = message["model"] as? String, model != "<synthetic>",
              let usage = message["usage"] as? [String: Any],
              let id = message["id"] as? String,
              let stamp = object["timestamp"] as? String, let date = Self.parseDate(stamp) else { return }
        let day = Self.dayKey(date)
        let key = Self.hash("\(id)|\(object["requestId"] as? String ?? "")")
        guard seen.insert(key).inserted else { return }
        seenToday[day, default: []].append(key)
        let cost = Self.cost(model: model, usage: usage)
        guard cost > 0 else { return }
        let name = (object["cwd"] as? String).map { ($0 as NSString).lastPathComponent } ?? project
        cache?.models[day, default: [:]][Self.modelKey(model), default: 0] += cost
        cache?.projects[day, default: [:]][name, default: 0] += cost
    }

    // MARK: Prices

    /// Dollars per million tokens: input, output, cache read. Cache writes are 1.25× input for five
    /// minutes and 2× for an hour. Checked in order, so longer ids come first.
    private static let prices: [(prefix: String, input: Double, output: Double, read: Double)] = [
        ("fable-5-1", 10, 50, 0.25), ("mythos-5-1", 10, 50, 0.25), ("fable", 10, 50, 1), ("mythos", 10, 50, 1),
        ("opus-5-5", 4, 20, 0.20), ("opus-5", 5, 25, 0.50),
        ("opus-4-8", 5, 25, 0.50), ("opus-4-7", 5, 25, 0.50), ("opus-4-6", 5, 25, 0.50), ("opus-4-5", 5, 25, 0.50),
        ("opus-4", 15, 75, 1.50),
        ("sonnet-5", 2, 10, 0.20), ("sonnet", 3, 15, 0.30),
        ("haiku-5-5", 0.10, 0.50, 0.01), ("haiku-4-5", 1, 5, 0.10), ("haiku", 0.80, 4, 0.08),
    ]

    static func cost(model: String, usage: [String: Any]) -> Double {
        let id = modelKey(model).replacingOccurrences(of: "claude-", with: "")
        guard var price = prices.first(where: { id.hasPrefix($0.prefix) }) else { return 0 }
        func n(_ key: String, _ from: [String: Any] = usage) -> Double { (from[key] as? Double) ?? 0 }
        let input = n("input_tokens"), output = n("output_tokens"), read = n("cache_read_input_tokens")
        var write5m = n("cache_creation_input_tokens"), write1h = 0.0
        if let split = usage["cache_creation"] as? [String: Any] {
            write1h = n("ephemeral_1h_input_tokens", split)
            write5m = n("ephemeral_5m_input_tokens", split)
        }
        // Haiku 5.5 costs five times as much once the prompt is over 100K tokens.
        if price.prefix == "haiku-5-5", input + read + write5m + write1h > 100_000 {
            price = (price.prefix, 0.50, 2.50, 0.05)
        }
        var dollars = input * price.input + output * price.output + read * price.read
            + write5m * price.input * 1.25 + write1h * price.input * 2
        if usage["speed"] as? String == "fast" { dollars *= 2 }
        return dollars / 1_000_000
    }

    /// "claude-opus-5-5[1m]" and "claude-haiku-4-5-20251001" → "claude-opus-5-5", "claude-haiku-4-5".
    private static func modelKey(_ model: String) -> String {
        var id = model
        if let bracket = id.firstIndex(of: "[") { id = String(id[..<bracket]) }
        return id.replacingOccurrences(of: #"-\d{8}$"#, with: "", options: .regularExpression)
    }

    /// "claude-opus-5-5" → "Opus 5.5".
    static func displayName(_ key: String) -> String {
        let parts = key.replacingOccurrences(of: "claude-", with: "").split(separator: "-")
        guard let family = parts.first else { return key }
        let version = parts.dropFirst().joined(separator: ".")
        return family.prefix(1).uppercased() + family.dropFirst() + (version.isEmpty ? "" : " \(version)")
    }

    // MARK: Summary

    private func summarize() -> Summary {
        guard let cache else { return Summary() }
        let calendar = Calendar.current
        let today = calendar.startOfDay(for: Date())
        func total(_ day: Date) -> Double { cache.models[Self.dayKey(day)]?.values.reduce(0, +) ?? 0 }
        func back(_ n: Int) -> Date { calendar.date(byAdding: .day, value: -n, to: today) ?? today.addingTimeInterval(-Double(n) * 86400) }

        var s = Summary()
        s.days = (0..<14).reversed().map { (back($0), total(back($0))) }
        s.today = total(today)
        s.week = (0..<7).map { total(back($0)) }.reduce(0, +)
        let month = Set((0..<30).map { Self.dayKey(back($0)) })
        s.month = month.map { cache.models[$0]?.values.reduce(0, +) ?? 0 }.reduce(0, +)

        func ranked(_ table: [String: [String: Double]], name: (String) -> String) -> [(String, Double)] {
            var sums: [String: Double] = [:]
            for day in month { for (k, v) in table[day] ?? [:] { sums[name(k), default: 0] += v } }
            return sums.filter { $0.value >= 0.01 }.sorted { $0.value > $1.value }
        }
        s.models = ranked(cache.models, name: Self.displayName)
        s.projects = ranked(cache.projects, name: { $0 })
        return s
    }

    // MARK: Helpers

    /// The project folder name is the cwd with "/" turned into "-"; its last part is close enough.
    private static func projectName(_ url: URL) -> String {
        var dir = url.deletingLastPathComponent()
        while dir.deletingLastPathComponent().path != projectsDir.path, dir.path.count > projectsDir.path.count {
            dir = dir.deletingLastPathComponent()
        }
        return dir.lastPathComponent.split(separator: "-").last.map(String.init) ?? dir.lastPathComponent
    }

    private static let dayFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        return f
    }()

    private static func dayKey(_ date: Date) -> String { dayFormatter.string(from: date) }

    private static let isoFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static func parseDate(_ s: String) -> Date? {
        isoFormatter.date(from: s) ?? ISO8601DateFormatter().date(from: s)
    }

    /// FNV-1a, stable across launches (Swift's Hasher isn't).
    private static func hash(_ s: String) -> UInt64 {
        var h: UInt64 = 0xcbf29ce484222325
        for byte in s.utf8 { h = (h ^ UInt64(byte)) &* 0x100000001b3 }
        return h
    }
}
