import Foundation

public enum SpawnEnvironment {
    /// A session started from inside another Claude session inherits CLAUDE_CODE_* markers that change
    /// how it behaves (child session: no transcript), so every one of them is dropped.
    public static func scrubbed(_ environment: [String: String]) -> [String: String] {
        var result = environment.filter { key, _ in
            !key.hasPrefix("CLAUDE_CODE_") && key != "CLAUDECODE"
        }
        result["TERM"] = "xterm-256color"
        result["COLORTERM"] = "truecolor"
        return result
    }

    /// The environment a login shell would give a terminal, since a Finder-launched app has almost none
    /// (no PATH entries for ~/.local/bin or Homebrew). Falls back to this process's environment.
    public static func loginEnvironment(timeout: TimeInterval = 5) -> [String: String] {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-l", "-c", "/usr/bin/env -0"]
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            ConsoleLog.error("loginEnvironment", "/bin/zsh", "launch failed: \(error)")
            return ProcessInfo.processInfo.environment
        }
        let deadline = Date().addingTimeInterval(timeout)
        var data = Data()
        // Read on this thread while the shell runs; it exits right after printing.
        let reader = DispatchQueue(label: "local.agentconsole.env")
        let done = DispatchSemaphore(value: 0)
        reader.async {
            data = output.fileHandleForReading.readDataToEndOfFile()
            done.signal()
        }
        if done.wait(timeout: .now() + max(0, deadline.timeIntervalSinceNow)) == .timedOut {
            process.terminate()
            ConsoleLog.error("loginEnvironment", "/bin/zsh", "timed out")
            return ProcessInfo.processInfo.environment
        }
        let parsed = parse(data)
        return parsed.isEmpty ? ProcessInfo.processInfo.environment : parsed
    }

    static func parse(_ data: Data) -> [String: String] {
        var result: [String: String] = [:]
        for entry in data.split(separator: 0) {
            guard let text = String(data: Data(entry), encoding: .utf8), let equals = text.firstIndex(of: "=") else { continue }
            result[String(text[..<equals])] = String(text[text.index(after: equals)...])
        }
        return result
    }
}

public enum ClaudeExecutable {
    /// Where the CLI is usually installed, in the order Notchling prefers.
    public static func candidatePaths(home: URL = FileManager.default.homeDirectoryForCurrentUser) -> [String] {
        [home.appendingPathComponent(".local/bin/claude").path, "/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
    }

    public static func resolve(home: URL = FileManager.default.homeDirectoryForCurrentUser) throws -> URL {
        for path in candidatePaths(home: home) where FileManager.default.isExecutableFile(atPath: path) {
            return URL(fileURLWithPath: path)
        }
        throw SendError.failed("Không tìm thấy lệnh claude (~/.local/bin/claude).")
    }
}

/// Parent process id, for matching a Claude process to the pty child that started it.
public func parentProcessID(of pid: Int32) -> Int32? {
    var info = kinfo_proc()
    var size = MemoryLayout<kinfo_proc>.stride
    var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
    guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
    let parent = info.kp_eproc.e_ppid
    return parent > 1 ? parent : nil
}
