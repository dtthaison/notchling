import Foundation

/// One read of everything: sessions, their subagents and pending prompts, merged and sorted.
/// Cheap enough to run every second; call it from one queue only (SessionSource keeps state).
public final class SessionCatalog {
    private let source: SessionSource
    private let subagentReader: SubagentReader
    private let promptReader: PendingPromptReader

    public init(paths: ClaudePaths = .live,
                now: @escaping () -> Date = Date.init,
                isAlive: @escaping (Int32) -> Bool = isProcessAlive,
                isOwned: @escaping (Int32) -> Bool = { _ in false }) {
        let locator = TranscriptLocator(projectsDir: paths.projectsDir, now: now)
        source = SessionSource(paths: paths, locator: locator, now: now, isAlive: isAlive, isOwned: isOwned)
        subagentReader = SubagentReader(now: now)
        promptReader = PendingPromptReader(requestsDir: paths.requestsDir, isAlive: isAlive)
    }

    public func snapshot() -> [Session] {
        let prompts = promptReader.prompts()
        let sessions = source.poll().map { base -> Session in
            var session = base
            guard session.state != .exited else { return session }
            session.subagents = subagentReader.subagents(sessionId: session.id, transcriptURL: session.transcriptURL)
            if session.state == .done, session.subagents.contains(where: { $0.state == .running }) {
                session.state = .running
            }
            if let prompt = prompts[session.id] {
                session.pending = prompt
                session.state = .needsInput
            }
            return session
        }
        return sessions.sorted { lhs, rhs in
            if lhs.state != rhs.state { return lhs.state.rawValue < rhs.state.rawValue }
            return lhs.lastActivity > rhs.lastActivity
        }
    }
}
