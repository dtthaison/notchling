import Foundation
import Observation

/// Where the chat pane is pointed.
public struct ChatSelection: Hashable, Sendable {
    public let sessionId: String
    public let subagentId: String?

    public init(sessionId: String, subagentId: String? = nil) {
        self.sessionId = sessionId
        self.subagentId = subagentId
    }
}

/// What one tick read for the selected chat.
struct ChatSnapshot: Sendable {
    var selection: ChatSelection?
    var messages: [ChatMessage] = []
    var model: String?
    var error: String?
}

/// Polls ~/.claude once a second on a background queue and publishes the result on the main actor.
@MainActor
@Observable
public final class SessionStore {
    public private(set) var sessions: [Session] = []
    public private(set) var chat: [ChatMessage] = []
    public private(set) var chatModel: String?
    /// The selection `chat` was read for; differs from `selection` while a new chat is loading.
    public private(set) var chatLoadedFor: ChatSelection?
    public private(set) var chatError: String?
    public private(set) var hasLoaded = false
    public private(set) var loadError: String?
    public var selection: ChatSelection? {
        didSet { if selection != oldValue { refreshNow() } }
    }

    @ObservationIgnored nonisolated(unsafe) private let catalog: SessionCatalog
    @ObservationIgnored nonisolated(unsafe) private let chatCache = ChatTranscriptCache()
    @ObservationIgnored private let claudeDir: URL
    @ObservationIgnored private let queue = DispatchQueue(label: "local.agentconsole.poll", qos: .utility)
    @ObservationIgnored private var timer: DispatchSourceTimer?
    @ObservationIgnored private let selectionLock = NSLock()
    @ObservationIgnored nonisolated(unsafe) private var polledSelection: ChatSelection?

    public init(catalog: SessionCatalog = SessionCatalog(), claudeDir: URL = ClaudePaths.live.claudeDir) {
        self.catalog = catalog
        self.claudeDir = claudeDir
    }

    public func start() {
        guard timer == nil else { return }
        let source = DispatchSource.makeTimerSource(queue: queue)
        source.schedule(deadline: .now(), repeating: .seconds(1), leeway: .milliseconds(100))
        source.setEventHandler { [weak self] in self?.tick() }
        source.resume()
        timer = source
    }

    public func stop() {
        timer?.cancel()
        timer = nil
    }

    /// "Thử lại": read again right now.
    public func reload() {
        refreshNow()
    }

    private func refreshNow() {
        selectionLock.lock(); polledSelection = selection; selectionLock.unlock()
        queue.async { [weak self] in self?.tick() }
    }

    private nonisolated func tick() {
        let claudeDir = self.claudeDir
        let unreadable = FileManager.default.isReadableFile(atPath: claudeDir.path)
            ? nil : "Không đọc được \(claudeDir.path)."
        let snapshot = unreadable == nil ? catalog.snapshot() : []
        selectionLock.lock(); let selected = polledSelection; selectionLock.unlock()
        let chat = chatSnapshot(for: selected, in: snapshot)
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated { self?.publish(snapshot, chat, loadError: unreadable) }
        }
    }

    private nonisolated func chatSnapshot(for selection: ChatSelection?, in sessions: [Session]) -> ChatSnapshot {
        guard let selection else { return ChatSnapshot() }
        guard let session = sessions.first(where: { $0.id == selection.sessionId }) else {
            return ChatSnapshot(selection: selection, error: "Session không còn trong danh sách.")
        }
        let url: URL?
        let isMain: Bool
        if let subagentId = selection.subagentId {
            url = session.subagents.first(where: { $0.id == subagentId })?.transcriptURL
            isMain = false
        } else {
            url = session.transcriptURL
            isMain = true
        }
        guard let url else {
            return ChatSnapshot(selection: selection, error: "Chưa tìm thấy transcript của session này.")
        }
        let result = chatCache.result(at: url, isMain: isMain)
        return ChatSnapshot(selection: selection, messages: result.messages, model: result.model, error: nil)
    }

    private func publish(_ snapshot: [Session], _ chat: ChatSnapshot, loadError: String?) {
        if snapshot != sessions { sessions = snapshot }
        if chat.messages != self.chat { self.chat = chat.messages }
        if chat.model != chatModel { chatModel = chat.model }
        if chat.error != chatError { chatError = chat.error }
        if chat.selection != chatLoadedFor { chatLoadedFor = chat.selection }
        if loadError != self.loadError { self.loadError = loadError }
        hasLoaded = true
    }
}
