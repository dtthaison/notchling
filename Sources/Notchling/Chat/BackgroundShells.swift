import Darwin
import Foundation

/// A Bash call Claude Code ran with `run_in_background`: its tool result names an id and the file its
/// output goes to, and a `<task-notification>` with the same id says how it ended (`finishedAgents`).
/// Claude Code's own Background dialog (↓ in the terminal) lists the same shells.
struct BackgroundShell: Identifiable, Equatable {
    let id: String
    let command: String
    /// The Bash call's description, when it gave one.
    let title: String
    let outputPath: String
    let started: Date?
}

enum BackgroundShells {
    /// "Command running in background with ID: <id>. Output is being written to: <path>. …"
    static func launch(in result: String) -> (id: String, path: String)? {
        guard let idStart = result.range(of: "Command running in background with ID: ")?.upperBound else { return nil }
        let id = result[idStart...].prefix { $0.isLetter || $0.isNumber || $0 == "_" || $0 == "-" }
        guard !id.isEmpty, let pathStart = result.range(of: "Output is being written to: ")?.upperBound else { return nil }
        // The path ends at ".output", before the sentence's full stop.
        let rest = result[pathStart...]
        guard let end = rest.range(of: ".output")?.upperBound else { return nil }
        return (String(id), String(rest[..<end]))
    }

    enum State: Equatable {
        case running
        /// The notification's status (completed, failed, killed…), or "ended" when the shell is gone
        /// without one (the session stopped, or the notification hasn't been written yet).
        case ended(String)
    }

    /// Running while some process still has the output file open for writing (the shell and its
    /// children inherit it as stdout and stderr).
    static func state(_ shell: BackgroundShell, finished: [String: String]) -> State {
        if let status = finished[shell.id] { return .ended(status) }
        return writers(of: shell.outputPath).isEmpty ? .ended("ended") : .running
    }

    /// Processes with the file open for writing, from `lsof -F pcan`. Claude Code itself never is one,
    /// but it's excluded by name anyway, so Stop can't end the session.
    static func writers(of path: String) -> [pid_t] {
        guard FileManager.default.fileExists(atPath: path) else { return [] }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/lsof")
        process.arguments = ["-F", "pca", "--", path]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { return [] }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return parseWriters(String(decoding: data, as: UTF8.self))
    }

    /// `lsof -F pca` output: `p<pid>`, `c<command>`, then per file `f<fd>` and `a<mode>` (r, w or u).
    static func parseWriters(_ text: String) -> [pid_t] {
        var result: [pid_t] = []
        var pid: pid_t?
        var command = ""
        for line in text.split(separator: "\n") {
            guard let tag = line.first else { continue }
            let value = line.dropFirst()
            switch tag {
            case "p": pid = pid_t(value); command = ""
            case "c": command = String(value)
            case "a":
                guard let pid, value == "w" || value == "u", !result.contains(pid),
                      !["claude", "node", "Notchling"].contains(command) else { continue }
                result.append(pid)
            default: break
            }
        }
        return result
    }

    /// What Claude Code's `x` does: ends the shell and what it started. SIGTERM first, then SIGKILL for
    /// whatever is still writing two seconds later. Claude Code then reports the end in the session.
    static func stop(_ shell: BackgroundShell) {
        let path = shell.outputPath
        DispatchQueue.global(qos: .userInitiated).async {
            for pid in writers(of: path) { kill(pid, SIGTERM) }
            Thread.sleep(forTimeInterval: 2)
            for pid in writers(of: path) { kill(pid, SIGKILL) }
        }
    }
}
