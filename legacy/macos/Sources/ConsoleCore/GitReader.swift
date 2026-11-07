import Foundation

public struct GitSnapshot: Equatable, Sendable {
    public var branch: String?
    public var diffStat: String?
}

/// Read-only git for the Flow tab: the branch from .git/HEAD, and `git diff --stat`, the only git
/// command Console runs. Optional locks are off so even refreshing the index never writes.
public enum GitReader {
    public static func read(cwd: String, timeout: TimeInterval = 3) -> GitSnapshot {
        guard !cwd.isEmpty, FileManager.default.fileExists(atPath: cwd) else { return GitSnapshot() }
        return GitSnapshot(branch: GitBranch.current(cwd: cwd), diffStat: diffStat(cwd: cwd, timeout: timeout))
    }

    static func diffStat(cwd: String, timeout: TimeInterval) -> String? {
        let git = "/usr/bin/git"
        guard FileManager.default.isExecutableFile(atPath: git) else { return nil }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: git)
        process.arguments = ["-C", cwd, "diff", "--stat", "--no-color", "--no-ext-diff"]
        var environment = ProcessInfo.processInfo.environment
        environment["GIT_OPTIONAL_LOCKS"] = "0"
        environment["GIT_TERMINAL_PROMPT"] = "0"
        environment["GIT_PAGER"] = "cat"
        process.environment = environment
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        process.standardInput = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            ConsoleLog.error("GitReader.diffStat", cwd, "launch failed: \(error)")
            return nil
        }

        var data = Data()
        let finished = DispatchSemaphore(value: 0)
        DispatchQueue.global(qos: .utility).async {
            data = output.fileHandleForReading.readDataToEndOfFile()
            finished.signal()
        }
        if finished.wait(timeout: .now() + timeout) == .timedOut {
            process.terminate()
            ConsoleLog.error("GitReader.diffStat", cwd, "timed out after \(timeout) s")
            return nil
        }
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { return nil }
        let text = String(decoding: data.prefix(16_000), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }
}
