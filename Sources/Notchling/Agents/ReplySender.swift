import Foundation

/// Sends a reply to a background agent by running `claude attach <id>` in a hidden pseudo-terminal,
/// typing the text and Enter, then detaching. The session keeps running after the client exits.
enum ReplySender {
    enum Failure: Error, LocalizedError {
        case pty, launch(String), timeout, dialog

        var errorDescription: String? {
            switch self {
            case .pty: return "Couldn't open a terminal for the reply."
            case .launch(let why): return "Couldn't run claude attach: \(why)"
            case .timeout: return "The session didn't open in time."
            case .dialog: return "The session is asking you something. Answer it in the terminal, then send again."
            }
        }
    }

    /// Words only a question or permission dialog shows, looked for near the bottom of the screen.
    /// A reply typed into one of those answers it: "create artifact for me see" went in as the answer to
    /// all three questions of an AskUserQuestion that Chat couldn't show (5 Nov). Checked on 2.1.318.
    static let dialogMarkers = ["Enter to select", "Esc to cancel", "Chat about this", "Do you want to proceed?",
                                "Do you want to allow", "Submit answers"]

    static func dialogOpen(_ screen: TerminalScreen) -> Bool {
        // The last drawn lines: a short session leaves blank rows under its prompt or dialog.
        let drawn = screen.lines.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
        return drawn.suffix(6).contains { line in dialogMarkers.contains { line.contains($0) } }
    }

    static let claudePath: String = {
        let local = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/claude").path
        return FileManager.default.isExecutableFile(atPath: local) ? local : "/usr/local/bin/claude"
    }()

    /// Blocking; call off the main thread.
    static func send(_ text: String, toJob jobID: String) throws {
        // A "!" command copied from a terminal is put back together first (wrapped lines, prompts, quotes).
        let text = ShellCommand.normalize(text)
        // Bracketed paste keeps newlines in the reply from submitting early.
        let flat = text.replacingOccurrences(of: "\r\n", with: "\n")
        try type([clearInput, "\u{1b}[200~" + flat + "\u{1b}[201~", "\r"], toJob: jobID, refuseDialog: true)
    }

    /// Empties the session's input box first. It keeps a draft across attaches, so text left there
    /// ("/rename", half a message) would join the reply: "/renamethe padding…" ran as an unknown
    /// command and the session did nothing. Per line: Ctrl+E (end), Ctrl+U (delete to start), Backspace
    /// (join the line above). Not Ctrl+K: sent in a burst it lands as a literal vertical tab, a line
    /// break that keeps Enter from submitting. Never Esc or Ctrl+C, which interrupt a busy session.
    /// Ctrl+Y in the terminal brings the text back. (Checked on 2.1.318.)
    static let clearInput = String(repeating: "\u{05}\u{15}\u{7F}", count: 20) + "\u{05}\u{15}"

    enum Choice: Equatable {
        case option(Int)   // 1-based
        case text(String)
    }

    /// Answers an AskUserQuestion prompt. The terminal shows one question at a time: a digit picks an
    /// option, the number after the last option is "Type something", and with several questions a
    /// final review screen needs "1" to submit.
    static func answer(_ choices: [Choice], to question: PendingQuestion, toJob jobID: String) throws {
        var chunks: [String] = []
        for (item, choice) in zip(question.items, choices) {
            switch choice {
            case .option(let n):
                chunks.append("\(n)")
            case .text(let t):
                chunks += ["\(item.options.count + 1)", t.replacingOccurrences(of: "\n", with: " "), "\r"]
            }
        }
        if question.items.count > 1 { chunks.append("1") }
        try type(chunks, toJob: jobID)
    }

    /// Stops the turn a background session is working on: Esc, as in its terminal. Only for a session
    /// that is working (Esc twice on an idle one opens the rewind menu). A busy screen keeps redrawing,
    /// so it doesn't wait for quiet as long as a reply does.
    static func interrupt(job jobID: String) throws {
        try type(["\u{1b}"], toJob: jobID, settle: 1.2)
    }

    /// `claude --bg` refuses a folder Claude Code hasn't trusted yet ("Workspace not trusted").
    static func isUntrusted(_ message: String) -> Bool { message.contains("not trusted") }

    /// Trusts `folder` the way you would: runs `claude` there in a hidden terminal, accepts its trust
    /// prompt (the first option, already selected) and quits, so Claude Code records it itself.
    /// `configDir` is a lane's folder, whose trust list is its own. Blocking; true when it was accepted.
    static func trustFolder(_ folder: String, configDir: String?) -> Bool {
        let master = posix_openpt(O_RDWR | O_NOCTTY)
        guard master >= 0, grantpt(master) == 0, unlockpt(master) == 0, let name = ptsname(master) else { return false }
        defer { close(master) }
        var size = winsize(ws_row: 50, ws_col: 200, ws_xpixel: 0, ws_ypixel: 0)
        _ = ioctl(master, TIOCSWINSZ, &size)
        guard let slave = FileHandle(forUpdatingAtPath: String(cString: name)) else { return false }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: claudePath)
        process.currentDirectoryURL = URL(fileURLWithPath: folder)
        process.standardInput = slave
        process.standardOutput = slave
        process.standardError = slave
        var env = ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env.removeValue(forKey: "NO_COLOR")
        if let configDir { env[LaneStore.variable] = configDir }
        process.environment = env
        do { try process.run() } catch { return false }
        try? slave.close()
        defer { if process.isRunning { process.terminate() } }

        let screen = TerminalScreen(rows: Int(size.ws_row), cols: Int(size.ws_col))
        func asking() -> Bool { screen.lines.contains { $0.contains("Yes, I trust this folder") } }
        let deadline = Date().addingTimeInterval(20)
        while Date() < deadline, !asking() { _ = waitForQuiet(master, quiet: 0.5, timeout: 2, into: screen) }
        guard asking() else { return false }
        // "No, exit" comes first and is selected (2.1.318): move down to Yes, then confirm.
        write(master, "\u{1b}[B")
        Thread.sleep(forTimeInterval: 0.3)
        write(master, "\r")
        // Wait for the session screen it opens once trusted, then quit it.
        _ = waitForQuiet(master, quiet: 1, timeout: 8, into: screen)
        for _ in 0..<2 { write(master, "\u{03}"); Thread.sleep(forTimeInterval: 0.3) }
        _ = waitForQuiet(master, quiet: 0.5, timeout: 3)
        return true
    }

    /// Attaches, sends each chunk of keystrokes with a pause between them, then detaches. `settle`:
    /// send after the screen has drawn for this long even if it never goes quiet (a working session).
    private static func type(_ chunks: [String], toJob jobID: String, refuseDialog: Bool = false, settle: TimeInterval? = nil) throws {
        try attached(jobID, refuseDialog: refuseDialog, settle: settle) { master, _ in
            for chunk in chunks {
                write(master, chunk)
                Thread.sleep(forTimeInterval: 0.15)
            }
        }
    }

    /// Takes the queued messages back out of a working session's queue: Up in its empty input box
    /// pulls them all into the box (`popAll`), which is then cleared. `keep` (the older ones) go back in
    /// the queue, in order. True when `latest` was there to take; false leaves the queue as it was.
    static func takeBackQueued(latest: String, keep: [String], fromJob jobID: String, transcript: String) throws -> Bool {
        var taken = false
        try attached(jobID, refuseDialog: true, settle: 1.2) { master, screen in
            write(master, clearInput)
            Thread.sleep(forTimeInterval: 0.3)
            let pressed = Date()
            write(master, "\u{1b}[A")
            // Up on an empty box with nothing queued brings back your last prompt, which may be these
            // same words, so the screen can't tell. The transcript can: taking the queue logs popAll.
            let deadline = Date().addingTimeInterval(3)
            while Date() < deadline, !taken {
                _ = waitForQuiet(master, quiet: 0.3, timeout: 0.5, into: screen)
                taken = poppedAll(latest, in: transcript, since: pressed.addingTimeInterval(-1))
            }
            write(master, clearInput)
            Thread.sleep(forTimeInterval: 0.3)
            // The queue had already gone (the turn ended): what Up brought back was history, now cleared.
            guard taken else { return }
            for text in keep {
                write(master, "\u{1b}[200~" + text + "\u{1b}[201~")
                Thread.sleep(forTimeInterval: 0.3)
                write(master, "\r")
                Thread.sleep(forTimeInterval: 0.3)
            }
        }
        return taken
    }

    /// The transcript has a `popAll` of `text` written since `since` (Claude Code 2.1.318 logs one
    /// queue-operation line per message Up took back).
    static func poppedAll(_ text: String, in transcript: String, since: Date) -> Bool {
        guard let handle = FileHandle(forReadingAtPath: transcript) else { return false }
        defer { try? handle.close() }
        let size = (try? handle.seekToEnd()) ?? 0
        try? handle.seek(toOffset: size > 65536 ? size - 65536 : 0)
        let tail = String(decoding: handle.readDataToEndOfFile(), as: UTF8.self)
        let iso = ISO8601DateFormatter()
        iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        for line in tail.split(separator: "\n").reversed() where line.contains("\"popAll\"") {
            guard let obj = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any],
                  obj["operation"] as? String == "popAll", obj["content"] as? String == text,
                  let stamp = (obj["timestamp"] as? String).flatMap(iso.date(from:)), stamp >= since else { continue }
            return true
        }
        return false
    }

    /// Attaches to the job, waits for its screen, runs `body` with the terminal and what it shows, then
    /// detaches. `settle`: go ahead after this long even if the screen never goes quiet (a working session).
    private static func attached(_ jobID: String, refuseDialog: Bool, settle: TimeInterval?,
                                 _ body: (Int32, TerminalScreen) -> Void) throws {
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
        let screen = TerminalScreen(rows: Int(size.ws_row), cols: Int(size.ws_col))
        if let settle {
            _ = waitForQuiet(master, quiet: 0.5, timeout: settle + 1, into: screen)
        } else {
            guard waitForQuiet(master, quiet: 0.5, timeout: 15, into: screen) else { throw Failure.timeout }
        }
        if refuseDialog, dialogOpen(screen) { throw Failure.dialog }
        body(master, screen)
        _ = waitForQuiet(master, quiet: 0.4, timeout: 3)
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
    private static func waitForQuiet(_ fd: Int32, quiet: TimeInterval, timeout: TimeInterval, into screen: TerminalScreen? = nil) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        var lastOutput: Date?
        var buffer = [UInt8](repeating: 0, count: 65536)
        while Date() < deadline {
            var pfd = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
            if poll(&pfd, 1, 100) > 0 {
                let n = read(fd, &buffer, buffer.count)
                if n <= 0 { return lastOutput != nil }
                screen?.feed(Array(buffer[0..<n]))
                lastOutput = Date()
            } else if let last = lastOutput, Date().timeIntervalSince(last) >= quiet {
                return true
            }
        }
        return lastOutput != nil
    }
}
