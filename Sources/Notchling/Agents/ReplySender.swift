import Foundation

/// Sends a reply to a background agent by running `claude attach <id>` in a hidden pseudo-terminal,
/// typing the text and Enter, then detaching. The session keeps running after the client exits.
enum ReplySender {
    enum Failure: Error, LocalizedError {
        case pty, launch(String), timeout

        var errorDescription: String? {
            switch self {
            case .pty: return "Couldn't open a terminal for the reply."
            case .launch(let why): return "Couldn't run claude attach: \(why)"
            case .timeout: return "The session didn't open in time."
            }
        }
    }

    static let claudePath: String = {
        let local = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/claude").path
        return FileManager.default.isExecutableFile(atPath: local) ? local : "/usr/local/bin/claude"
    }()

    /// Blocking; call off the main thread. A background job goes through `claude attach`; a terminal session whose
    /// pty lives in the session host (hostd) is typed into through the host, so the Mac and the phone share one terminal.
    static func send(_ text: String, to agent: Agent) throws {
        switch agent.target {
        case .job(let jobID): try send(text, toJob: jobID)
        case .process(let pid) where HostTerminals.owns(pid): try HostTerminals.send(ShellCommand.normalize(text), pid)
        default: throw Failure.launch("this session takes no replies")
        }
    }

    /// Blocking; call off the main thread.
    static func send(_ text: String, toJob jobID: String) throws {
        // A "!" command copied from a terminal is put back together first (wrapped lines, prompts, quotes).
        let text = ShellCommand.normalize(text)
        // Bracketed paste keeps newlines in the reply from submitting early.
        let flat = text.replacingOccurrences(of: "\r\n", with: "\n")
        try type(["\u{1b}[200~" + flat + "\u{1b}[201~", "\r"], toJob: jobID)
    }

    enum Choice: Equatable {
        case option(Int)   // 1-based
        case text(String)
        case options([Int])  // 1-based, multi-select
    }

    /// Answers an AskUserQuestion prompt in a background job's terminal.
    static func answer(_ choices: [Choice], to question: PendingQuestion, toJob jobID: String) throws {
        try type(answerKeys(choices, to: question), toJob: jobID)
    }

    /// The keystrokes for an AskUserQuestion prompt, one chunk per key press. The terminal shows one question at a
    /// time: a digit picks an option (single select) or toggles it (multi select), the number after the last option is
    /// "Type something". A multi-select list ends with an "Other" row, so Tab count+1 times lands on Submit/Next and
    /// Enter confirms. A final review screen (several questions, or any multi-select) needs "1" to submit.
    static func answerKeys(_ choices: [Choice], to question: PendingQuestion) -> [String] {
        var chunks: [String] = []
        for (item, choice) in zip(question.items, choices) {
            switch choice {
            case .option(let n):
                chunks.append("\(n)")
            case .text(let t):
                chunks += ["\(item.options.count + 1)", t.replacingOccurrences(of: "\n", with: " "), "\r"]
            case .options(let numbers):
                chunks += numbers.map(String.init)
                chunks += Array(repeating: "\t", count: item.options.count + 1)
                chunks.append("\r")
            }
        }
        if question.items.count > 1 || question.items.contains(where: \.multiSelect) { chunks.append("1") }
        return chunks
    }

    /// The answers a hook-held question is released with, keyed by question text. Claude Code joins multi-select
    /// answers with ", " itself.
    static func hookAnswers(_ choices: [Choice], for question: PendingQuestion) -> [String: String] {
        var answers: [String: String] = [:]
        for (item, choice) in zip(question.items, choices) {
            switch choice {
            case .option(let n):
                guard item.options.indices.contains(n - 1) else { continue }
                answers[item.question] = item.options[n - 1]
            case .text(let t):
                answers[item.question] = t
            case .options(let numbers):
                let labels = numbers.compactMap { item.options.indices.contains($0 - 1) ? item.options[$0 - 1] : nil }
                guard !labels.isEmpty else { continue }
                answers[item.question] = labels.joined(separator: ", ")
            }
        }
        return answers
    }

    /// Attaches, sends each chunk of keystrokes with a pause between them, then detaches.
    private static func type(_ chunks: [String], toJob jobID: String) throws {
        let master = posix_openpt(O_RDWR | O_NOCTTY)
        guard master >= 0, grantpt(master) == 0, unlockpt(master) == 0, let name = ptsname(master) else {
            throw Failure.pty
        }
        defer { close(master) }
        var size = winsize(ws_row: 50, ws_col: 200, ws_xpixel: 0, ws_ypixel: 0)
        _ = ioctl(master, TIOCSWINSZ, &size)

        let slave = FileHandle(forUpdatingAtPath: String(cString: name))
        guard let slave else { throw Failure.pty }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: claudePath)
        process.arguments = ["attach", jobID]
        process.standardInput = slave
        process.standardOutput = slave
        process.standardError = slave
        // A lane's job is only reachable through its own config folder.
        var env = AgentMonitor.environment(forJob: jobID)
        env["TERM"] = "xterm-256color"
        process.environment = env
        do { try process.run() } catch { throw Failure.launch(error.localizedDescription) }
        try? slave.close()
        defer { if process.isRunning { process.terminate() } }

        // Wait for the session UI to draw and then go quiet before typing.
        guard waitForQuiet(master, quiet: 1.2, timeout: 15) else { throw Failure.timeout }

        for chunk in chunks {
            write(master, chunk)
            Thread.sleep(forTimeInterval: 0.4)
        }
        _ = waitForQuiet(master, quiet: 1.0, timeout: 4)
    }

    private static func write(_ fd: Int32, _ s: String) {
        let bytes = Array(s.utf8)
        var offset = 0
        while offset < bytes.count {
            let n = bytes[offset...].withUnsafeBufferPointer { Darwin.write(fd, $0.baseAddress, $0.count) }
            if n <= 0 { return }
            offset += n
        }
    }

    /// Drains output until nothing arrives for `quiet` seconds. False if no output arrived before `timeout`.
    private static func waitForQuiet(_ fd: Int32, quiet: TimeInterval, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        var lastOutput: Date?
        var buffer = [UInt8](repeating: 0, count: 65536)
        while Date() < deadline {
            var pfd = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
            if poll(&pfd, 1, 100) > 0 {
                if read(fd, &buffer, buffer.count) <= 0 { return lastOutput != nil }
                lastOutput = Date()
            } else if let last = lastOutput, Date().timeIntervalSince(last) >= quiet {
                return true
            }
        }
        return lastOutput != nil
    }
}

/// Terminal sessions that live in the session host (hostd), set by the phone bridge at launch. Nothing is owned
/// until then, so a build without the bridge behaves as before.
enum HostTerminals {
    nonisolated(unsafe) static var owns: (pid_t) -> Bool = { _ in false }
    nonisolated(unsafe) static var send: (String, pid_t) throws -> Void = { _, _ in }
    /// Raw keystrokes (no bracketed paste) for answering prompts.
    nonisolated(unsafe) static var keys: ([String], pid_t) throws -> Void = { _, _ in }
}

extension Agent {
    /// A terminal session whose pty the host owns: replies can be typed into it from Chat.
    var repliesViaHost: Bool {
        if case .process(let pid) = target { return HostTerminals.owns(pid) }
        return false
    }

    /// Chat's reply box is available.
    var canType: Bool { canReply || repliesViaHost }
}
