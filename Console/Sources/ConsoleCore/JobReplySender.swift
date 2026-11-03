import Foundation

/// Replies to a background job by running `claude attach <id>` in a hidden pseudo-terminal, pasting the
/// text and pressing Enter, then detaching. The job keeps running after the client exits.
/// Same idea as Notchling's ReplySender, written again here so Console shares no code with it.
public struct JobReplySender: MessageSender {
    public init() {}

    public func send(_ text: String, to session: Session) async throws {
        guard case .job(let jobID) = session.control else { throw SendError.viewOnly }
        let chunks = TerminalKeys.replyChunks(text)
        try await Task.detached(priority: .userInitiated) {
            try Self.attachAndType(chunks, jobID: jobID)
        }.value
    }

    private static func attachAndType(_ chunks: [String], jobID: String) throws {
        let executable = try ClaudeExecutable.resolve()
        let master = posix_openpt(O_RDWR | O_NOCTTY)
        guard master >= 0, grantpt(master) == 0, unlockpt(master) == 0, let name = ptsname(master) else {
            throw SendError.failed("Không mở được pty để gửi tin.")
        }
        defer { close(master) }
        var size = winsize(ws_row: 50, ws_col: 200, ws_xpixel: 0, ws_ypixel: 0)
        _ = ioctl(master, TIOCSWINSZ, &size)
        guard let slave = FileHandle(forUpdatingAtPath: String(cString: name)) else {
            throw SendError.failed("Không mở được đầu slave của pty.")
        }

        let process = Process()
        process.executableURL = executable
        process.arguments = ["attach", jobID]
        process.standardInput = slave
        process.standardOutput = slave
        process.standardError = slave
        process.environment = SpawnEnvironment.scrubbed(ProcessInfo.processInfo.environment)
        do {
            try process.run()
        } catch {
            ConsoleLog.error("JobReplySender.attachAndType", executable.path, "launch failed: \(error)")
            throw SendError.failed("Không chạy được claude attach: \(error.localizedDescription)")
        }
        do { try slave.close() } catch { ConsoleLog.error("JobReplySender.attachAndType", jobID, "slave close: \(error)") }
        defer { if process.isRunning { process.terminate() } }

        // Wait for the session UI to draw and go quiet before typing.
        guard waitForQuiet(master, quiet: 1.2, timeout: 15) else {
            throw SendError.failed("Job không mở được trong 15 giây.")
        }
        for chunk in chunks {
            writeAll(master, chunk)
            Thread.sleep(forTimeInterval: 0.4)
        }
        _ = waitForQuiet(master, quiet: 1.0, timeout: 4)
    }

    private static func writeAll(_ fd: Int32, _ text: String) {
        let bytes = Array(text.utf8)
        var offset = 0
        while offset < bytes.count {
            let written = bytes[offset...].withUnsafeBufferPointer { Darwin.write(fd, $0.baseAddress, $0.count) }
            if written <= 0 { return }
            offset += written
        }
    }

    /// Drains output until nothing arrives for `quiet` seconds. False if nothing arrived before `timeout`.
    private static func waitForQuiet(_ fd: Int32, quiet: TimeInterval, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        var lastOutput: Date?
        var buffer = [UInt8](repeating: 0, count: 65536)
        while Date() < deadline {
            var descriptor = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
            if poll(&descriptor, 1, 100) > 0 {
                if read(fd, &buffer, buffer.count) <= 0 { return lastOutput != nil }
                lastOutput = Date()
            } else if let last = lastOutput, Date().timeIntervalSince(last) >= quiet {
                return true
            }
        }
        return lastOutput != nil
    }
}
