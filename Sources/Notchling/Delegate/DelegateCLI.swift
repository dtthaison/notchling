import Foundation

/// Runs `Notchling --delegate` and `--delegate-models` in this short-lived process, without the app:
/// Codex as one `codex exec --json` turn, Antigravity CLI over `agy acp`. The session it makes is the
/// tool's own, so Chat lists it with the others. Never returns.
enum DelegateCLI {
    static func run(_ args: [String]) -> Never {
        let options: Delegate.Options
        switch Delegate.parse(args, cwd: FileManager.default.currentDirectoryPath) {
        case .failure(let e): fail(e.message, code: 2)
        case .success(let o): options = o
        }
        let depth = Int(ProcessInfo.processInfo.environment[Delegate.depthVariable] ?? "") ?? 0
        guard depth < Delegate.maxDepth else {
            fail("This job was itself delegated \(depth) times; do it yourself instead of delegating again.", code: 2)
        }
        setenv(Delegate.depthVariable, String(depth + 1), 1)
        guard FileManager.default.fileExists(atPath: options.folder) else { fail("No folder at \(options.folder).", code: 2) }

        let finish: (Delegate.Outcome) -> Never = { outcome in
            var o = outcome
            if let out = options.out, !o.media.isEmpty {
                do {
                    o.copied = try Delegate.copy(o.media.map { URL(fileURLWithPath: $0) }, to: URL(fileURLWithPath: out)).map(\.path)
                } catch {
                    o.error = o.error ?? "Couldn't copy to \(out): \(error.localizedDescription)"
                }
            }
            print(options.json ? Delegate.json(o) : Delegate.render(o))
            exit(o.exitCode)
        }
        switch options.tool {
        case .codex:
            finish(CodexDelegate(options).run())
        case .agy:
            AgyDelegate.start(options, done: finish)
            dispatchMain()
        }
    }

    static func models(_ args: [String]) -> Never {
        let only = args.first.flatMap(Delegate.Tool.init(name:))
        var lines: [String] = []
        if only != .agy {
            lines.append("codex (--model <slug> --effort <level>):")
            let models = CodexModels.load()
            if models.isEmpty { lines.append("  (Codex hasn't cached its model list yet; run codex once)") }
            for m in models {
                let def = m.defaultEffort.map { " (default \($0))" } ?? ""
                lines.append("  \(m.slug)  efforts: \(m.efforts.joined(separator: ", "))\(def)")
            }
        }
        if only != .codex {
            lines.append("agy (--model <model> --effort low|medium|high):")
            if let agy = AntigravityRunner.binary(), let listing = output(agy, ["models"]) {
                for m in Delegate.agyModels(listing) {
                    lines.append("  \(m.model)" + (m.levels.isEmpty ? "" : "  efforts: \(m.levels.joined(separator: ", "))"))
                }
            } else {
                lines.append("  (couldn't run agy models)")
            }
        }
        print(lines.joined(separator: "\n"))
        exit(0)
    }

    static func fail(_ message: String, code: Int32) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(code)
    }

    private static func output(_ binary: String, _ arguments: [String]) -> String? {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: binary)
        p.arguments = arguments
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch { return nil }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return p.terminationStatus == 0 ? String(decoding: data, as: UTF8.self) : nil
    }

    /// Stops the child when the caller stops this command (Claude Code's TaskStop sends SIGTERM).
    static func onTermination(_ handler: @escaping () -> Void) -> [DispatchSourceSignal] {
        [SIGINT, SIGTERM, SIGHUP].map { sig in
            signal(sig, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: sig, queue: .global())
            source.setEventHandler(handler: handler)
            source.resume()
            return source
        }
    }
}

/// One Codex turn: read-only sandbox unless `--write` (then workspace-write, the folder only).
private struct CodexDelegate {
    let options: Delegate.Options
    init(_ options: Delegate.Options) { self.options = options }

    func run() -> Delegate.Outcome {
        let start = Date()
        var o = Delegate.Outcome(tool: .codex, session: options.session, model: options.model, effort: options.effort)
        guard let codex = CodexStore.binary() else {
            o.error = "Couldn't find the codex command. Install the Codex CLI first."
            return o
        }
        let pick = CodexPick(model: options.model, effort: options.effort)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: codex)
        process.arguments = CodexRunner.arguments(prompt: options.prompt, session: options.session, folder: options.folder,
                                                  pick: pick, sandbox: options.write ? "workspace-write" : "read-only")
        process.currentDirectoryURL = URL(fileURLWithPath: options.folder)
        process.environment = CodexRunner.environment()
        let out = Pipe(), err = Pipe()
        process.standardOutput = out
        process.standardError = err
        process.standardInput = FileHandle.nullDevice
        do { try process.run() } catch {
            o.error = error.localizedDescription
            return o
        }

        let lock = NSLock()
        var timedOut = false
        let signals = DelegateCLI.onTermination { process.interrupt() }
        DispatchQueue.global().asyncAfter(deadline: .now() + options.timeout) {
            guard process.isRunning else { return }
            lock.lock(); timedOut = true; lock.unlock()
            process.interrupt()
            DispatchQueue.global().asyncAfter(deadline: .now() + 5) { if process.isRunning { process.terminate() } }
        }
        var errorTail = ""
        DispatchQueue.global().async {
            let text = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            lock.lock(); errorTail = text; lock.unlock()
        }

        var buffer = Data()
        var lastError: String?
        while true {
            let chunk = out.fileHandleForReading.availableData
            if chunk.isEmpty { break }
            buffer.append(chunk)
            while let newline = buffer.firstIndex(of: 0x0A) {
                let line = buffer[buffer.startIndex..<newline]
                buffer.removeSubrange(buffer.startIndex...newline)
                guard let event = (try? JSONSerialization.jsonObject(with: Data(line))) as? [String: Any] else { continue }
                switch event["type"] as? String {
                case "thread.started":
                    o.session = o.session ?? event["thread_id"] as? String
                case "item.completed":
                    if let item = event["item"] as? [String: Any], item["type"] as? String == "agent_message",
                       let text = item["text"] as? String, !text.isEmpty {
                        o.reply = text
                    }
                case "turn.completed":
                    if let u = event["usage"] as? [String: Any] {
                        o.tokens = ((u["input_tokens"] as? Int) ?? 0) + ((u["output_tokens"] as? Int) ?? 0)
                    }
                case "error", "turn.failed":
                    lastError = (event["message"] as? String) ?? ((event["error"] as? [String: Any])?["message"] as? String) ?? lastError
                default:
                    break
                }
            }
        }
        process.waitUntilExit()
        withExtendedLifetime(signals) {}
        lock.lock()
        o.timedOut = timedOut
        let stderrLine = errorTail.split(whereSeparator: \.isNewline).last(where: { $0.contains("rror") }).map(String.init)
        lock.unlock()
        if o.timedOut {
            o.error = "Stopped after \(Int(options.timeout)) s."
        } else if process.terminationStatus != 0 {
            o.error = lastError ?? stderrLine ?? "Codex exited with code \(process.terminationStatus)."
        }
        if let s = o.session {
            o.media = Delegate.media(in: Delegate.mediaFolder(.codex, session: s), since: start).map(\.path)
        }
        o.seconds = Date().timeIntervalSince(start)
        return o
    }
}

/// One Antigravity CLI turn over ACP. Nobody is there to answer its permission requests, so they are
/// declined (and listed in the result), except edits in the workspace with `--write`. Images its
/// subagent makes are picked up from its own folder, so it never needs a command to copy them.
private final class AgyDelegate {
    private static var current: AgyDelegate?

    private let options: Delegate.Options
    private let done: (Delegate.Outcome) -> Never
    private let connection: ACPConnection
    private var outcome: Delegate.Outcome
    private let start = Date()
    private var loading = false
    /// The agent's text since its last tool call; the reply is the last part that has any.
    private var segment = ""
    private var lastSegment = ""
    private var signals: [DispatchSourceSignal] = []
    private var watch: FileEvents?
    /// Subagents this turn started; what they made can be in their own folders too.
    private var subagents: [String] = []
    private var ended = false

    static func start(_ options: Delegate.Options, done: @escaping (Delegate.Outcome) -> Never) {
        guard let agy = AntigravityRunner.binary() else {
            var o = Delegate.Outcome(tool: .agy)
            o.error = "Couldn't find the agy command. Install Antigravity CLI first."
            done(o)
        }
        let d = AgyDelegate(options, binary: agy, done: done)
        current = d
        DispatchQueue.main.async { d.begin() }
    }

    private init(_ options: Delegate.Options, binary: String, done: @escaping (Delegate.Outcome) -> Never) {
        self.options = options
        self.done = done
        connection = ACPConnection(binary: binary, folder: options.folder)
        outcome = Delegate.Outcome(tool: .agy, session: options.session)
    }

    private func begin() {
        connection.onNotification = { [weak self] method, params in self?.notified(method, params) }
        connection.onRequest = { [weak self] id, method, params in self?.asked(id, method, params) }
        connection.onExit = { [weak self] tail in self?.end(error: tail ?? "Antigravity stopped.") }
        do { try connection.start() } catch { end(error: error.localizedDescription) }
        signals = DelegateCLI.onTermination { [weak self] in
            DispatchQueue.main.async { self?.cancel(error: "Stopped.") }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + options.timeout) { [weak self] in
            self?.outcome.timedOut = true
            self?.cancel(error: "Stopped after \(Int(self?.options.timeout ?? 0)) s.")
        }
        let params: [String: Any] = [
            "protocolVersion": 1,
            "clientCapabilities": ["fs": ["readTextFile": false, "writeTextFile": false], "terminal": false],
            "clientInfo": ["name": "Notchling", "version": "1"],
        ]
        connection.request("initialize", params) { [weak self] result in
            guard let self else { return }
            if case .failure(let e) = result { return self.end(error: e.localizedDescription) }
            self.openSession()
        }
    }

    private func openSession() {
        let handle: (Result<[String: Any], Error>) -> Void = { [weak self] result in
            guard let self else { return }
            self.loading = false
            switch result {
            case .failure(let e): self.end(error: e.localizedDescription)
            case .success(let r):
                if let id = r["sessionId"] as? String { self.outcome.session = id }
                self.configure(ACPConfigOption.parse(r["configOptions"]))
            }
        }
        if let id = options.session {
            loading = true  // `session/load` replays the history first; none of it is this turn's reply.
            connection.request("session/load", ["sessionId": id, "cwd": options.folder, "mcpServers": []], done: handle)
        } else {
            connection.request("session/new", ["cwd": options.folder, "mcpServers": []], done: handle)
        }
    }

    /// Sets model, thinking level and (with `--write`) accept-edits, one after another, then prompts.
    private func configure(_ config: [ACPConfigOption]) {
        let models = config.first { $0.id == "model" }?.values.map(\.value) ?? []
        let pick = Delegate.agyPick(model: options.model, effort: options.effort, models: models)
        var steps: [(id: String, value: String)] = []
        if let m = pick.model { steps.append(("model", m)) }
        if let e = pick.effort { steps.append(("thought_level", e)) }
        if options.write { steps.append(("mode", "accept-edits")) }
        for step in steps {
            guard let option = config.first(where: { $0.id == step.id }) else { continue }
            guard option.values.contains(where: { $0.value == step.value }) else {
                let flag = step.id == "model" ? "--model" : "--effort"
                return end(error: "\(flag) \(step.value) isn't offered. Choices: \(option.values.map(\.value).joined(separator: ", ")).")
            }
        }
        outcome.model = pick.model ?? config.first { $0.id == "model" }?.current
        outcome.effort = pick.effort ?? config.first { $0.id == "thought_level" }?.current
        setNext(steps[...])
    }

    private func setNext(_ steps: ArraySlice<(id: String, value: String)>) {
        guard let step = steps.first, let session = outcome.session else { return prompt() }
        connection.request("session/set_config_option", ["sessionId": session, "configId": step.id, "value": step.value]) { [weak self] result in
            if case .failure(let e) = result {
                self?.end(error: "Couldn't set \(step.id): \(e.localizedDescription)")
                return
            }
            self?.setNext(steps.dropFirst())
        }
    }

    private func prompt() {
        guard let session = outcome.session else { return end(error: "Antigravity didn't open a session.") }
        connection.request("session/prompt", ["sessionId": session, "prompt": [["type": "text", "text": options.prompt]]]) { [weak self] result in
            guard let self else { return }
            switch result {
            case .failure(let e): self.end(error: e.localizedDescription)
            case .success(let r):
                if let u = r["usage"] as? [String: Any] { self.outcome.tokens = u["totalTokens"] as? Int }
                let reason = r["stopReason"] as? String ?? "end_turn"
                guard reason == "end_turn" else { return self.end(error: "Antigravity stopped (\(reason)).") }
                self.awaitSubagents()
            }
        }
    }

    /// The turn may have left subagents working (the image generator does). Closing agy now would kill
    /// them, so wait, woken by changes to the session's files, until they report and agy answers.
    private func awaitSubagents() {
        guard let session = outcome.session else { return end(error: nil) }
        let system = Delegate.mediaFolder(.agy, session: session).appendingPathComponent(".system_generated")
        let check: () -> Bool = { [weak self] in
            guard let self else { return true }
            switch Delegate.agyProgress(system: system, since: self.start) {
            case .finished:
                self.end(error: nil)
            case .answered(let text, let subagents):
                self.subagents = subagents
                self.lastSegment = text
                self.segment = ""
                self.end(error: nil)
            case .waiting(let subagents):
                self.subagents = subagents
                return false
            }
            return true
        }
        if check() { return }
        FileHandle.standardError.write(Data("Waiting for Antigravity's subagents (\(subagents.count))…\n".utf8))
        watch = FileEvents(folders: [system], latency: 0.5, queue: .main) { _ in _ = check() }
        if watch == nil { end(error: "Couldn't watch \(system.path) for the subagents' results.") }
    }

    private func notified(_ method: String, _ params: [String: Any]) {
        guard method == "session/update", !loading, let update = params["update"] as? [String: Any] else { return }
        switch update["sessionUpdate"] as? String {
        case "agent_message_chunk":
            segment += (update["content"] as? [String: Any])?["text"] as? String ?? ""
        case "tool_call":
            closeSegment()
        default:
            break
        }
    }

    private func closeSegment() {
        let text = segment.trimmingCharacters(in: .whitespacesAndNewlines)
        if !text.isEmpty { lastSegment = text }
        segment = ""
    }

    private func asked(_ id: Any, _ method: String, _ params: [String: Any]) {
        guard method == "session/request_permission" else {
            return connection.respond(id, errorCode: -32601, message: "Not offered in a delegated run.")
        }
        let call = params["toolCall"] as? [String: Any] ?? [:]
        let kind = call["kind"] as? String ?? ""
        let options = params["options"] as? [[String: Any]] ?? []
        func option(_ prefix: String) -> String? {
            options.first { ($0["kind"] as? String ?? "") == prefix + "_once" }?["optionId"] as? String
                ?? options.first { ($0["kind"] as? String ?? "").hasPrefix(prefix) }?["optionId"] as? String
        }
        if self.options.write, ["edit", "delete", "move"].contains(kind), let allow = option("allow") {
            return connection.respond(id, result: ["outcome": ["outcome": "selected", "optionId": allow]])
        }
        outcome.declined.append(call["title"] as? String ?? kind)
        if let reject = option("reject") {
            connection.respond(id, result: ["outcome": ["outcome": "selected", "optionId": reject]])
        } else {
            connection.respond(id, result: ["outcome": ["outcome": "cancelled"]])
        }
    }

    private func cancel(error: String) {
        if let s = outcome.session { connection.notify("session/cancel", ["sessionId": s]) }
        // Give it a moment to end the turn cleanly, then stop regardless.
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in self?.end(error: error) }
    }

    private func end(error: String?) {
        guard !ended else { return }
        ended = true
        watch = nil
        closeSegment()
        outcome.reply = lastSegment
        outcome.error = outcome.error ?? error
        if outcome.timedOut { outcome.error = "Stopped after \(Int(options.timeout)) s." }
        if let s = outcome.session {
            outcome.media = ([s] + subagents).flatMap { Delegate.media(in: Delegate.mediaFolder(.agy, session: $0), since: start) }.map(\.path)
        }
        outcome.seconds = Date().timeIntervalSince(start)
        connection.onExit = nil
        connection.close()
        withExtendedLifetime(signals) {}
        done(outcome)
    }
}
