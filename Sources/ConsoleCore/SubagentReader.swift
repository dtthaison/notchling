import Foundation

private struct SubagentMeta: Decodable {
    let agentType: String?
    let description: String?
    let toolUseId: String?
    let spawnDepth: Int?
    let model: String?
    let finished: Bool?
}

/// Reads `<project dir>/<sessionId>/subagents/agent-<id>.meta.json` next to each `agent-<id>.jsonl`.
public struct SubagentReader {
    private let now: () -> Date
    /// A subagent that never wrote `finished` counts as running only while its transcript keeps moving.
    public var runningWindow: TimeInterval = 10 * 60

    public init(now: @escaping () -> Date = Date.init) {
        self.now = now
    }

    public func subagents(sessionId: String, transcriptURL: URL?) -> [Subagent] {
        guard let transcriptURL else { return [] }
        let directory = transcriptURL.deletingLastPathComponent()
            .appendingPathComponent(sessionId).appendingPathComponent("subagents")
        let current = now()

        let agents: [Subagent] = JSONFile.contents(of: directory)
            .filter { $0.lastPathComponent.hasPrefix("agent-") && $0.lastPathComponent.hasSuffix(".meta.json") }
            .compactMap { metaURL in
                guard let meta = JSONFile.decode(SubagentMeta.self, at: metaURL) else { return nil }
                let name = metaURL.lastPathComponent
                let id = String(name.dropFirst("agent-".count).dropLast(".meta.json".count))
                let jsonl = directory.appendingPathComponent("agent-\(id).jsonl")
                let lastActivity = modificationDate(of: jsonl) ?? modificationDate(of: metaURL) ?? .distantPast
                let finished = meta.finished ?? false
                let state: SubagentState = !finished && current.timeIntervalSince(lastActivity) < runningWindow
                    ? .running : .done
                return Subagent(
                    id: id, type: meta.agentType ?? "agent", description: meta.description ?? "",
                    model: meta.model ?? "", depth: meta.spawnDepth ?? 1, parentToolUseId: meta.toolUseId,
                    finished: finished, lastActivity: lastActivity, transcriptURL: jsonl, state: state)
            }
        return agents.sorted { lhs, rhs in
            if (lhs.state == .running) != (rhs.state == .running) { return lhs.state == .running }
            return lhs.lastActivity > rhs.lastActivity
        }
    }
}
