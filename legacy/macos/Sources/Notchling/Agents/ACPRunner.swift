import Foundation

/// A permission the agent asked for mid-turn (`session/request_permission`), waiting for the user.
struct ACPPermission: Equatable {
    struct Option: Equatable, Identifiable {
        let id: String
        let name: String
        /// allow_once, allow_always, reject_once, reject_always.
        let kind: String
        var allows: Bool { kind.hasPrefix("allow") }
    }
    /// The JSON-RPC id to answer (a string or a number), kept as text and sent back as it came.
    let rpcID: String
    let rpcIDIsNumber: Bool
    let call: ToolCall
    let options: [Option]
}

/// A choice the agent offers for the session: Antigravity's `configOptions` (Model, Thinking Level, Mode).
struct ACPConfigOption: Equatable, Identifiable {
    struct Value: Equatable { let value: String; let name: String }
    let id: String
    let name: String
    var current: String
    let values: [Value]

    static func parse(_ list: Any?) -> [ACPConfigOption] {
        (list as? [[String: Any]] ?? []).compactMap { o in
            guard let id = o["id"] as? String, (o["type"] as? String) == "select" else { return nil }
            let values = (o["options"] as? [[String: Any]] ?? []).compactMap { v -> Value? in
                guard let value = v["value"] as? String else { return nil }
                return Value(value: value, name: v["name"] as? String ?? value)
            }
            return ACPConfigOption(id: id, name: o["name"] as? String ?? id, current: o["currentValue"] as? String ?? "", values: values)
        }
    }

    var currentName: String { values.first { $0.value == current }?.name ?? current }
}

/// The tools Chat drives over the Agent Client Protocol. Only Antigravity CLI now (it replaced Gemini CLI
/// on 2026-06-18); the runner stays generic so another ACP tool is one case.
enum ACPTool: Equatable {
    case antigravity

    var name: String { "Antigravity" }
    /// What the row's `host` and label show.
    var host: String { name }
    /// Chat row ids are `<prefix><session id>`; a draft is `<draftPrefix><uuid>`.
    var idPrefix: String { "agy:" }
    var draftPrefix: String { "agy-new:" }
    var arguments: [String] { ["acp"] }
    var command: String { "agy" }
    var product: String { "Antigravity CLI" }

    var binary: String? { AntigravityRunner.binary() }
    var runner: ACPRunner { AntigravityRunner.shared }

    func agentID(_ session: String) -> String { idPrefix + session }
    func target(_ session: String) -> AgentTarget { .antigravity(sessionID: session) }

    /// A title from the first message until the tool names the session.
    func title(_ text: String) -> String { AntigravitySessions.title(text) }

    /// The rows for Chat's sidebar.
    func list(live: Set<String>) -> [Agent] { AntigravitySessions.list(live: live) }
}

/// Sessions Chat has open over the Agent Client Protocol: one process each, which loads the conversation
/// (`session/load` replays it), takes prompts, streams the turn, asks permissions and switches models.
/// A process that sits idle is closed after a while; opening the session again reloads it.
/// Main thread only.
class ACPRunner: ObservableObject {
    let tool: ACPTool

    init(tool: ACPTool) { self.tool = tool }

    final class Live {
        let connection: ACPConnection
        let folder: String
        var sessionID: String?
        var transcript = ACPTranscript()
        var loaded = false
        var working = false
        var permission: ACPPermission?
        var config: [ACPConfigOption] = []
        var commands: [SlashCommand] = []
        var lastUsed = Date()
        /// When loading failed; a later open retries after `retryAfter`.
        var failedAt: Date?
        /// Work waiting for the load to finish.
        var whenLoaded: [() -> Void] = []
        var loadError: String?

        init(connection: ACPConnection, folder: String) {
            self.connection = connection
            self.folder = folder
        }
    }

    /// Bumped on every change, so views and `ChatModel` refresh.
    @Published private(set) var version = 0
    private var live: [String: Live] = [:]
    private var reaper: Timer?
    private var bumpScheduled = false

    /// Sessions with a turn running.
    var working: Set<String> { Set(live.filter { $0.value.working }.map(\.key)) }

    func isWorking(_ id: String) -> Bool { live[id]?.working == true }
    func permission(_ id: String) -> ACPPermission? { live[id]?.permission }
    func config(_ id: String) -> [ACPConfigOption] { live[id]?.config ?? [] }
    func commands(_ id: String) -> [SlashCommand] { live[id]?.commands ?? [] }
    func title(_ id: String) -> String? { live[id]?.transcript.title }
    func loadError(_ id: String) -> String? { live[id]?.loadError }

    /// The conversation as far as it has loaded; nil until the session is opened.
    func conversation(_ id: String) -> Conversation? {
        guard let l = live[id] else { return nil }
        l.lastUsed = Date()
        return l.transcript.conversation()
    }

    /// The conversation as far as it has loaded, without counting as use (the phone's session list reads it
    /// every few seconds; that must not keep an idle process from being reaped).
    func peekConversation(_ id: String) -> Conversation? { live[id]?.transcript.conversation() }

    func isOpen(_ id: String) -> Bool { live[id] != nil }

    func isLoading(_ id: String) -> Bool { live[id].map { !$0.loaded && $0.loadError == nil } ?? false }

    /// How long a failed load is shown before opening the session tries again.
    private static let retryAfter: TimeInterval = 15

    /// Starts a process for the session and loads it, unless one is open already. A load that failed is
    /// tried again once `retryAfter` has passed (Chat opens the selected session on every refresh).
    func open(_ id: String, folder: String, retryNow: Bool = false) {
        if let l = live[id] {
            guard let at = l.failedAt, retryNow || Date().timeIntervalSince(at) > Self.retryAfter else {
                l.lastUsed = Date()
                return
            }
            live[id] = nil
            l.connection.close()
        }
        guard let binary = tool.binary else { return }
        let connection = ACPConnection(binary: binary, arguments: tool.arguments, folder: folder, toolName: tool.name)
        let l = Live(connection: connection, folder: folder)
        l.sessionID = id
        live[id] = l
        wire(l)
        do { try connection.start() } catch {
            failLoad(l, "Couldn't start \(tool.name): \(error.localizedDescription)")
            return
        }
        initialize(l) { [weak self] ok in
            guard let self, ok else { return }
            connection.request("session/load", ["sessionId": id, "cwd": folder, "mcpServers": []]) { [weak self] result in
                guard let self else { return }
                switch result {
                case .success(let r):
                    self.loaded(l, r)
                case .failure(let e):
                    self.failLoad(l, e.localizedDescription)
                }
            }
        }
        startReaper()
    }

    /// A new session in `folder`; `done` gets its id.
    func start(in folder: String, done: @escaping (Result<String, Error>) -> Void) {
        guard let binary = tool.binary else {
            done(.failure(AccountError("Couldn't find the \(tool.command) command. Install \(tool.product) first.")))
            return
        }
        let connection = ACPConnection(binary: binary, arguments: tool.arguments, folder: folder, toolName: tool.name)
        let l = Live(connection: connection, folder: folder)
        wire(l)
        do { try connection.start() } catch { done(.failure(error)); return }
        initialize(l) { [weak self] ok in
            guard let self else { return }
            guard ok else {
                connection.close()
                done(.failure(AccountError(l.loadError ?? "\(self.tool.name) didn't start.")))
                return
            }
            connection.request("session/new", ["cwd": folder, "mcpServers": []]) { [weak self] result in
                guard let self else { return }
                switch result {
                case .success(let r):
                    guard let id = r["sessionId"] as? String else {
                        connection.close()
                        done(.failure(AccountError("\(self.tool.name) didn't start a session.")))
                        return
                    }
                    l.sessionID = id
                    self.live[id] = l
                    self.loaded(l, r)
                    self.startReaper()
                    done(.success(id))
                case .failure(let e):
                    connection.close()
                    done(.failure(AccountError(e.localizedDescription)))
                }
            }
        }
    }

    /// Sends a prompt; `ended` runs when the turn ends, with an error message if it failed.
    func prompt(_ id: String, text: String, folder: String, ended: @escaping (String?) -> Void) {
        // Sending is a clear request to try again after a failed load.
        open(id, folder: folder, retryNow: live[id]?.failedAt != nil)
        guard let l = live[id] else { ended("Couldn't find the \(tool.command) command. Install \(tool.product) first."); return }
        let send = { [weak self] in
            guard let self else { return }
            if let error = l.loadError { ended(error); return }
            l.working = true
            l.lastUsed = Date()
            l.transcript.beginTurn()
            self.bump()
            l.connection.request("session/prompt", ["sessionId": id, "prompt": [["type": "text", "text": text]]]) { [weak self] result in
                l.working = false
                l.permission = nil
                l.lastUsed = Date()
                switch result {
                case .success(let r):
                    l.transcript.endTurn(stopReason: r["stopReason"] as? String, usage: r["usage"] as? [String: Any])
                    ended(nil)
                case .failure(let e):
                    l.transcript.failTurn(e.localizedDescription)
                    ended(e.localizedDescription)
                }
                self?.bump()
            }
        }
        if l.loaded || l.loadError != nil { send() } else { l.whenLoaded.append(send) }
    }

    /// Stops the running turn; what was said so far stays.
    func cancel(_ id: String) {
        guard let l = live[id], l.working else { return }
        if let p = l.permission {
            answer(id, option: p.options.first { !$0.allows }?.id, cancelled: true)
        }
        l.connection.notify("session/cancel", ["sessionId": id])
    }

    /// Answers the waiting permission with one of its options (nil or `cancelled`: the turn was stopped).
    func answer(_ id: String, option: String?, cancelled: Bool = false) {
        guard let l = live[id], let p = l.permission else { return }
        l.permission = nil
        let rpcID: Any = p.rpcIDIsNumber ? (Int(p.rpcID) ?? 0) : p.rpcID
        if cancelled || option == nil {
            l.connection.respond(rpcID, result: ["outcome": ["outcome": "cancelled"]])
        } else if let option {
            l.connection.respond(rpcID, result: ["outcome": ["outcome": "selected", "optionId": option]])
        }
        bump()
    }

    /// Picks a value for one of the session's options (model, thinking level, mode).
    func set(_ id: String, option: String, to value: String, failed: @escaping (String) -> Void) {
        guard let l = live[id] else { return }
        l.connection.request("session/set_config_option", ["sessionId": id, "configId": option, "value": value]) { [weak self] result in
            switch result {
            case .success(let r):
                let fresh = ACPConfigOption.parse(r["configOptions"])
                if !fresh.isEmpty {
                    l.config = fresh
                } else if let i = l.config.firstIndex(where: { $0.id == option }) {
                    l.config[i].current = value
                }
                if option == "model" { l.transcript.model = l.config.first { $0.id == "model" }?.currentName }
                self?.bump()
            case .failure(let e):
                failed(e.localizedDescription)
            }
        }
    }

    /// Ends the process for a session (it's reloaded when opened again).
    func close(_ id: String) {
        live.removeValue(forKey: id)?.connection.close()
        bump()
    }

    // MARK: Private

    private func wire(_ l: Live) {
        l.connection.onNotification = { [weak self, weak l] method, params in
            guard let self, let l, method == "session/update", let update = params["update"] as? [String: Any] else { return }
            switch update["sessionUpdate"] as? String {
            case "available_commands_update":
                l.commands = (update["availableCommands"] as? [[String: Any]] ?? []).compactMap { c in
                    guard let name = c["name"] as? String else { return nil }
                    let hint = (c["input"] as? [String: Any])?["hint"] as? String
                    return SlashCommand(name: name, description: c["description"] as? String ?? "", argumentHint: hint)
                }
            case "config_option_update":
                let fresh = ACPConfigOption.parse(update["configOptions"])
                if !fresh.isEmpty { l.config = fresh }
            case "current_mode_update":
                if let mode = update["currentModeId"] as? String, let i = l.config.firstIndex(where: { $0.id == "mode" }) {
                    l.config[i].current = mode
                }
            default:
                l.transcript.apply(update, at: l.loaded ? Date() : nil)
            }
            self.bump()
        }
        l.connection.onRequest = { [weak self, weak l] id, method, params in
            guard let self, let l else { return }
            guard method == "session/request_permission", let call = params["toolCall"] as? [String: Any] else {
                // No file system or terminal access is offered in `initialize`; anything else is refused.
                l.connection.respond(id, errorCode: -32601, message: "Not supported")
                return
            }
            l.transcript.apply(call.merging(["sessionUpdate": "tool_call_update"]) { _, new in new }, at: Date())
            let options = (params["options"] as? [[String: Any]] ?? []).compactMap { o -> ACPPermission.Option? in
                guard let oid = o["optionId"] as? String else { return nil }
                return ACPPermission.Option(id: oid, name: o["name"] as? String ?? oid, kind: o["kind"] as? String ?? "")
            }
            l.permission = ACPPermission(rpcID: "\(id)", rpcIDIsNumber: id is Int || id is NSNumber,
                                         call: ACPTranscript.toolCall(call, merging: nil), options: options)
            self.bump()
        }
        l.connection.onExit = { [weak self, weak l] message in
            guard let self, let l else { return }
            if !l.loaded {
                self.failLoad(l, message ?? "\(self.tool.name) stopped.")
            } else {
                if let id = l.sessionID, self.live[id] === l { self.live[id] = nil }
                self.bump()
            }
        }
    }

    private func initialize(_ l: Live, then: @escaping (Bool) -> Void) {
        let params: [String: Any] = [
            "protocolVersion": 1,
            "clientCapabilities": ["fs": ["readTextFile": false, "writeTextFile": false], "terminal": false],
            "clientInfo": ["name": "Notchling", "version": "1"],
        ]
        l.connection.request("initialize", params) { [weak self] result in
            if case .failure(let e) = result {
                if l.sessionID != nil { self?.failLoad(l, e.localizedDescription) } else { l.loadError = e.localizedDescription }
                then(false)
            } else {
                then(true)
            }
        }
    }

    /// Loading failed: the error stays on show, sends that waited for the load end with it (they would
    /// otherwise wait forever), and the process is closed so a later open starts a fresh one.
    private func failLoad(_ l: Live, _ message: String) {
        guard !l.loaded, l.failedAt == nil else { return }
        l.loadError = message
        l.failedAt = Date()
        let waiting = l.whenLoaded
        l.whenLoaded = []
        waiting.forEach { $0() }
        l.connection.close()
        bump()
    }

    private func loaded(_ l: Live, _ result: [String: Any]) {
        l.loaded = true
        l.config = ACPConfigOption.parse(result["configOptions"])
        if let model = l.config.first(where: { $0.id == "model" })?.currentName {
            l.transcript.model = model
        } else if let models = result["models"] as? [String: Any], let current = models["currentModelId"] as? String {
            l.transcript.model = current
        }
        let waiting = l.whenLoaded
        l.whenLoaded = []
        waiting.forEach { $0() }
        bump()
    }

    /// Coalesces bursts of chunks into one change per run-loop pass.
    private func bump() {
        guard !bumpScheduled else { return }
        bumpScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.bumpScheduled = false
            self.version &+= 1
        }
    }

    /// Closes processes nobody has looked at for ten minutes and that aren't working or asking.
    private func startReaper() {
        guard reaper == nil else { return }
        reaper = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            guard let self else { return }
            for (id, l) in self.live where !l.working && l.permission == nil && Date().timeIntervalSince(l.lastUsed) > 600 {
                self.close(id)
            }
            if self.live.isEmpty { self.reaper?.invalidate(); self.reaper = nil }
        }
    }

    /// At quit: no ACP process left behind.
    func closeAll() {
        for l in live.values { l.connection.close() }
        live = [:]
    }
}
