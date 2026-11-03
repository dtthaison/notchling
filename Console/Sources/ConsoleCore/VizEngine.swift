import Foundation

/// The four web-rendered specs plus what the native tabs and the suggester need, for one session.
public struct VizBundle: Equatable, Sendable {
    public var flow: VizSpec
    public var diagram: VizSpec
    public var chart: VizSpec
    public var signals: VizSignals
    public var webURL: URL?
    public var deviceHints: [String]
}

/// Builds VizSpecs from the session's transcripts and folder. Pure data in, specs out; it caches parsed
/// transcripts by modification time so a recompute after a small change reads only what moved.
/// Call it off the main thread.
public final class VizEngine: @unchecked Sendable {
    private struct CachedFacts {
        let modified: Date
        let size: UInt64
        let facts: TranscriptFacts
    }

    private let now: () -> Date
    private let git: (String) -> GitSnapshot
    private let home: URL
    private let entityTTL: TimeInterval
    private let lock = NSLock()
    private var factsCache: [URL: CachedFacts] = [:]
    private var entityCache: [String: (at: Date, result: SchemaScanResult)] = [:]
    private var gitCache: [String: (at: Date, snapshot: GitSnapshot)] = [:]

    public static let maxSubagentFiles = 40
    static let subagentTailBytes: UInt64 = 1_000_000
    static let maxCallsShown = 300
    static let maxTurnsShown = 40
    static let maxChartPoints = 300

    public init(now: @escaping () -> Date = Date.init,
                git: @escaping (String) -> GitSnapshot = { GitReader.read(cwd: $0) },
                home: URL = FileManager.default.homeDirectoryForCurrentUser,
                entityTTL: TimeInterval = 30) {
        self.now = now
        self.git = git
        self.home = home
        self.entityTTL = entityTTL
    }

    /// Changes whenever any transcript of the session grows; the store recomputes only then.
    public func fingerprint(of session: Session, cwd: String? = nil) -> String {
        var parts = [session.id, cwd ?? session.cwd]
        for url in [session.transcriptURL].compactMap({ $0 }) + session.subagents.map(\.transcriptURL) {
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0
            parts.append("\(url.lastPathComponent):\(modificationDate(of: url)?.timeIntervalSince1970 ?? 0):\(size)")
        }
        return parts.joined(separator: "|")
    }

    public func build(session: Session, cwdOverride: String? = nil) -> VizBundle {
        let cwd = cwdOverride ?? session.cwd
        let main = session.transcriptURL.map { facts(at: $0, isMain: true, origin: "main", tail: TranscriptAnalyzer.tailBytes) } ?? TranscriptFacts()
        var combined = main
        for agent in session.subagents.prefix(Self.maxSubagentFiles) {
            combined.merge(facts(at: agent.transcriptURL, isMain: false,
                                 origin: agent.description.isEmpty ? agent.type : agent.description, tail: Self.subagentTailBytes))
        }

        let signals = VizSignals(
            hasRecentMermaid: combined.mermaid.contains { block in
                block.time.map { now().timeIntervalSince($0) < 30 * 60 } ?? false
            },
            hasLocalhostURL: !main.localURLs.isEmpty)
        let urls = main.localURLs + combined.localURLs
        return VizBundle(
            flow: flowSpec(main: main, combined: combined, cwd: cwd),
            diagram: diagramSpec(combined),
            chart: chartSpec(combined, subagents: session.subagents),
            signals: signals,
            webURL: WebURLResolver.resolve(transcriptURLs: urls, cwd: cwd, home: home),
            deviceHints: combined.deviceHints)
    }

    // MARK: - Cached reads

    private func facts(at url: URL, isMain: Bool, origin: String, tail: UInt64) -> TranscriptFacts {
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        let modified = attributes?[.modificationDate] as? Date ?? .distantPast
        let size = (attributes?[.size] as? NSNumber)?.uint64Value ?? 0
        lock.lock()
        if let cached = factsCache[url], cached.modified == modified, cached.size == size {
            lock.unlock()
            return cached.facts
        }
        lock.unlock()
        let parsed = TranscriptAnalyzer.analyze(url: url, isMain: isMain, origin: origin, tailBytes: tail)
        lock.lock()
        factsCache[url] = CachedFacts(modified: modified, size: size, facts: parsed)
        lock.unlock()
        return parsed
    }

    private func gitSnapshot(cwd: String) -> GitSnapshot {
        lock.lock()
        if let cached = gitCache[cwd], now().timeIntervalSince(cached.at) < 10 { lock.unlock(); return cached.snapshot }
        lock.unlock()
        let snapshot = git(cwd)
        lock.lock(); gitCache[cwd] = (now(), snapshot); lock.unlock()
        return snapshot
    }

    // MARK: - Specs

    func flowSpec(main: TranscriptFacts, combined: TranscriptFacts, cwd: String) -> VizSpec {
        var turns = Array(main.turns.suffix(Self.maxTurnsShown))
        var budget = Self.maxCallsShown
        for index in turns.indices.reversed() {
            if turns[index].calls.count > budget { turns[index].calls = Array(turns[index].calls.suffix(max(0, budget))) }
            budget -= turns[index].calls.count
            if budget <= 0 {
                turns = Array(turns[index...])
                break
            }
        }
        turns.removeAll { $0.calls.isEmpty && $0.prompt.isEmpty }
        let totalCalls = main.turns.reduce(0) { $0 + $1.calls.count }
        let git = gitSnapshot(cwd: cwd)
        guard totalCalls > 0 || git.diffStat != nil else {
            return VizSpec(kind: .flow, empty: .init(title: "Chưa có công cụ nào chạy",
                                                     detail: "Dòng thời gian hiện khi Claude đọc, sửa file hoặc chạy lệnh."))
        }
        let files = combined.fileEdits
            .map { FlowFile(path: $0.key, name: ($0.key as NSString).lastPathComponent, count: $0.value) }
            .sorted { $0.count != $1.count ? $0.count > $1.count : $0.name < $1.name }
        return VizSpec(kind: .flow, flow: FlowSpec(branch: git.branch, diffStat: git.diffStat, files: files,
                                                   turns: turns, totalCalls: totalCalls))
    }

    func diagramSpec(_ facts: TranscriptFacts) -> VizSpec {
        let ordered = facts.mermaid.enumerated().sorted { lhs, rhs in
            switch (lhs.element.time, rhs.element.time) {
            case let (left?, right?): return left != right ? left > right : lhs.offset > rhs.offset
            case (nil, _?): return false
            case (_?, nil): return true
            case (nil, nil): return lhs.offset > rhs.offset
            }
        }.map(\.element)
        guard !ordered.isEmpty else {
            return VizSpec(kind: .diagram, empty: .init(title: "Chưa có sơ đồ nào",
                                                        detail: "Khi Claude trả lời bằng khối ```mermaid, sơ đồ hiện ở đây."))
        }
        let blocks = ordered.enumerated().map { index, block -> DiagramBlock in
            let firstLine = block.source.split(separator: "\n").first.map(String.init) ?? "mermaid"
            return DiagramBlock(id: "\(block.messageId)-\(index)", label: "\(index + 1). \(firstLine) (\(block.origin))",
                                source: block.source, origin: block.origin, time: block.time.map { $0.timeIntervalSince1970 * 1000 })
        }
        return VizSpec(kind: .diagram, diagram: DiagramSpec(blocks: blocks))
    }

    /// Scanning a folder for schemas can take a while on a big tree, so it is separate from `build` and
    /// runs only when the Entity tab is open.
    public func entitySpec(cwd: String) -> VizSpec {
        guard !cwd.isEmpty else {
            return VizSpec(kind: .entity, empty: .init(title: "Session chưa có thư mục", detail: nil))
        }
        let result = schemaScan(cwd: cwd)
        guard !result.model.tables.isEmpty else {
            return VizSpec(kind: .entity, empty: .init(
                title: "Không thấy schema trong thư mục này",
                detail: "Console đọc Prisma (*.prisma), SQL (CREATE TABLE trong *.sql) và Drift (class ... extends Table)."))
        }
        let inferred = result.model.drawnRelations().filter(\.inferred).count
        return VizSpec(kind: .entity, entity: EntitySpec(
            mermaid: result.model.mermaid(),
            tables: result.model.tables.map { EntityTable(id: ERModel.safeName($0.name), name: $0.name, columns: $0.columns.count) },
            links: result.model.drawnRelations().map {
                EntityLink(from: ERModel.safeName($0.from), to: ERModel.safeName($0.to), inferred: $0.inferred)
            },
            files: result.files, tableCount: result.model.tables.count,
            relationCount: result.model.drawnRelations().count, inferredCount: inferred, warnings: result.warnings))
    }

    private func schemaScan(cwd: String) -> SchemaScanResult {
        lock.lock()
        if let cached = entityCache[cwd], now().timeIntervalSince(cached.at) < entityTTL { lock.unlock(); return cached.result }
        lock.unlock()
        let result = SchemaScanner.scan(root: URL(fileURLWithPath: cwd))
        lock.lock(); entityCache[cwd] = (now(), result); lock.unlock()
        return result
    }

    func chartSpec(_ facts: TranscriptFacts, subagents: [Subagent]) -> VizSpec {
        var points = facts.usage.filter { $0.time > 0 }.sorted { $0.time < $1.time }
        let totals = points.reduce(UsageTotals(input: 0, output: 0, cacheCreation: 0, cacheRead: 0)) { sum, point in
            UsageTotals(input: sum.input + point.input, output: sum.output + point.output,
                        cacheCreation: sum.cacheCreation + point.cacheCreation, cacheRead: sum.cacheRead + point.cacheRead)
        }
        if points.count > Self.maxChartPoints {
            let step = Double(points.count) / Double(Self.maxChartPoints)
            points = (0..<Self.maxChartPoints).map { points[Int(Double($0) * step)] }
        }
        let tools = facts.toolCounts.map { ChartCount(name: $0.key, count: $0.value) }
            .sorted { $0.count != $1.count ? $0.count > $1.count : $0.name < $1.name }.prefix(12)
        var modelCounts: [String: Int] = [:]
        for agent in subagents where !agent.model.isEmpty { modelCounts[agent.model, default: 0] += 1 }
        let models = modelCounts.map { ModelCount(model: $0.key, count: $0.value) }
            .sorted { $0.count != $1.count ? $0.count > $1.count : $0.model < $1.model }
        guard !points.isEmpty || !tools.isEmpty else {
            return VizSpec(kind: .chart, empty: .init(title: "Chưa có số liệu",
                                                      detail: "Biểu đồ token và công cụ hiện khi session đã trả lời ít nhất một lần."))
        }
        return VizSpec(kind: .chart, chart: ChartSpec(tokens: points, totals: totals, tools: Array(tools), models: models))
    }
}
