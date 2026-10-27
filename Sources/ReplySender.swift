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

    /// Blocking; call off the main thread.
    static func send(_ text: String, toJob jobID: String) throws {
        // Bracketed paste keeps newlines in the reply from submitting early.
        let flat = text.replacingOccurrences(of: "\r\n", with: "\n")
        try type(["\u{1b}[200~" + flat + "\u{1b}[201~", "\r"], toJob: jobID)
    }

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
        var env = ProcessInfo.processInfo.environment
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
